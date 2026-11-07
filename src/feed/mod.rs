pub mod api;
pub mod entry;
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
use store::Store;

#[derive(Clone)]
pub struct FeedConfig {
    pub data_dir: PathBuf,
    pub max_file_bytes: u64,
}

pub struct FeedState {
    pub store: Mutex<Store>,
    pub config: FeedConfig,
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
        .service(
            web::resource("/api/entries")
                .route(web::get().to(api::list))
                .route(web::post().to(upload::create)),
        )
        .route("/api/entries/{id}", web::delete().to(api::delete))
        .route("/api/entries/{id}/expiry", web::put().to(api::set_expiry))
        .route("/api/changes", web::get().to(api::changes))
        .route("/api/search", web::get().to(api::search));
}
