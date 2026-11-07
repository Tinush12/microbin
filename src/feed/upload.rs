use std::fs;
use std::io::Write;
use std::path::PathBuf;

use actix_multipart::{Field, Multipart};
use actix_web::{web, HttpRequest, HttpResponse};
use futures::TryStreamExt;
use tempfile::NamedTempFile;

use super::api::{require_same_origin, to_json, with_store, ApiError};
use super::entry::{safe_file_name, Entry};
use super::store::{temp_dir, Store, StoreResult};
use super::{FeedConfig, FeedState};

const MAX_TEXT_BYTES: usize = 16 * 1024 * 1024;
const BYTES_PER_MB: u64 = 1024 * 1024;
const TEXT_FIELD: &str = "text";
const FILE_FIELD: &str = "file";
const UNNAMED_UPLOAD: &str = "file";

#[derive(Clone)]
pub struct StagedFile {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
}

/// A received file; its temp file is deleted on drop unless the store moved it into place.
struct Upload {
    staged: StagedFile,
    _temp: NamedTempFile,
}

#[derive(Default)]
struct UploadForm {
    text: Option<String>,
    uploads: Vec<Upload>,
}

fn too_large(name: &str, limit_bytes: u64) -> ApiError {
    let limit_mb = limit_bytes.div_ceil(BYTES_PER_MB);
    ApiError::TooLarge(format!("{name} is larger than {limit_mb} MB"))
}

fn malformed(e: actix_multipart::MultipartError) -> ApiError {
    log::warn!("malformed upload: {e}");
    ApiError::BadRequest("The upload could not be read".to_string())
}

fn io_failure(e: std::io::Error) -> ApiError {
    log::error!("could not store upload: {e}");
    ApiError::Internal
}

async fn read_text(mut field: Field) -> Result<String, ApiError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = field.try_next().await.map_err(malformed)? {
        if bytes.len() + chunk.len() > MAX_TEXT_BYTES {
            return Err(too_large("Text", MAX_TEXT_BYTES as u64));
        }
        bytes.extend_from_slice(&chunk);
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| ApiError::BadRequest("Text is not valid UTF-8".to_string()))?;
    // Browsers turn every newline in a form value into CRLF when encoding it.
    Ok(text.replace("\r\n", "\n"))
}

async fn save_file(mut field: Field, config: &FeedConfig) -> Result<Upload, ApiError> {
    let name = field
        .content_disposition()
        .and_then(|disposition| disposition.get_filename())
        .unwrap_or(UNNAMED_UPLOAD)
        .to_string();
    let mut temp = NamedTempFile::new_in(temp_dir(&config.data_dir)).map_err(io_failure)?;
    let mut size: u64 = 0;
    while let Some(chunk) = field.try_next().await.map_err(malformed)? {
        size += chunk.len() as u64;
        if size > config.max_file_bytes {
            return Err(too_large(&safe_file_name(&name), config.max_file_bytes));
        }
        temp.write_all(&chunk).map_err(io_failure)?;
    }
    temp.flush().map_err(io_failure)?;
    let staged = StagedFile { name, path: temp.path().to_path_buf(), size };
    Ok(Upload { staged, _temp: temp })
}

async fn read_form(payload: &mut Multipart, config: &FeedConfig) -> Result<UploadForm, ApiError> {
    let mut form = UploadForm::default();
    while let Some(field) = payload.try_next().await.map_err(malformed)? {
        let field_name = field.name().unwrap_or_default().to_string();
        match field_name.as_str() {
            TEXT_FIELD => form.text = Some(read_text(field).await?),
            FILE_FIELD => form.uploads.push(save_file(field, config).await?),
            _ => log::debug!("ignoring upload field {field_name}"),
        }
    }
    Ok(form)
}

/// Reads what is left of the request so the browser gets the error instead of a reset connection.
async fn discard_rest(payload: &mut Multipart) {
    while let Ok(Some(mut field)) = payload.try_next().await {
        while let Ok(Some(_)) = field.try_next().await {}
    }
}

/// Creates the text entry, then one entry per file. On failure, removes what it already created.
pub fn create_entries(
    store: &mut Store,
    text: Option<&str>,
    files: &[StagedFile],
    at: i64,
) -> StoreResult<Vec<Entry>> {
    let mut created = Vec::with_capacity(files.len() + 1);
    let text_entry = text.map(|t| store.insert_text(t, at)).transpose();
    let outcome = text_entry.and_then(|entry| {
        created.extend(entry);
        files.iter().try_for_each(|file| {
            created.push(store.insert_file(&file.name, &file.path, file.size, at)?);
            Ok(())
        })
    });
    match outcome {
        Ok(()) => Ok(created),
        Err(e) => {
            for entry in &created {
                if let Err(cleanup) = store.delete(entry.id) {
                    log::error!("could not undo entry {}: {cleanup}", entry.id);
                }
            }
            Err(e)
        }
    }
}

pub async fn create(
    req: HttpRequest,
    state: web::Data<FeedState>,
    mut payload: Multipart,
) -> Result<HttpResponse, ApiError> {
    require_same_origin(&req)?;
    fs::create_dir_all(temp_dir(&state.config.data_dir)).map_err(io_failure)?;
    let form = match read_form(&mut payload, &state.config).await {
        Ok(form) => form,
        Err(e @ ApiError::TooLarge(_)) => {
            discard_rest(&mut payload).await;
            return Err(e);
        }
        Err(e) => return Err(e),
    };
    let text = form.text.as_deref().filter(|t| !t.trim().is_empty());
    if text.is_none() && form.uploads.is_empty() {
        return Err(ApiError::BadRequest("Nothing to send".to_string()));
    }
    let staged: Vec<StagedFile> = form.uploads.iter().map(|u| u.staged.clone()).collect();
    let entries = with_store(&state, |store, at| create_entries(store, text, &staged, at))?;
    Ok(HttpResponse::Created().json(serde_json::json!({ "entries": to_json(&entries) })))
}

#[cfg(test)]
mod tests {
    use actix_web::{http::StatusCode, test, App};
    use serde_json::Value;

    use super::*;
    use crate::feed::configure;
    use crate::feed::test_support::{app_state, multipart_body, multipart_content_type, Part};

    fn post(parts: &[Part]) -> test::TestRequest {
        test::TestRequest::post()
            .uri("/api/entries")
            .insert_header(("content-type", multipart_content_type()))
            .set_payload(multipart_body(parts))
    }

    #[actix_web::test]
    async fn send_text_and_two_files_creates_three_entries_in_order() {
        let (_dir, state) = app_state(1_000);
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let parts = [Part::File("a.png", b"png"), Part::Text("hello"), Part::File("b.txt", b"txt")];
        let res = test::call_service(&app, post(&parts).to_request()).await;
        assert_eq!(res.status(), StatusCode::CREATED);
        let body: Value = test::read_body_json(res).await;
        let entries = body["entries"].as_array().unwrap();
        let kinds: Vec<_> = entries.iter().map(|e| e["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["text", "image", "file"]);
        let ids: Vec<_> = entries.iter().map(|e| e["id"].as_i64().unwrap()).collect();
        assert!(ids[0] < ids[1] && ids[1] < ids[2]);
        assert_eq!(entries[0]["text"], "hello");
        assert_eq!(entries[2]["size"], 3);
    }

    #[actix_web::test]
    async fn oversized_file_creates_nothing() {
        let (dir, state) = app_state(10);
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let parts = [Part::Text("hi"), Part::File("ok.txt", b"fine"), Part::File("big.bin", &[0; 11])];
        let res = test::call_service(&app, post(&parts).to_request()).await;
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let body: Value = test::read_body_json(res).await;
        assert_eq!(body["error"], "big.bin is larger than 1 MB");
        let list: Value = test::call_and_read_body_json(
            &app,
            test::TestRequest::get().uri("/api/entries").to_request(),
        )
        .await;
        assert!(list["entries"].as_array().unwrap().is_empty());
        let temp_files = std::fs::read_dir(dir.path().join("feed_files/tmp")).unwrap().count();
        assert_eq!(temp_files, 0);
    }

    #[actix_web::test]
    async fn empty_send_is_rejected() {
        let (_dir, state) = app_state(1_000);
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let res = test::call_service(&app, post(&[Part::Text("   ")]).to_request()).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let body: Value = test::read_body_json(res).await;
        assert!(body["error"].is_string());
    }

    #[actix_web::test]
    async fn whitespace_text_with_a_file_creates_only_the_file() {
        let (_dir, state) = app_state(1_000);
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let parts = [Part::Text("  \n"), Part::File("a.txt", b"x")];
        let res = test::call_service(&app, post(&parts).to_request()).await;
        assert_eq!(res.status(), StatusCode::CREATED);
        let body: Value = test::read_body_json(res).await;
        assert_eq!(body["entries"].as_array().unwrap().len(), 1);
    }

    #[actix_web::test]
    async fn invalid_utf8_text_is_rejected() {
        let (_dir, state) = app_state(1_000);
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let body = [
            b"--feedtestboundary\r\nContent-Disposition: form-data; name=\"text\"\r\n\r\n".as_slice(),
            &[0xff, 0xfe],
            b"\r\n--feedtestboundary--\r\n",
        ]
        .concat();
        let req = test::TestRequest::post()
            .uri("/api/entries")
            .insert_header(("content-type", multipart_content_type()))
            .set_payload(body)
            .to_request();
        assert_eq!(test::call_service(&app, req).await.status(), StatusCode::BAD_REQUEST);
    }

    #[actix_web::test]
    async fn not_multipart_gets_a_json_error() {
        let (_dir, state) = app_state(1_000);
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let req = test::TestRequest::post()
            .uri("/api/entries")
            .insert_header(("content-type", "text/plain"))
            .set_payload("hello")
            .to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let body: Value = test::read_body_json(res).await;
        assert!(body["error"].is_string());
    }

    #[actix_web::test]
    async fn oversized_text_is_rejected() {
        let (_dir, state) = app_state(1_000);
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let huge = "x".repeat(MAX_TEXT_BYTES + 1);
        let res = test::call_service(&app, post(&[Part::Text(&huge)]).to_request()).await;
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(state.store.lock().unwrap().count(i64::MAX - 1).unwrap(), 0);
    }

    #[actix_web::test]
    async fn browser_line_endings_do_not_change_the_text() {
        let (_dir, state) = app_state(1_000);
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let res = test::call_service(&app, post(&[Part::Text("a\r\nb\nc")]).to_request()).await;
        let body: Value = test::read_body_json(res).await;
        assert_eq!(body["entries"][0]["text"], "a\nb\nc");
    }

    #[actix_web::test]
    async fn failure_midway_leaves_no_entries() {
        let (dir, state) = app_state(1_000);
        let good = dir.path().join("good.tmp");
        std::fs::write(&good, b"x").unwrap();
        let files = [
            StagedFile { name: "a.txt".into(), path: good, size: 1 },
            StagedFile { name: "b.txt".into(), path: dir.path().join("missing.tmp"), size: 1 },
        ];
        let mut store = state.store.lock().unwrap();
        assert!(create_entries(&mut store, Some("hello"), &files, 1).is_err());
        assert_eq!(store.count(2).unwrap(), 0);
        let on_disk = std::fs::read_dir(dir.path().join("feed_files")).unwrap().count();
        assert_eq!(on_disk, 0);
    }
}
