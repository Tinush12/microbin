use std::io;

use actix_files::NamedFile;
use actix_web::http::header::{
    self, Charset, ContentDisposition, DispositionParam, DispositionType, ExtendedValue,
};
use actix_web::{web, HttpRequest, HttpResponse};
use askama::Template;
use bytesize::ByteSize;
use html_escape::encode_double_quoted_attribute;
use mime_guess::{mime, Mime};
use qrcode_generator::QrCodeEcc;
use serde::Deserialize;

use super::api::{require_same_origin, with_store, ApiError};
use super::entry::{Entry, EntryKind, Expiry, StoredFile};
use super::render::body_html;
use super::FeedState;

const QR_SIZE_PX: usize = 256;
const SANDBOX_POLICY: &str = "default-src 'none'; style-src 'unsafe-inline'; sandbox";
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Template)]
#[template(path = "feed.html")]
struct FeedPage<'a> {
    title: &'a str,
    count: i64,
    seq: i64,
    around_attr: String,
}

#[derive(Template)]
#[template(path = "entry.html")]
struct EntryPage<'a> {
    title: &'a str,
    body: String,
    created_utc: String,
    id: i64,
    open_attr: Option<String>,
}

#[derive(Template)]
#[template(path = "entry_qr.html")]
struct QrPage<'a> {
    title: &'a str,
    svg: String,
    target_attr: String,
}

struct ExpiryOption {
    value: &'static str,
    label: &'static str,
    selected: bool,
}

#[derive(Template)]
#[template(path = "settings.html")]
struct SettingsPage<'a> {
    title: &'a str,
    options: Vec<ExpiryOption>,
    version: &'static str,
    count: i64,
    storage: String,
}

#[derive(Template)]
#[template(path = "not_found.html")]
struct NotFoundPage<'a> {
    title: &'a str,
}

fn html(page: &impl Template) -> Result<HttpResponse, ApiError> {
    let rendered = page.render().map_err(|e| {
        log::error!("could not render page: {e}");
        ApiError::Internal
    })?;
    Ok(HttpResponse::Ok().content_type("text/html; charset=utf-8").body(rendered))
}

fn not_found(state: &FeedState) -> Result<HttpResponse, ApiError> {
    let page = html(&NotFoundPage { title: &state.config.title })?;
    let (_, body) = page.into_parts();
    Ok(HttpResponse::NotFound()
        .content_type("text/html; charset=utf-8")
        .body(body))
}

pub async fn fallback(state: web::Data<FeedState>) -> Result<HttpResponse, ApiError> {
    not_found(&state)
}

/// Looks up an id from a URL. Anything that is not an integer is simply not an entry.
fn find_entry(state: &FeedState, raw_id: &str) -> Result<Option<Entry>, ApiError> {
    match raw_id.parse::<i64>() {
        Ok(id) => with_store(state, |store, at| store.get(id, at)),
        Err(_) => Ok(None),
    }
}

#[derive(Deserialize)]
pub struct FeedParams {
    around: Option<i64>,
}

pub async fn feed(
    state: web::Data<FeedState>,
    params: web::Query<FeedParams>,
) -> Result<HttpResponse, ApiError> {
    let (count, seq) = with_store(&state, |store, at| Ok((store.count(at)?, store.current_seq()?)))?;
    html(&FeedPage {
        title: &state.config.title,
        count,
        seq,
        around_attr: params.around.map_or(String::new(), |id| format!(" data-around=\"{id}\"")),
    })
}

fn format_utc(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp(timestamp, 0)
        .map_or(String::new(), |time| time.format("%Y-%m-%d %H:%M UTC").to_string())
}

pub async fn entry_page(
    state: web::Data<FeedState>,
    id: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let Some(entry) = find_entry(&state, &id)? else {
        return not_found(&state);
    };
    let open_target = match entry.kind {
        EntryKind::Text => None,
        EntryKind::Link => entry.text.clone(),
        _ => Some(format!("/f/{}", entry.id)),
    };
    html(&EntryPage {
        title: &state.config.title,
        body: body_html(&entry),
        created_utc: format_utc(entry.created),
        id: entry.id,
        open_attr: open_target.map(|target| encode_double_quoted_attribute(&target).into_owned()),
    })
}

fn disposition(file: &StoredFile, download: bool) -> ContentDisposition {
    let parameter = if file.name.is_ascii() {
        DispositionParam::Filename(file.name.clone())
    } else {
        DispositionParam::FilenameExt(ExtendedValue {
            charset: Charset::Ext("UTF-8".to_string()),
            language_tag: None,
            value: file.name.clone().into_bytes(),
        })
    };
    ContentDisposition {
        disposition: if download { DispositionType::Attachment } else { DispositionType::Inline },
        parameters: vec![parameter],
    }
}

/// Types a browser would execute as a page; serving them plainly would run scripts on our origin.
fn is_active_content(mime_type: &Mime) -> bool {
    let is_xml_family = mime_type.subtype() == mime::XML || mime_type.suffix() == Some(mime::XML);
    mime_type.subtype() == mime::HTML || is_xml_family
}

#[derive(Deserialize)]
pub struct FileParams {
    download: Option<String>,
}

pub async fn file(
    req: HttpRequest,
    state: web::Data<FeedState>,
    id: web::Path<String>,
    params: web::Query<FileParams>,
) -> Result<HttpResponse, ApiError> {
    let found = with_entry_file(&state, &id)?;
    let Some((stored, path)) = found else {
        return not_found(&state);
    };
    let named = match NamedFile::open_async(&path).await {
        Ok(named) => named,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return not_found(&state),
        Err(e) => {
            log::error!("could not open {}: {e}", path.display());
            return Err(ApiError::Internal);
        }
    };
    let mime_type = mime_guess::from_path(&stored.name).first_or_octet_stream();
    let sandbox = is_active_content(&mime_type);
    let mut response = named
        .set_content_type(mime_type)
        .set_content_disposition(disposition(&stored, params.download.is_some()))
        .into_response(&req);
    let headers = response.headers_mut();
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, header::HeaderValue::from_static("nosniff"));
    if sandbox {
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            header::HeaderValue::from_static(SANDBOX_POLICY),
        );
    }
    Ok(response)
}

fn with_entry_file(
    state: &FeedState,
    raw_id: &str,
) -> Result<Option<(StoredFile, std::path::PathBuf)>, ApiError> {
    let Ok(id) = raw_id.parse::<i64>() else {
        return Ok(None);
    };
    with_store(state, |store, at| {
        let entry = store.get(id, at)?;
        Ok(entry.and_then(|e| Some((e.file.clone()?, store.file_path(&e)?))))
    })
}

pub async fn short_link(
    state: web::Data<FeedState>,
    id: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    match find_entry(&state, &id)? {
        Some(Entry { kind: EntryKind::Link, text: Some(url), .. }) => {
            Ok(HttpResponse::Found().insert_header((header::LOCATION, url)).finish())
        }
        _ => not_found(&state),
    }
}

pub async fn qr(
    req: HttpRequest,
    state: web::Data<FeedState>,
    id: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let Some(entry) = find_entry(&state, &id)? else {
        return not_found(&state);
    };
    let route = if entry.kind == EntryKind::Link { "u" } else { "e" };
    let target = {
        let info = req.connection_info();
        format!("{}://{}/{route}/{}", info.scheme(), info.host(), entry.id)
    };
    let svg = qrcode_generator::to_svg_to_string(&target, QrCodeEcc::Low, QR_SIZE_PX, None::<&str>)
        .map_err(|e| {
            log::error!("could not build qr code: {e}");
            ApiError::Internal
        })?;
    html(&QrPage {
        title: &state.config.title,
        svg,
        target_attr: encode_double_quoted_attribute(&target).into_owned(),
    })
}

pub async fn settings(state: web::Data<FeedState>) -> Result<HttpResponse, ApiError> {
    let (current, count, used) = with_store(&state, |store, at| {
        Ok((store.default_expiry()?, store.count(at)?, store.storage_used(at)?))
    })?;
    html(&SettingsPage {
        title: &state.config.title,
        options: Expiry::ALL
            .into_iter()
            .map(|e| ExpiryOption { value: e.as_str(), label: e.label(), selected: e == current })
            .collect(),
        version: VERSION,
        count,
        storage: ByteSize::b(used).to_string(),
    })
}

#[derive(Deserialize)]
pub struct SettingsForm {
    default_expiry: String,
}

pub async fn save_settings(
    req: HttpRequest,
    state: web::Data<FeedState>,
    form: web::Form<SettingsForm>,
) -> Result<HttpResponse, ApiError> {
    require_same_origin(&req)?;
    let expiry = Expiry::parse(&form.default_expiry)
        .ok_or_else(|| ApiError::BadRequest(format!("Unknown expiry: {}", form.default_expiry)))?;
    with_store(&state, |store, _| store.set_default_expiry(expiry))?;
    Ok(HttpResponse::SeeOther().insert_header((header::LOCATION, "/settings")).finish())
}

#[cfg(test)]
mod tests {
    use actix_web::{body::to_bytes, http::StatusCode, test, App};

    use crate::feed::configure;
    use crate::feed::now;
    use crate::feed::test_support::{app_state, multipart_body, multipart_content_type, Part};

    fn file_entry(dir: &tempfile::TempDir, state: &crate::feed::FeedState, name: &str, data: &[u8]) -> i64 {
        let tmp = dir.path().join(format!("{name}.upload"));
        std::fs::write(&tmp, data).unwrap();
        let mut store = state.store.lock().unwrap();
        store.insert_file(name, &tmp, data.len() as u64, now()).unwrap().id
    }

    fn header(res: &actix_web::dev::ServiceResponse, name: &str) -> String {
        res.headers().get(name).map_or(String::new(), |v| v.to_str().unwrap().to_string())
    }

    #[actix_web::test]
    async fn feed_page_has_composer_and_seq() {
        let (_dir, state) = app_state(1_000);
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let res = test::call_service(&app, test::TestRequest::get().uri("/").to_request()).await;
        assert_eq!(res.status(), StatusCode::OK);
        let body = String::from_utf8(to_bytes(res.into_body()).await.unwrap().to_vec()).unwrap();
        for needle in ["id=\"input\"", "data-seq=", "id=\"feed\"", "id=\"dropzone\"", "/static/feed.js", "/static/composer.js"] {
            assert!(body.contains(needle), "{needle}");
        }
        assert!(!body.contains("data-around"));
    }

    #[actix_web::test]
    async fn feed_page_carries_the_jump_target() {
        let (_dir, state) = app_state(1_000);
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let body = test::call_and_read_body(&app, test::TestRequest::get().uri("/?around=12").to_request()).await;
        assert!(String::from_utf8_lossy(&body).contains("data-around=\"12\""));
    }

    #[actix_web::test]
    async fn file_download_headers() {
        let (dir, state) = app_state(1_000);
        let id = file_entry(&dir, &state, "a.png", b"png");
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let inline = test::call_service(&app, test::TestRequest::get().uri(&format!("/f/{id}")).to_request()).await;
        assert_eq!(inline.status(), StatusCode::OK);
        assert_eq!(header(&inline, "content-type"), "image/png");
        assert!(header(&inline, "content-disposition").starts_with("inline"));
        assert_eq!(header(&inline, "x-content-type-options"), "nosniff");
        assert_eq!(header(&inline, "content-security-policy"), "");
        let download = test::call_service(&app, test::TestRequest::get().uri(&format!("/f/{id}?download=1")).to_request()).await;
        assert!(header(&download, "content-disposition").starts_with("attachment"));
    }

    #[actix_web::test]
    async fn active_content_is_sandboxed() {
        let (dir, state) = app_state(1_000);
        let html = file_entry(&dir, &state, "page.html", b"<script>alert(1)</script>");
        let svg = file_entry(&dir, &state, "pic.svg", b"<svg onload='alert(1)'/>");
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        for id in [html, svg] {
            let res = test::call_service(&app, test::TestRequest::get().uri(&format!("/f/{id}")).to_request()).await;
            assert!(header(&res, "content-security-policy").contains("sandbox"), "{id}");
        }
    }

    #[actix_web::test]
    async fn non_ascii_names_survive_in_the_download_header() {
        let (dir, state) = app_state(1_000);
        let id = file_entry(&dir, &state, "Grüße.txt", b"x");
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let res = test::call_service(&app, test::TestRequest::get().uri(&format!("/f/{id}")).to_request()).await;
        assert!(header(&res, "content-disposition").contains("filename*=UTF-8''Gr%C3%BC%C3%9Fe.txt"));
    }

    #[actix_web::test]
    async fn videos_can_seek() {
        let (dir, state) = app_state(1_000);
        let id = file_entry(&dir, &state, "a.webm", b"0123456789");
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let req = test::TestRequest::get().uri(&format!("/f/{id}")).insert_header(("range", "bytes=2-4")).to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(&to_bytes(res.into_body()).await.unwrap()[..], b"234");
    }

    #[actix_web::test]
    async fn file_route_is_404_for_text_entries() {
        let (_dir, state) = app_state(1_000);
        let id = state.store.lock().unwrap().insert_text("x", now()).unwrap().id;
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let res = test::call_service(&app, test::TestRequest::get().uri(&format!("/f/{id}")).to_request()).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[actix_web::test]
    async fn short_link_redirects_only_links() {
        let (_dir, state) = app_state(1_000);
        let (link, text) = {
            let mut store = state.store.lock().unwrap();
            let link = store.insert_text("https://a.b/c", now()).unwrap().id;
            (link, store.insert_text("plain", now()).unwrap().id)
        };
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let res = test::call_service(&app, test::TestRequest::get().uri(&format!("/u/{link}")).to_request()).await;
        assert_eq!(res.status(), StatusCode::FOUND);
        assert_eq!(header(&res, "location"), "https://a.b/c");
        let res = test::call_service(&app, test::TestRequest::get().uri(&format!("/u/{text}")).to_request()).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[actix_web::test]
    async fn qr_points_at_entry() {
        let (_dir, state) = app_state(1_000);
        let id = state.store.lock().unwrap().insert_text("x", now()).unwrap().id;
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let req = test::TestRequest::get().uri(&format!("/qr/{id}")).insert_header(("host", "nas.lan:8080")).to_request();
        let body = String::from_utf8(test::call_and_read_body(&app, req).await.to_vec()).unwrap();
        assert!(body.contains("<svg"));
        assert!(body.contains(&format!("http://nas.lan:8080/e/{id}")));
    }

    #[actix_web::test]
    async fn entry_page_shows_the_body_and_rejects_bad_ids() {
        let (_dir, state) = app_state(1_000);
        let id = state.store.lock().unwrap().insert_text("**bold** <script>x</script>", now()).unwrap().id;
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let body = test::call_and_read_body(&app, test::TestRequest::get().uri(&format!("/e/{id}")).to_request()).await;
        let body = String::from_utf8_lossy(&body);
        assert!(body.contains("<strong>bold</strong>") && !body.contains("<script>x"));
        let res = test::call_service(&app, test::TestRequest::get().uri("/e/abc").to_request()).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[actix_web::test]
    async fn settings_round_trip() {
        let (dir, state) = app_state(1_000);
        file_entry(&dir, &state, "a.bin", &[0; 2_000]);
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let post = |value: &str| {
            test::TestRequest::post()
                .uri("/settings")
                .insert_header(("content-type", "application/x-www-form-urlencoded"))
                .set_payload(format!("default_expiry={value}"))
                .to_request()
        };
        let saved = test::call_service(&app, post("1w")).await;
        assert_eq!(saved.status(), StatusCode::SEE_OTHER);
        assert_eq!(header(&saved, "location"), "/settings");
        let page = test::call_and_read_body(&app, test::TestRequest::get().uri("/settings").to_request()).await;
        let page = String::from_utf8_lossy(&page);
        assert!(page.contains(r#"<option value="1w" selected>1 week</option>"#), "{page}");
        assert!(page.contains("2.0 KB"), "{page}");
        let bogus = test::call_service(&app, post("bogus")).await;
        assert_eq!(bogus.status(), StatusCode::BAD_REQUEST);
    }

    #[actix_web::test]
    async fn expired_entry_page_is_404() {
        let (_dir, state) = app_state(1_000);
        let id = {
            let mut store = state.store.lock().unwrap();
            let id = store.insert_text("x", now() - 7_200).unwrap().id;
            store.set_expiry(id, crate::feed::entry::Expiry::Hour, now() - 7_200).unwrap();
            id
        };
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let res = test::call_service(&app, test::TestRequest::get().uri(&format!("/e/{id}")).to_request()).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[actix_web::test]
    async fn other_sites_cannot_post() {
        let (_dir, state) = app_state(1_000);
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure)).await;
        let settings = test::TestRequest::post()
            .uri("/settings")
            .insert_header(("host", "nas.lan:8080"))
            .insert_header(("origin", "https://evil.example"))
            .insert_header(("content-type", "application/x-www-form-urlencoded"))
            .set_payload("default_expiry=1h")
            .to_request();
        assert_eq!(test::call_service(&app, settings).await.status(), StatusCode::FORBIDDEN);
        let upload = |origin: &'static str| {
            test::TestRequest::post()
                .uri("/api/entries")
                .insert_header(("host", "nas.lan:8080"))
                .insert_header(("origin", origin))
                .insert_header(("content-type", multipart_content_type()))
                .set_payload(multipart_body(&[Part::Text("hi")]))
                .to_request()
        };
        assert_eq!(test::call_service(&app, upload("https://evil.example")).await.status(), StatusCode::FORBIDDEN);
        assert_eq!(test::call_service(&app, upload("http://nas.lan:8080")).await.status(), StatusCode::CREATED);
    }
}
