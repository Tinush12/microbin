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
