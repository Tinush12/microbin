use std::collections::HashSet;

use bytesize::ByteSize;
use html_escape::{encode_double_quoted_attribute, encode_text};
use pulldown_cmark::{html, Event, Options, Parser};
use serde::Serialize;

use super::entry::{link_domain, Entry, EntryKind, StoredFile};

const LANGUAGE_CLASS_PREFIX: &str = "language-";
const LINK_REL: &str = "noopener noreferrer";
const ALLOWED_URL_SCHEMES: [&str; 3] = ["http", "https", "mailto"];
const MAX_EXTENSION_LABEL_CHARS: usize = 4;
const NO_EXTENSION_LABEL: &str = "FILE";

#[derive(Serialize)]
pub struct EntryJson {
    pub id: i64,
    pub kind: EntryKind,
    pub created: i64,
    pub expires: Option<i64>,
    pub text: Option<String>,
    pub file_name: Option<String>,
    pub size: Option<u64>,
    pub body_html: String,
}

impl From<&Entry> for EntryJson {
    fn from(entry: &Entry) -> Self {
        EntryJson {
            id: entry.id,
            kind: entry.kind,
            created: entry.created,
            expires: entry.expires,
            text: entry.text.clone(),
            file_name: entry.file.as_ref().map(|f| f.name.clone()),
            size: entry.file.as_ref().map(|f| f.size),
            body_html: body_html(entry),
        }
    }
}

fn is_language_class(value: &str) -> bool {
    value.starts_with(LANGUAGE_CLASS_PREFIX) && !value.contains(char::is_whitespace)
}

fn sanitize(unsafe_html: &str) -> String {
    ammonia::Builder::default()
        .add_tag_attributes("code", ["class"])
        .attribute_filter(|element, attribute, value| match (element, attribute) {
            ("code", "class") if !is_language_class(value) => None,
            _ => Some(value.into()),
        })
        .url_schemes(HashSet::from(ALLOWED_URL_SCHEMES))
        .link_rel(Some(LINK_REL))
        .set_tag_attribute_value("a", "target", "_blank")
        .clean(unsafe_html)
        .to_string()
}

pub fn markdown_to_html(text: &str) -> String {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    let events = Parser::new_ext(text, options).map(|event| match event {
        Event::SoftBreak => Event::HardBreak,
        other => other,
    });
    let mut rendered = String::new();
    html::push_html(&mut rendered, events);
    sanitize(&rendered)
}

fn extension_label(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => {
            ext.chars().take(MAX_EXTENSION_LABEL_CHARS).collect::<String>().to_uppercase()
        }
        _ => NO_EXTENSION_LABEL.to_string(),
    }
}

fn link_body(url: &str) -> String {
    format!(
        r#"<a href="{href}" target="_blank" rel="{LINK_REL}">{label}</a><div class="domain">{domain}</div>"#,
        href = encode_double_quoted_attribute(url),
        label = encode_text(url),
        domain = encode_text(&link_domain(url)),
    )
}

fn image_body(id: i64, file: &StoredFile) -> String {
    format!(
        r#"<a href="/f/{id}" target="_blank"><img class="shot" src="/f/{id}" alt="{alt}" loading="lazy"></a>"#,
        alt = encode_double_quoted_attribute(&file.name),
    )
}

fn file_body(file: &StoredFile) -> String {
    format!(
        r#"<div class="file"><div class="ext">{ext}</div><div><div class="fname">{name}</div><div class="fsize">{size}</div></div></div>"#,
        ext = encode_text(&extension_label(&file.name)),
        name = encode_text(&file.name),
        size = ByteSize::b(file.size),
    )
}

pub fn body_html(entry: &Entry) -> String {
    let text = entry.text.as_deref().unwrap_or_default();
    match (entry.kind, entry.file.as_ref()) {
        (EntryKind::Text, _) => format!(r#"<div class="md">{}</div>"#, markdown_to_html(text)),
        (EntryKind::Link, _) => link_body(text),
        (EntryKind::Image, Some(file)) => image_body(entry.id, file),
        (EntryKind::Video, Some(_)) => {
            format!(r#"<video class="shot" controls preload="metadata" src="/f/{}"></video>"#, entry.id)
        }
        (EntryKind::File, Some(file)) => file_body(file),
        (_, None) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed::entry::{EntryKind, StoredFile};

    fn file_entry(kind: EntryKind, name: &str, size: u64) -> Entry {
        Entry {
            id: 7,
            kind,
            text: None,
            file: Some(StoredFile { name: name.into(), size }),
            created: 0,
            expires: None,
        }
    }

    #[test]
    fn markdown_renders_with_chat_line_breaks() {
        let h = markdown_to_html("# Todo\n- a\n- b\n\nline1\nline2");
        assert!(h.contains("<h1>Todo</h1>") && h.contains("<li>a</li>") && h.contains("line1<br>"));
    }

    #[test]
    fn markdown_is_sanitized() {
        let combined = markdown_to_html("<script>alert(1)</script>[x](javascript:alert(1)) <img src=x onerror=alert(1)>");
        assert!(!combined.contains("<script") && !combined.contains("href") && !combined.contains("onerror"));
        for hostile in [
            "[x](javascript:alert(1))",
            "<a href=\"javascript:alert(1)\">y</a>",
            "![x](javascript:alert(1))",
            "<img src=x onerror=alert(1)>",
        ] {
            let h = markdown_to_html(hostile);
            assert!(!h.contains("javascript:") && !h.contains("onerror"), "{hostile} -> {h}");
        }
    }

    #[test]
    fn code_fences_keep_language_class() {
        let h = markdown_to_html("```rust\nfn main() {}\n```");
        assert!(h.contains(r#"<code class="language-rust">"#));
    }

    #[test]
    fn only_language_classes_survive() {
        let h = markdown_to_html(r#"<code class="evil">a</code> <code class="language-x evil">b</code>"#);
        assert!(!h.contains("class="), "{h}");
    }

    #[test]
    fn markdown_links_open_safely() {
        let h = markdown_to_html("[x](https://a.b) [m](mailto:a@b.c) [f](ftp://a.b)");
        assert!(h.contains(r#"href="https://a.b""#) && h.contains(r#"href="mailto:a@b.c""#));
        assert!(h.contains(r#"target="_blank""#) && h.contains(r#"rel="noopener noreferrer""#));
        assert!(!h.contains("ftp:"));
    }

    #[test]
    fn link_body_is_escaped() {
        let e = Entry {
            id: 1,
            kind: EntryKind::Link,
            text: Some("https://a.b/?q=\"><x>".into()),
            file: None,
            created: 0,
            expires: None,
        };
        let h = body_html(&e);
        assert!(!h.contains("<x>") && h.contains(r#"<div class="domain">a.b</div>"#));
    }

    #[test]
    fn file_body() {
        let h = body_html(&file_entry(EntryKind::File, "nas.tar.gz", 48_200_000));
        assert!(h.contains(">GZ<") && h.contains("nas.tar.gz") && h.contains("MB"));
    }

    #[test]
    fn file_extension_label_is_capped() {
        assert!(body_html(&file_entry(EntryKind::File, "a.torrent", 1)).contains(">TORR<"));
        assert!(body_html(&file_entry(EntryKind::File, "README", 1)).contains(">FILE<"));
    }

    #[test]
    fn image_and_video_bodies_point_at_the_file_route() {
        let img = body_html(&file_entry(EntryKind::Image, "a\"<b>.png", 1));
        assert!(img.contains(r#"src="/f/7""#) && img.contains(r#"loading="lazy""#) && !img.contains("<b>"));
        let vid = body_html(&file_entry(EntryKind::Video, "a.webm", 1));
        assert!(vid.contains("<video") && vid.contains(r#"src="/f/7""#));
    }

    #[test]
    fn entry_json_flattens_the_file() {
        let j = EntryJson::from(&file_entry(EntryKind::File, "a.bin", 9));
        assert_eq!(j.file_name.as_deref(), Some("a.bin"));
        assert_eq!(j.size, Some(9));
        assert_eq!(serde_json::to_value(&j).unwrap()["kind"], "file");
    }
}
