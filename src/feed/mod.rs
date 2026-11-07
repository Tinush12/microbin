pub mod api;
pub mod entry;
pub mod pages;
pub mod render;
pub mod store;
#[cfg(test)]
pub mod test_support;
pub mod upload;

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use actix_web::web;

use api::ApiError;
use store::{clear_temp_dir, Store, StoreResult};

#[derive(Clone)]
pub struct FeedConfig {
    pub data_dir: PathBuf,
    pub max_file_bytes: u64,
    pub title: String,
}

pub struct FeedState {
    pub store: Mutex<Store>,
    pub config: FeedConfig,
}

impl FeedState {
    pub fn open(config: FeedConfig) -> StoreResult<FeedState> {
        let store = Store::open(&config.data_dir)?;
        clear_temp_dir(&config.data_dir)?;
        Ok(FeedState { store: Mutex::new(store), config })
    }
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.app_data(web::QueryConfig::default().error_handler(|e, _| ApiError::BadRequest(e.to_string()).into()))
        .app_data(web::PathConfig::default().error_handler(|e, _| ApiError::BadRequest(e.to_string()).into()))
        .app_data(web::JsonConfig::default().error_handler(|e, _| ApiError::BadRequest(e.to_string()).into()))
        .app_data(web::FormConfig::default().error_handler(|e, _| ApiError::BadRequest(e.to_string()).into()))
        .service(
            web::resource("/api/entries")
                .route(web::get().to(api::list))
                .route(web::post().to(upload::create)),
        )
        .route("/api/entries/{id}", web::delete().to(api::delete))
        .route("/api/entries/{id}/expiry", web::put().to(api::set_expiry))
        .route("/api/changes", web::get().to(api::changes))
        .route("/api/search", web::get().to(api::search))
        .route("/", web::get().to(pages::feed))
        .route("/e/{id}", web::get().to(pages::entry_page))
        .route("/f/{id}", web::get().to(pages::file))
        .route("/u/{id}", web::get().to(pages::short_link))
        .route("/qr/{id}", web::get().to(pages::qr))
        .service(
            web::resource("/settings")
                .route(web::get().to(pages::settings))
                .route(web::post().to(pages::save_settings)),
        );
}

#[cfg(test)]
mod tests {
    use actix_web::{http::StatusCode, test, web, App};

    use super::*;

    #[actix_web::test]
    async fn opening_clears_uploads_left_by_a_crash() {
        let dir = tempfile::tempdir().unwrap();
        let tmp = store::temp_dir(dir.path());
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("stale"), b"partial").unwrap();
        let config = FeedConfig {
            data_dir: dir.path().to_path_buf(),
            max_file_bytes: 1,
            title: "t".to_string(),
        };
        FeedState::open(config).unwrap();
        assert!(!tmp.join("stale").exists());
    }

    #[actix_web::test]
    async fn unknown_paths_get_the_not_found_page() {
        let (_dir, state) = test_support::app_state(1_000);
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .configure(configure)
                .default_service(web::route().to(pages::fallback)),
        )
        .await;
        let req = test::TestRequest::get().uri("/upload/old-name").to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let body = test::read_body(res).await;
        assert!(String::from_utf8_lossy(&body).contains("Not found"));
    }
}
