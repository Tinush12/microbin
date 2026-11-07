use std::fmt;
use std::sync::PoisonError;

use actix_web::http::StatusCode;
use actix_web::{web, HttpResponse, ResponseError};
use serde::{Deserialize, Serialize};

use super::entry::{Entry, Expiry};
use super::render::EntryJson;
use super::store::{PageQuery, Store, StoreError, StoreResult};
use super::{now, FeedState};

const PAGE_SIZE: usize = 50;
const SEARCH_LIMIT: usize = 50;
const INTERNAL_ERROR_MESSAGE: &str = "Something went wrong on the server";

#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    NotFound,
    TooLarge(String),
    Internal,
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::BadRequest(message) | ApiError::TooLarge(message) => f.write_str(message),
            ApiError::NotFound => f.write_str("Entry not found"),
            ApiError::Internal => f.write_str(INTERNAL_ERROR_MESSAGE),
        }
    }
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        match self {
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::NotFound => StatusCode::NOT_FOUND,
            ApiError::TooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            ApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code()).json(serde_json::json!({ "error": self.to_string() }))
    }
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        log::error!("feed store failure: {e}");
        ApiError::Internal
    }
}

/// Runs `op` on the store after removing expired entries; the lock covers only this call.
pub fn with_store<T>(
    state: &FeedState,
    op: impl FnOnce(&mut Store, i64) -> StoreResult<T>,
) -> Result<T, ApiError> {
    let at = now();
    // SQLite rolls back unfinished transactions, so a poisoned lock still guards a sound store.
    let mut store = state.store.lock().unwrap_or_else(PoisonError::into_inner);
    store.sweep_expired(at)?;
    Ok(op(&mut store, at)?)
}

pub fn to_json(entries: &[Entry]) -> Vec<EntryJson> {
    entries.iter().map(EntryJson::from).collect()
}

#[derive(Deserialize)]
pub struct ListParams {
    before: Option<i64>,
    after: Option<i64>,
    around: Option<i64>,
}

impl ListParams {
    fn page_query(&self) -> Result<PageQuery, ApiError> {
        match (self.before, self.after, self.around) {
            (None, None, None) => Ok(PageQuery::Latest),
            (Some(id), None, None) => Ok(PageQuery::Before(id)),
            (None, Some(id), None) => Ok(PageQuery::After(id)),
            (None, None, Some(id)) => Ok(PageQuery::Around(id)),
            _ => Err(ApiError::BadRequest(
                "Use at most one of before, after and around".to_string(),
            )),
        }
    }
}

#[derive(Serialize)]
struct ListResponse {
    entries: Vec<EntryJson>,
    has_before: bool,
    has_after: bool,
    count: i64,
    seq: i64,
}

pub async fn list(
    state: web::Data<FeedState>,
    params: web::Query<ListParams>,
) -> Result<HttpResponse, ApiError> {
    let query = params.page_query()?;
    let (page, count, seq) = with_store(&state, |store, at| {
        Ok((store.page(query, PAGE_SIZE, at)?, store.count(at)?, store.current_seq()?))
    })?;
    Ok(HttpResponse::Ok().json(ListResponse {
        entries: to_json(&page.entries),
        has_before: page.has_before,
        has_after: page.has_after,
        count,
        seq,
    }))
}

#[derive(Deserialize)]
pub struct ChangesParams {
    since: i64,
}

#[derive(Serialize)]
struct ChangesResponse {
    seq: i64,
    upserted: Vec<EntryJson>,
    removed: Vec<i64>,
    count: i64,
}

pub async fn changes(
    state: web::Data<FeedState>,
    params: web::Query<ChangesParams>,
) -> Result<HttpResponse, ApiError> {
    let (changes, count) = with_store(&state, |store, at| {
        Ok((store.changes_since(params.since, at)?, store.count(at)?))
    })?;
    Ok(HttpResponse::Ok().json(ChangesResponse {
        seq: changes.seq,
        upserted: to_json(&changes.upserted),
        removed: changes.removed,
        count,
    }))
}

#[derive(Deserialize)]
pub struct SearchParams {
    q: String,
}

pub async fn search(
    state: web::Data<FeedState>,
    params: web::Query<SearchParams>,
) -> Result<HttpResponse, ApiError> {
    let query = params.q.trim();
    if query.is_empty() {
        return Err(ApiError::BadRequest("Enter something to search for".to_string()));
    }
    let entries = with_store(&state, |store, at| store.search(query, SEARCH_LIMIT, at))?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "entries": to_json(&entries) })))
}

pub async fn delete(
    state: web::Data<FeedState>,
    id: web::Path<i64>,
) -> Result<HttpResponse, ApiError> {
    if with_store(&state, |store, _| store.delete(*id))? {
        Ok(HttpResponse::NoContent().finish())
    } else {
        Err(ApiError::NotFound)
    }
}

#[derive(Deserialize)]
pub struct ExpiryBody {
    expiry: String,
}

pub async fn set_expiry(
    state: web::Data<FeedState>,
    id: web::Path<i64>,
    body: web::Json<ExpiryBody>,
) -> Result<HttpResponse, ApiError> {
    let expiry = Expiry::parse(&body.expiry)
        .ok_or_else(|| ApiError::BadRequest(format!("Unknown expiry: {}", body.expiry)))?;
    let updated = with_store(&state, |store, at| store.set_expiry(*id, expiry, at))?;
    let entry = updated.ok_or(ApiError::NotFound)?;
    Ok(HttpResponse::Ok().json(EntryJson::from(&entry)))
}

#[cfg(test)]
mod tests {
    use actix_web::{http::StatusCode, test, App};
    use serde_json::{json, Value};

    use crate::feed::configure;
    use crate::feed::test_support::app_state;

    #[actix_web::test]
    async fn changes_report_new_and_deleted() {
        let (_dir, state) = app_state(1_000);
        let old = state.store.lock().unwrap().insert_text("old", 1).unwrap();
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let list: Value = test::call_and_read_body_json(
            &app,
            test::TestRequest::get().uri("/api/entries").to_request(),
        )
        .await;
        let seq = list["seq"].as_i64().unwrap();
        let new = state.store.lock().unwrap().insert_text("new", 2).unwrap();
        let del = test::TestRequest::delete().uri(&format!("/api/entries/{}", old.id)).to_request();
        assert_eq!(test::call_service(&app, del).await.status(), StatusCode::NO_CONTENT);
        let changes: Value = test::call_and_read_body_json(
            &app,
            test::TestRequest::get().uri(&format!("/api/changes?since={seq}")).to_request(),
        )
        .await;
        assert_eq!(changes["upserted"][0]["id"], new.id);
        assert_eq!(changes["removed"], json!([old.id]));
        assert_eq!(changes["count"], 1);
        assert!(changes["seq"].as_i64().unwrap() > seq);
    }

    #[actix_web::test]
    async fn delete_missing_is_404_with_json_error() {
        let (_dir, state) = app_state(1_000);
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let req = test::TestRequest::delete().uri("/api/entries/9999").to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let body: Value = test::read_body_json(res).await;
        assert!(body["error"].is_string());
    }

    #[actix_web::test]
    async fn expiry_endpoint() {
        let (_dir, state) = app_state(1_000);
        let now = crate::feed::now();
        let id = state.store.lock().unwrap().insert_text("x", now).unwrap().id;
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let put = |uri: String, expiry: &str| {
            test::TestRequest::put().uri(&uri).set_json(json!({ "expiry": expiry })).to_request()
        };
        let uri = format!("/api/entries/{id}/expiry");
        let ok = test::call_service(&app, put(uri.clone(), "1h")).await;
        assert_eq!(ok.status(), StatusCode::OK);
        let body: Value = test::read_body_json(ok).await;
        assert!(body["expires"].is_i64());
        let bad = test::call_service(&app, put(uri, "24hour")).await;
        assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
        let missing = test::call_service(&app, put("/api/entries/9999/expiry".into(), "1h")).await;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }

    #[actix_web::test]
    async fn search_requires_query() {
        let (_dir, state) = app_state(1_000);
        state.store.lock().unwrap().insert_text("hello", 1).unwrap();
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let empty = test::TestRequest::get().uri("/api/search?q=%20%20").to_request();
        assert_eq!(test::call_service(&app, empty).await.status(), StatusCode::BAD_REQUEST);
        let found: Value = test::call_and_read_body_json(
            &app,
            test::TestRequest::get().uri("/api/search?q=hel").to_request(),
        )
        .await;
        assert_eq!(found["entries"].as_array().unwrap().len(), 1);
    }

    #[actix_web::test]
    async fn list_pages_and_rejects_bad_queries() {
        let (_dir, state) = app_state(1_000);
        let ids: Vec<i64> = (0..60)
            .map(|i| state.store.lock().unwrap().insert_text("x", i + 1).unwrap().id)
            .collect();
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let latest: Value = test::call_and_read_body_json(
            &app,
            test::TestRequest::get().uri("/api/entries").to_request(),
        )
        .await;
        assert_eq!(latest["entries"].as_array().unwrap().len(), 50);
        assert_eq!(latest["count"], 60);
        assert_eq!(latest["has_before"], true);
        assert_eq!(latest["has_after"], false);
        let around: Value = test::call_and_read_body_json(
            &app,
            test::TestRequest::get().uri(&format!("/api/entries?around={}", ids[30])).to_request(),
        )
        .await;
        assert!(around["entries"].as_array().unwrap().iter().any(|e| e["id"] == ids[30]));
        for uri in ["/api/entries?before=1&after=2", "/api/entries?before=abc"] {
            let res = test::call_service(&app, test::TestRequest::get().uri(uri).to_request()).await;
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{uri}");
            let body: Value = test::read_body_json(res).await;
            assert!(body["error"].is_string(), "{uri}");
        }
    }

    #[actix_web::test]
    async fn requests_sweep_expired_entries() {
        let (_dir, state) = app_state(1_000);
        let id = {
            let mut store = state.store.lock().unwrap();
            let id = store.insert_text("x", 1).unwrap().id;
            store.set_expiry(id, crate::feed::entry::Expiry::Hour, 1).unwrap();
            id
        };
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let changes: Value = test::call_and_read_body_json(
            &app,
            test::TestRequest::get().uri("/api/changes?since=0").to_request(),
        )
        .await;
        assert_eq!(changes["removed"], json!([id]));
        assert_eq!(changes["count"], 0);
    }
}
