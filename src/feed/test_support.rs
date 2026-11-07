use actix_web::web;

use super::store::Store;
use super::{FeedConfig, FeedState};

const BOUNDARY: &str = "feedtestboundary";

pub fn app_state(max_file_bytes: u64) -> (tempfile::TempDir, web::Data<FeedState>) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let config = FeedConfig {
        data_dir: dir.path().to_path_buf(),
        max_file_bytes,
    };
    let state = FeedState {
        store: std::sync::Mutex::new(store),
        config,
    };
    (dir, web::Data::new(state))
}

pub enum Part<'a> {
    Text(&'a str),
    File(&'a str, &'a [u8]),
}

pub fn multipart_content_type() -> String {
    format!("multipart/form-data; boundary={BOUNDARY}")
}

pub fn multipart_body(parts: &[Part]) -> Vec<u8> {
    let encoded = parts.iter().flat_map(|part| {
        let (head, data): (String, &[u8]) = match part {
            Part::Text(text) => (
                "Content-Disposition: form-data; name=\"text\"\r\n\r\n".to_string(),
                text.as_bytes(),
            ),
            Part::File(name, data) => (
                format!(
                    "Content-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\n\
                     Content-Type: application/octet-stream\r\n\r\n"
                ),
                data,
            ),
        };
        [format!("--{BOUNDARY}\r\n{head}").into_bytes(), data.to_vec(), b"\r\n".to_vec()].concat()
    });
    encoded.chain(format!("--{BOUNDARY}--\r\n").into_bytes()).collect()
}
