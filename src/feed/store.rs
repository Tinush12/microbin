use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension, Row, ToSql};

use super::entry::{
    kind_for_file, kind_for_text, safe_file_name, Entry, EntryKind, Expiry, StoredFile,
};

const DB_FILE: &str = "feed.sqlite";
const FILES_DIR: &str = "feed_files";
const TEMP_DIR: &str = "tmp";
const DEFAULT_EXPIRY_KEY: &str = "default_expiry";

const ENTRY_COLUMNS: &str = "id, kind, text, file_name, file_size, created, expires";
const LIVE: &str = "(expires IS NULL OR expires > :now)";

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS entries (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        kind TEXT NOT NULL,
        text TEXT,
        file_name TEXT,
        file_size INTEGER,
        created INTEGER NOT NULL,
        expires INTEGER NULL
    );
    CREATE INDEX IF NOT EXISTS entries_expires ON entries(expires);
    CREATE TABLE IF NOT EXISTS changes (
        seq INTEGER PRIMARY KEY AUTOINCREMENT,
        entry_id INTEGER NOT NULL
    );
    CREATE TABLE IF NOT EXISTS settings (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
";

#[derive(Debug)]
pub enum StoreError {
    Db(rusqlite::Error),
    Io(io::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Db(e) => write!(f, "database error: {e}"),
            StoreError::Io(e) => write!(f, "file error: {e}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Db(e)
    }
}

impl From<io::Error> for StoreError {
    fn from(e: io::Error) -> Self {
        StoreError::Io(e)
    }
}

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PageQuery {
    Latest,
    Before(i64),
    After(i64),
    Around(i64),
}

#[derive(Debug)]
pub struct Page {
    pub entries: Vec<Entry>,
    pub has_before: bool,
    pub has_after: bool,
}

#[derive(Debug)]
pub struct Changes {
    pub seq: i64,
    pub upserted: Vec<Entry>,
    pub removed: Vec<i64>,
}

pub struct Store {
    conn: Connection,
    files_dir: PathBuf,
}

fn entry_from_row(row: &Row) -> rusqlite::Result<Entry> {
    let kind_text: String = row.get(1)?;
    let kind = EntryKind::from_str(&kind_text).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Text,
            format!("unknown entry kind: {kind_text}").into(),
        )
    })?;
    let name: Option<String> = row.get(3)?;
    let size: Option<u64> = row.get(4)?;
    Ok(Entry {
        id: row.get(0)?,
        kind,
        text: row.get(2)?,
        file: name.zip(size).map(|(name, size)| StoredFile { name, size }),
        created: row.get(5)?,
        expires: row.get(6)?,
    })
}

fn log_change(conn: &Connection, entry_id: i64) -> rusqlite::Result<()> {
    conn.execute("INSERT INTO changes (entry_id) VALUES (?1)", params![entry_id])
        .map(|_| ())
}

fn remove_dir_if_present(dir: &Path) -> io::Result<()> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Where uploads wait until they are complete. Not an integer, so it never collides with an entry.
pub fn temp_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(FILES_DIR).join(TEMP_DIR)
}

/// Escapes `%`, `_` and `\` so user input only ever matches literally.
fn like_pattern(query: &str) -> String {
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

impl Store {
    pub fn open(data_dir: &Path) -> StoreResult<Store> {
        let files_dir = data_dir.join(FILES_DIR);
        fs::create_dir_all(&files_dir)?;
        let conn = Connection::open(data_dir.join(DB_FILE))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn, files_dir })
    }

    pub fn insert_text(&mut self, text: &str, now: i64) -> StoreResult<Entry> {
        let kind = kind_for_text(text);
        let stored = if kind == EntryKind::Link { text.trim() } else { text };
        let expires = self.default_expiry()?.deadline(now);
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO entries (kind, text, created, expires) VALUES (?1, ?2, ?3, ?4)",
            params![kind.as_str(), stored, now, expires],
        )?;
        let id = tx.last_insert_rowid();
        log_change(&tx, id)?;
        tx.commit()?;
        Ok(Entry {
            id,
            kind,
            text: Some(stored.to_string()),
            file: None,
            created: now,
            expires,
        })
    }

    pub fn insert_file(&mut self, name: &str, temp: &Path, size: u64, now: i64) -> StoreResult<Entry> {
        let safe_name = safe_file_name(name);
        let kind = kind_for_file(&safe_name);
        let expires = self.default_expiry()?.deadline(now);
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO entries (kind, file_name, file_size, created, expires)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![kind.as_str(), safe_name, size, now, expires],
        )?;
        let id = tx.last_insert_rowid();
        let dir = self.files_dir.join(id.to_string());
        // The transaction rolls back when dropped on any early return; the directory does not.
        let placed = fs::create_dir_all(&dir)
            .and_then(|_| fs::rename(temp, dir.join(&safe_name)))
            .map_err(StoreError::from)
            .and_then(|_| log_change(&tx, id).map_err(StoreError::from))
            .and_then(|_| tx.commit().map_err(StoreError::from));
        if let Err(e) = placed {
            if let Err(cleanup) = remove_dir_if_present(&dir) {
                log::error!("could not remove {}: {cleanup}", dir.display());
            }
            return Err(e);
        }
        Ok(Entry {
            id,
            kind,
            text: None,
            file: Some(StoredFile { name: safe_name, size }),
            created: now,
            expires,
        })
    }

    pub fn get(&self, id: i64, now: i64) -> StoreResult<Option<Entry>> {
        let sql = format!("SELECT {ENTRY_COLUMNS} FROM entries WHERE id = :id AND {LIVE}");
        let entry = self
            .conn
            .query_row(&sql, &[(":id", &id as &dyn ToSql), (":now", &now)], entry_from_row)
            .optional()?;
        Ok(entry)
    }

    pub fn file_path(&self, entry: &Entry) -> Option<PathBuf> {
        let file = entry.file.as_ref()?;
        Some(self.files_dir.join(entry.id.to_string()).join(&file.name))
    }

    fn older_than(&self, id: i64, limit: usize, now: i64) -> StoreResult<Vec<Entry>> {
        let sql = format!(
            "SELECT * FROM (SELECT {ENTRY_COLUMNS} FROM entries WHERE id < :id AND {LIVE}
             ORDER BY id DESC LIMIT :limit) ORDER BY id"
        );
        self.fetch(&sql, id, limit, now)
    }

    fn newer_than(&self, id: i64, limit: usize, now: i64) -> StoreResult<Vec<Entry>> {
        let sql = format!(
            "SELECT {ENTRY_COLUMNS} FROM entries WHERE id > :id AND {LIVE}
             ORDER BY id LIMIT :limit"
        );
        self.fetch(&sql, id, limit, now)
    }

    fn fetch(&self, sql: &str, id: i64, limit: usize, now: i64) -> StoreResult<Vec<Entry>> {
        let limit = limit as i64;
        let mut stmt = self.conn.prepare_cached(sql)?;
        let rows = stmt.query_map(
            &[(":id", &id as &dyn ToSql), (":limit", &limit), (":now", &now)],
            entry_from_row,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn exists(&self, comparison: &str, id: i64, now: i64) -> StoreResult<bool> {
        let sql = format!("SELECT EXISTS(SELECT 1 FROM entries WHERE id {comparison} :id AND {LIVE})");
        let found = self.conn.query_row(
            &sql,
            &[(":id", &id as &dyn ToSql), (":now", &now)],
            |row| row.get(0),
        )?;
        Ok(found)
    }

    pub fn page(&self, q: PageQuery, limit: usize, now: i64) -> StoreResult<Page> {
        let (entries, anchor) = match q {
            PageQuery::Latest => (self.older_than(i64::MAX, limit, now)?, i64::MAX),
            PageQuery::Before(id) => (self.older_than(id, limit, now)?, id),
            PageQuery::After(id) => (self.newer_than(id, limit, now)?, id),
            PageQuery::Around(id) => (self.around(id, limit, now)?, id),
        };
        let first = entries.first().map_or(anchor, |e| e.id);
        let last = entries.last().map_or(anchor, |e| e.id);
        Ok(Page {
            has_before: self.exists("<", first, now)?,
            has_after: self.exists(">", last, now)?,
            entries,
        })
    }

    fn around(&self, id: i64, limit: usize, now: i64) -> StoreResult<Vec<Entry>> {
        let side = limit / 2;
        let before = self.older_than(id, side, now)?;
        let target = self.get(id, now)?;
        let after = self.newer_than(id, side, now)?;
        Ok(before.into_iter().chain(target).chain(after).collect())
    }

    pub fn delete(&mut self, id: i64) -> StoreResult<bool> {
        let tx = self.conn.transaction()?;
        let removed = tx.execute("DELETE FROM entries WHERE id = ?1", params![id])?;
        if removed == 0 {
            return Ok(false);
        }
        log_change(&tx, id)?;
        tx.commit()?;
        remove_dir_if_present(&self.files_dir.join(id.to_string()))?;
        Ok(true)
    }

    pub fn set_expiry(&mut self, id: i64, expiry: Expiry, now: i64) -> StoreResult<Option<Entry>> {
        let sql = format!("UPDATE entries SET expires = :expires WHERE id = :id AND {LIVE}");
        let tx = self.conn.transaction()?;
        let updated = tx.execute(
            &sql,
            &[
                (":expires", &expiry.deadline(now) as &dyn ToSql),
                (":id", &id),
                (":now", &now),
            ],
        )?;
        if updated == 0 {
            return Ok(None);
        }
        log_change(&tx, id)?;
        tx.commit()?;
        self.get(id, now)
    }

    pub fn sweep_expired(&mut self, now: i64) -> StoreResult<Vec<i64>> {
        let ids = {
            let mut stmt = self
                .conn
                .prepare_cached("SELECT id FROM entries WHERE expires <= ?1 ORDER BY id")?;
            let rows = stmt.query_map(params![now], |row| row.get::<_, i64>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for id in &ids {
            self.delete(*id)?;
        }
        Ok(ids)
    }

    pub fn changes_since(&self, seq: i64, now: i64) -> StoreResult<Changes> {
        let latest: Option<i64> =
            self.conn
                .query_row("SELECT MAX(seq) FROM changes WHERE seq > ?1", params![seq], |row| {
                    row.get(0)
                })?;
        let ids = {
            let mut stmt = self.conn.prepare_cached(
                "SELECT DISTINCT entry_id FROM changes WHERE seq > ?1 ORDER BY entry_id",
            )?;
            let rows = stmt.query_map(params![seq], |row| row.get::<_, i64>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let states = ids
            .into_iter()
            .map(|id| Ok((id, self.get(id, now)?)))
            .collect::<StoreResult<Vec<_>>>()?;
        let (present, absent): (Vec<_>, Vec<_>) =
            states.into_iter().partition(|(_, entry)| entry.is_some());
        Ok(Changes {
            seq: latest.unwrap_or(seq),
            upserted: present.into_iter().filter_map(|(_, entry)| entry).collect(),
            removed: absent.into_iter().map(|(id, _)| id).collect(),
        })
    }

    pub fn current_seq(&self) -> StoreResult<i64> {
        let seq = self
            .conn
            .query_row("SELECT COALESCE(MAX(seq), 0) FROM changes", [], |row| row.get(0))?;
        Ok(seq)
    }

    pub fn count(&self, now: i64) -> StoreResult<i64> {
        let sql = format!("SELECT COUNT(*) FROM entries WHERE {LIVE}");
        let count = self
            .conn
            .query_row(&sql, &[(":now", &now as &dyn ToSql)], |row| row.get(0))?;
        Ok(count)
    }

    pub fn storage_used(&self, now: i64) -> StoreResult<u64> {
        let sql = format!("SELECT COALESCE(SUM(file_size), 0) FROM entries WHERE {LIVE}");
        let used = self
            .conn
            .query_row(&sql, &[(":now", &now as &dyn ToSql)], |row| row.get(0))?;
        Ok(used)
    }

    pub fn search(&self, query: &str, limit: usize, now: i64) -> StoreResult<Vec<Entry>> {
        let sql = format!(
            "SELECT {ENTRY_COLUMNS} FROM entries
             WHERE (text LIKE :q ESCAPE '\\' OR file_name LIKE :q ESCAPE '\\') AND {LIVE}
             ORDER BY id DESC LIMIT :limit"
        );
        let pattern = like_pattern(query);
        let limit = limit as i64;
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(
            &[(":q", &pattern as &dyn ToSql), (":limit", &limit), (":now", &now)],
            entry_from_row,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn default_expiry(&self) -> StoreResult<Expiry> {
        let stored: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![DEFAULT_EXPIRY_KEY],
                |row| row.get(0),
            )
            .optional()?;
        Ok(stored.as_deref().and_then(Expiry::parse).unwrap_or(Expiry::Never))
    }

    pub fn set_default_expiry(&mut self, e: Expiry) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![DEFAULT_EXPIRY_KEY, e.as_str()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn ids_are_unique_and_never_reused() {
        let (_d, mut s) = store();
        let a = s.insert_text("a", 1).unwrap();
        s.delete(a.id).unwrap();
        let b = s.insert_text("b", 2).unwrap();
        assert!(b.id > a.id);
    }

    #[test]
    fn file_insert_moves_temp_into_place() {
        let (d, mut s) = store();
        let tmp = d.path().join("upload.tmp");
        std::fs::write(&tmp, b"png").unwrap();
        let e = s.insert_file("../evil name.png", &tmp, 3, 1).unwrap();
        assert_eq!(e.kind, EntryKind::Image);
        assert_eq!(e.file.as_ref().unwrap().name, "evil_name.png");
        let p = s.file_path(&e).unwrap();
        assert!(p.starts_with(d.path().join("feed_files")) && p.exists() && !tmp.exists());
    }

    #[test]
    fn failed_file_insert_leaves_nothing_behind() {
        let (d, mut s) = store();
        let missing = d.path().join("never-written.tmp");
        assert!(s.insert_file("a.png", &missing, 3, 1).is_err());
        assert_eq!(s.count(2).unwrap(), 0);
        assert_eq!(s.current_seq().unwrap(), 0);
        let leftovers = std::fs::read_dir(d.path().join("feed_files")).unwrap().count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn delete_removes_the_file_from_disk() {
        let (d, mut s) = store();
        let tmp = d.path().join("t");
        std::fs::write(&tmp, b"x").unwrap();
        let e = s.insert_file("a.bin", &tmp, 1, 1).unwrap();
        let p = s.file_path(&e).unwrap();
        assert!(s.delete(e.id).unwrap());
        assert!(!p.exists() && !p.parent().unwrap().exists());
        assert!(!s.delete(e.id).unwrap());
    }

    #[test]
    fn link_text_is_stored_without_surrounding_whitespace() {
        let (_d, mut s) = store();
        let e = s.insert_text("  http://a.b/c  \n", 1).unwrap();
        assert_eq!(e.kind, EntryKind::Link);
        assert_eq!(e.text.as_deref(), Some("http://a.b/c"));
        let t = s.insert_text("  keep me\n", 1).unwrap();
        assert_eq!(t.text.as_deref(), Some("  keep me\n"));
    }

    #[test]
    fn paging_with_long_history() {
        let (_d, mut s) = store();
        for i in 0..120 {
            s.insert_text(&i.to_string(), i).unwrap();
        }
        let latest = s.page(PageQuery::Latest, 50, 999).unwrap();
        assert_eq!(latest.entries.len(), 50);
        assert_eq!(latest.entries.last().unwrap().text.as_deref(), Some("119"));
        assert!(latest.has_before && !latest.has_after);
        let older = s.page(PageQuery::Before(latest.entries[0].id), 50, 999).unwrap();
        assert_eq!(older.entries.last().unwrap().id, latest.entries[0].id - 1);
        let mid = s.page(PageQuery::Around(60), 50, 999).unwrap();
        assert!(mid.entries.iter().any(|e| e.id == 60) && mid.has_before && mid.has_after);
        let newer = s.page(PageQuery::After(mid.entries.last().unwrap().id), 50, 999).unwrap();
        assert_eq!(newer.entries[0].id, mid.entries.last().unwrap().id + 1);
    }

    #[test]
    fn changes_catch_up_without_ghosts() {
        let (_d, mut s) = store();
        let seq0 = s.current_seq().unwrap();
        let keep = s.insert_text("keep", 1).unwrap();
        let gone = s.insert_text("gone", 2).unwrap();
        s.delete(gone.id).unwrap();
        let c = s.changes_since(seq0, 3).unwrap();
        assert_eq!(c.upserted.iter().map(|e| e.id).collect::<Vec<_>>(), vec![keep.id]);
        assert_eq!(c.removed, vec![gone.id]);
        assert!(s.changes_since(c.seq, 3).unwrap().upserted.is_empty());
    }

    #[test]
    fn expiry_and_sweep() {
        let (_d, mut s) = store();
        let e = s.insert_text("x", 0).unwrap();
        assert_eq!(e.expires, None);
        s.set_expiry(e.id, Expiry::Hour, 0).unwrap();
        assert!(s.get(e.id, 3599).unwrap().is_some());
        assert!(s.get(e.id, 3600).unwrap().is_none());
        assert_eq!(s.sweep_expired(3600).unwrap(), vec![e.id]);
        s.set_default_expiry(Expiry::Day).unwrap();
        assert_eq!(s.insert_text("y", 10).unwrap().expires, Some(86_410));
    }

    #[test]
    fn search_text_and_file_names() {
        let (d, mut s) = store();
        s.insert_text("Jellyfin notes", 1).unwrap();
        s.insert_text("https://example.com/100%_real", 2).unwrap();
        let tmp = d.path().join("t");
        std::fs::write(&tmp, b"x").unwrap();
        s.insert_file("NAS-backup.tar.gz", &tmp, 1, 3).unwrap();
        assert_eq!(s.search("jellyfin", 50, 9).unwrap().len(), 1);
        assert_eq!(s.search("nas-BACKUP", 50, 9).unwrap().len(), 1);
        assert_eq!(s.search("100%_", 50, 9).unwrap().len(), 1);
        assert_eq!(s.search("%", 50, 9).unwrap().len(), 1);
    }

    #[test]
    fn legacy_files_untouched() {
        let (d, _s) = store();
        assert!(!d.path().join("database.sqlite").exists());
        assert!(d.path().join("feed.sqlite").exists());
    }
}
