use serde::Serialize;

const IMAGE_EXTENSIONS: [&str; 16] = [
    ".jpg", ".jpeg", ".png", ".gif", ".bmp", ".webp", ".ico", ".svg", ".tiff", ".tif", ".jfif",
    ".pjpeg", ".pjp", ".avif", ".jxl", ".heif",
];
const VIDEO_EXTENSIONS: [&str; 8] = [
    ".mp4", ".mov", ".wmv", ".webm", ".avi", ".flv", ".mkv", ".mts",
];
const FALLBACK_FILE_NAME: &str = "file";
const LINK_SCHEMES: [&str; 2] = ["http://", "https://"];

const HOUR_SECS: i64 = 3_600;
const DAY_SECS: i64 = 24 * HOUR_SECS;
const WEEK_SECS: i64 = 7 * DAY_SECS;

#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Text,
    Link,
    Image,
    Video,
    File,
}

impl EntryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntryKind::Text => "text",
            EntryKind::Link => "link",
            EntryKind::Image => "image",
            EntryKind::Video => "video",
            EntryKind::File => "file",
        }
    }

    pub fn parse(value: &str) -> Option<EntryKind> {
        match value {
            "text" => Some(EntryKind::Text),
            "link" => Some(EntryKind::Link),
            "image" => Some(EntryKind::Image),
            "video" => Some(EntryKind::Video),
            "file" => Some(EntryKind::File),
            _ => None,
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct StoredFile {
    pub name: String,
    pub size: u64,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Entry {
    pub id: i64,
    pub kind: EntryKind,
    pub text: Option<String>,
    pub file: Option<StoredFile>,
    pub created: i64,
    pub expires: Option<i64>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Expiry {
    Never,
    Hour,
    Day,
    Week,
}

impl Expiry {
    pub const ALL: [Expiry; 4] = [Expiry::Never, Expiry::Hour, Expiry::Day, Expiry::Week];

    pub fn parse(value: &str) -> Option<Expiry> {
        Expiry::ALL.into_iter().find(|e| e.as_str() == value)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Expiry::Never => "never",
            Expiry::Hour => "1h",
            Expiry::Day => "1d",
            Expiry::Week => "1w",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Expiry::Never => "Never",
            Expiry::Hour => "1 hour",
            Expiry::Day => "1 day",
            Expiry::Week => "1 week",
        }
    }

    pub fn deadline(&self, now: i64) -> Option<i64> {
        match self {
            Expiry::Never => None,
            Expiry::Hour => Some(now + HOUR_SECS),
            Expiry::Day => Some(now + DAY_SECS),
            Expiry::Week => Some(now + WEEK_SECS),
        }
    }
}

pub fn kind_for_text(text: &str) -> EntryKind {
    let trimmed = text.trim();
    let is_url = !trimmed.contains(char::is_whitespace)
        && LINK_SCHEMES.iter().any(|s| trimmed.starts_with(s));
    if is_url {
        EntryKind::Link
    } else {
        EntryKind::Text
    }
}

pub fn kind_for_file(name: &str) -> EntryKind {
    let lowercase = name.to_lowercase();
    let has_extension = |list: &[&str]| list.iter().any(|ext| lowercase.ends_with(ext));
    if has_extension(&IMAGE_EXTENSIONS) {
        EntryKind::Image
    } else if has_extension(&VIDEO_EXTENSIONS) {
        EntryKind::Video
    } else {
        EntryKind::File
    }
}

pub fn safe_file_name(raw: &str) -> String {
    let last_component = raw.rsplit(['/', '\\']).next().unwrap_or_default();
    let sanitized = sanitize_filename::sanitize(last_component).replace(' ', "_");
    if sanitized.is_empty() {
        FALLBACK_FILE_NAME.to_string()
    } else {
        sanitized
    }
}

pub fn link_domain(url: &str) -> String {
    let without_scheme = url.trim().split_once("://").map_or(url.trim(), |(_, rest)| rest);
    let authority = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    let host = match host_port.strip_prefix('[') {
        Some(ipv6) => ipv6.split(']').next().unwrap_or_default(),
        None => host_port.split(':').next().unwrap_or_default(),
    };
    host.strip_prefix("www.").unwrap_or(host).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_detection() {
        assert_eq!(kind_for_text("https://github.com/x"), EntryKind::Link);
        assert_eq!(kind_for_text("  http://a.b/c?d=1  "), EntryKind::Link);
        assert_eq!(kind_for_text("see https://a.b"), EntryKind::Text);
        assert_eq!(kind_for_text("ftp://a.b"), EntryKind::Text);
    }

    #[test]
    fn file_kinds() {
        assert_eq!(kind_for_file("Screenshot.PNG"), EntryKind::Image);
        assert_eq!(kind_for_file("clip.webm"), EntryKind::Video);
        assert_eq!(kind_for_file("backup.tar.gz"), EntryKind::File);
    }

    #[test]
    fn file_names_are_made_safe() {
        assert_eq!(safe_file_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_file_name("my notes.md"), "my_notes.md");
        assert_eq!(safe_file_name("Grüße.txt"), "Grüße.txt");
        assert_eq!(safe_file_name(".."), "file");
    }

    #[test]
    fn domains() {
        assert_eq!(link_domain("https://www.youtube.com/watch?v=1"), "youtube.com");
        assert_eq!(link_domain("http://nas.lan:8080/x"), "nas.lan");
    }

    #[test]
    fn expiry() {
        assert_eq!(Expiry::parse("1d"), Some(Expiry::Day));
        assert_eq!(Expiry::parse("24hour"), None);
        assert_eq!(Expiry::Never.deadline(100), None);
        assert_eq!(Expiry::Hour.deadline(100), Some(3700));
        assert_eq!(Expiry::Week.deadline(0), Some(604_800));
    }
}
