//! KOReader's progress sync ("kosync"), the protocol of koreader-sync-server,
//! so KOReader can use the library server as its custom sync server
//! (Tools › Progress sync). Positions are kept in their own small database,
//! never in a library, keyed by user name and KOReader's document
//! fingerprint (see `magnitide_core::fingerprint`).
//!
//! Signing in: KOReader sends a user name and the MD5 of a password. With a
//! server password set, any user name works with that password; without one
//! the server is open, like the rest of it. User names only keep people's
//! positions apart.

use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use md5::{Digest, Md5};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::Reply;

/// Longest values accepted (KOReader's are far shorter).
const MAX_DOCUMENT: usize = 128;
const MAX_TEXT: usize = 4096;
/// Request bodies read at most this much.
pub const MAX_BODY: u64 = 64 * 1024;

/// A kosync request: what the routes need from HTTP.
pub struct SyncRequest<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub user: Option<&'a str>,
    pub key: Option<&'a str>,
    pub body: &'a [u8],
}

/// Whether `path` (slashes collapsed) is one of kosync's.
pub fn is_sync_path(path: &str) -> bool {
    let path = collapse(path);
    matches!(path.as_str(), "/healthcheck" | "/users/create" | "/users/auth") || path.starts_with("/syncs/progress")
}

/// KOReader joins its server URL and the path, so "http://host:8180/" gives "//users/auth".
fn collapse(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        if !(c == '/' && out.ends_with('/')) {
            out.push(c);
        }
    }
    if out.len() > 1 { out.trim_end_matches('/').to_owned() } else { out }
}

/// A reading position, as KOReader sends and receives it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Position {
    pub document: String,
    /// An XPointer in a reflowable book, a page number in a fixed-layout one.
    pub progress: String,
    /// 0.0 to 1.0.
    pub percentage: f64,
    pub device: String,
    pub device_id: String,
    /// Seconds since the epoch, set by the server.
    pub timestamp: i64,
}

/// A stored position with what KOReader said about the book (if it sends
/// metadata), for showing in the app.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub username: String,
    pub position: Position,
    pub title: Option<String>,
    pub authors: Option<String>,
    pub filename: Option<String>,
}

/// The positions database.
pub struct SyncStore {
    conn: Mutex<Connection>,
}

impl SyncStore {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        Self::with(Connection::open(path).map_err(|e| e.to_string())?)
    }

    pub fn in_memory() -> Result<Self, String> {
        Self::with(Connection::open_in_memory().map_err(|e| e.to_string())?)
    }

    fn with(conn: Connection) -> Result<Self, String> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS progress (
                username TEXT NOT NULL,
                document TEXT NOT NULL,
                progress TEXT NOT NULL,
                percentage REAL NOT NULL,
                device TEXT NOT NULL,
                device_id TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                title TEXT,
                authors TEXT,
                filename TEXT,
                PRIMARY KEY (username, document)
            ) WITHOUT ROWID;
            PRAGMA user_version = 1;",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn put(&self, username: &str, p: &Position, meta: &Metadata) -> rusqlite::Result<()> {
        self.conn().execute(
            "INSERT INTO progress (username, document, progress, percentage, device, device_id, timestamp, title, authors, filename)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT (username, document) DO UPDATE SET
                progress = excluded.progress, percentage = excluded.percentage, device = excluded.device,
                device_id = excluded.device_id, timestamp = excluded.timestamp,
                title = coalesce(excluded.title, title), authors = coalesce(excluded.authors, authors),
                filename = coalesce(excluded.filename, filename)",
            params![
                username,
                p.document,
                p.progress,
                p.percentage,
                p.device,
                p.device_id,
                p.timestamp,
                meta.title,
                meta.authors,
                meta.filename
            ],
        )?;
        Ok(())
    }

    fn get(&self, username: &str, document: &str) -> rusqlite::Result<Option<Position>> {
        self.conn()
            .query_row(
                "SELECT document, progress, percentage, device, device_id, timestamp FROM progress
                 WHERE username = ?1 AND document = ?2",
                params![username, document],
                |r| {
                    Ok(Position {
                        document: r.get(0)?,
                        progress: r.get(1)?,
                        percentage: r.get(2)?,
                        device: r.get(3)?,
                        device_id: r.get(4)?,
                        timestamp: r.get(5)?,
                    })
                },
            )
            .optional()
    }

    /// Every stored position, newest first.
    pub fn entries(&self) -> rusqlite::Result<Vec<Entry>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT username, document, progress, percentage, device, device_id, timestamp, title, authors, filename
             FROM progress ORDER BY timestamp DESC",
        )?;
        stmt.query_map([], |r| {
            Ok(Entry {
                username: r.get(0)?,
                position: Position {
                    document: r.get(1)?,
                    progress: r.get(2)?,
                    percentage: r.get(3)?,
                    device: r.get(4)?,
                    device_id: r.get(5)?,
                    timestamp: r.get(6)?,
                },
                title: r.get(7)?,
                authors: r.get(8)?,
                filename: r.get(9)?,
            })
        })?
        .collect()
    }
}

/// KOReader's optional `metadata` (Settings › "Send metadata"): what it knows about the book.
#[derive(Debug, Default, Deserialize)]
struct Metadata {
    title: Option<String>,
    authors: Option<String>,
    filename: Option<String>,
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn md5_hex(s: &str) -> String {
    Md5::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether a user name and key may sign in: the key is MD5 of the server password, if there is one.
fn accepted(password: Option<&str>, user: Option<&str>, key: Option<&str>) -> bool {
    let (Some(_), Some(key)) = (user.filter(|u| !u.trim().is_empty()), key.filter(|k| !k.is_empty())) else {
        return false;
    };
    match password.filter(|p| !p.is_empty()) {
        None => true,
        Some(p) => {
            let want = md5_hex(p);
            let key = key.to_ascii_lowercase();
            // Compare without stopping at the first difference.
            key.len() == want.len() && key.bytes().zip(want.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
        }
    }
}

fn reply(status: u16, body: Value) -> Reply {
    Reply::new(status, "application/json", body.to_string())
}

/// koreader-sync-server's error codes, which KOReader shows by message.
fn error(status: u16, code: u16, message: &str) -> Reply {
    reply(status, json!({ "code": code, "message": message }))
}

fn unauthorized(password: Option<&str>) -> Reply {
    let message = if password.is_some_and(|p| !p.is_empty()) {
        "Wrong password. Use the password set in Magnitide (Settings › Library Server), with any user name."
    } else {
        "Enter a user name to sign in."
    };
    error(401, 2001, message)
}

fn text(v: &Value, field: &str, max: usize) -> Option<String> {
    match v.get(field)? {
        Value::String(s) if !s.is_empty() && s.len() <= max => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Handle a kosync request. `store` is None while sync is off.
pub fn handle(req: &SyncRequest, store: Option<&SyncStore>, password: Option<&str>) -> Reply {
    let path = collapse(req.path);
    let Some(store) = store else {
        return error(404, 1000, "Progress sync isn't turned on in Magnitide.");
    };
    match (req.method, path.as_str()) {
        ("GET" | "HEAD", "/healthcheck") => reply(200, json!({ "state": "OK" })),
        ("POST", "/users/create") => {
            let Ok(body) = serde_json::from_slice::<Value>(req.body) else {
                return error(403, 2003, "Invalid request");
            };
            let user = text(&body, "username", MAX_TEXT);
            let key = text(&body, "password", MAX_TEXT);
            if accepted(password, user.as_deref(), key.as_deref()) {
                // There are no accounts to create: the password is the server's.
                reply(201, json!({ "username": user }))
            } else {
                unauthorized(password)
            }
        }
        ("GET" | "HEAD", "/users/auth") => {
            if accepted(password, req.user, req.key) {
                reply(200, json!({ "authorized": "OK" }))
            } else {
                unauthorized(password)
            }
        }
        ("PUT", "/syncs/progress") => {
            if !accepted(password, req.user, req.key) {
                return unauthorized(password);
            }
            let Ok(body) = serde_json::from_slice::<Value>(req.body) else {
                return error(403, 2003, "Invalid request");
            };
            let Some(document) = text(&body, "document", MAX_DOCUMENT) else {
                return error(403, 2004, "Field 'document' not provided.");
            };
            let percentage = match body.get("percentage") {
                Some(Value::Number(n)) => n.as_f64(),
                Some(Value::String(s)) => s.parse().ok(),
                _ => None,
            };
            let (Some(progress), Some(percentage)) = (text(&body, "progress", MAX_TEXT), percentage) else {
                return error(403, 2003, "Invalid request");
            };
            let position = Position {
                document,
                progress,
                percentage: percentage.clamp(0.0, 1.0),
                device: text(&body, "device", MAX_TEXT).unwrap_or_default(),
                device_id: text(&body, "device_id", MAX_TEXT).unwrap_or_default(),
                timestamp: now(),
            };
            let meta: Metadata =
                body.get("metadata").and_then(|m| serde_json::from_value(m.clone()).ok()).unwrap_or_default();
            match store.put(req.user.unwrap_or_default(), &position, &meta) {
                Ok(()) => reply(200, json!({ "document": position.document, "timestamp": position.timestamp })),
                Err(e) => error(500, 1001, &format!("Couldn't save the position: {e}")),
            }
        }
        ("GET" | "HEAD", p) if p.starts_with("/syncs/progress/") => {
            if !accepted(password, req.user, req.key) {
                return unauthorized(password);
            }
            let document = &p["/syncs/progress/".len()..];
            match store.get(req.user.unwrap_or_default(), document) {
                Ok(Some(position)) => reply(200, serde_json::to_value(position).unwrap_or_default()),
                // KOReader reads an empty object as "no progress yet".
                Ok(None) => reply(200, json!({})),
                Err(e) => error(500, 1001, &format!("Couldn't read the position: {e}")),
            }
        }
        _ => error(405, 1000, "Not a progress sync request."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req<'a>(method: &'a str, path: &'a str, auth: Option<(&'a str, &'a str)>, body: &'a [u8]) -> SyncRequest<'a> {
        SyncRequest { method, path, user: auth.map(|a| a.0), key: auth.map(|a| a.1), body }
    }

    fn json_of(r: &Reply) -> Value {
        serde_json::from_str(&r.text()).unwrap()
    }

    #[test]
    fn paths() {
        assert!(is_sync_path("/users/auth") && is_sync_path("//users/auth") && is_sync_path("/syncs/progress/abc"));
        assert!(is_sync_path("/healthcheck/") && !is_sync_path("/books") && !is_sync_path("/opds"));
        assert_eq!(collapse("//syncs//progress/abc/"), "/syncs/progress/abc");
    }

    #[test]
    fn signing_in() {
        let store = SyncStore::in_memory().unwrap();
        let key = md5_hex("hunter2");
        // With a password: any user name, that password's MD5.
        let pw = Some("hunter2");
        assert_eq!(handle(&req("GET", "/users/auth", Some(("jamie", &key)), b""), Some(&store), pw).status, 200);
        let upper = key.to_uppercase();
        assert_eq!(handle(&req("GET", "/users/auth", Some(("kid", &upper)), b""), Some(&store), pw).status, 200);
        let wrong = handle(&req("GET", "/users/auth", Some(("jamie", &md5_hex("nope"))), b""), Some(&store), pw);
        assert_eq!(wrong.status, 401);
        assert!(json_of(&wrong)["message"].as_str().unwrap().contains("Settings › Library Server"));
        assert_eq!(handle(&req("GET", "/users/auth", None, b""), Some(&store), pw).status, 401);
        assert_eq!(handle(&req("GET", "/users/auth", Some(("", &key)), b""), Some(&store), pw).status, 401);
        // Registering is signing in.
        let body = format!(r#"{{"username":"jamie","password":"{key}"}}"#);
        let created = handle(&req("POST", "/users/create", None, body.as_bytes()), Some(&store), pw);
        assert_eq!((created.status, json_of(&created)["username"].as_str()), (201, Some("jamie")));
        let bad = format!(r#"{{"username":"jamie","password":"{}"}}"#, md5_hex("x"));
        assert_eq!(handle(&req("POST", "/users/create", None, bad.as_bytes()), Some(&store), pw).status, 401);
        // Without one, any user name and key.
        assert_eq!(
            handle(&req("GET", "/users/auth", Some(("jamie", "anything")), b""), Some(&store), None).status,
            200
        );
        assert_eq!(handle(&req("GET", "/healthcheck", None, b""), Some(&store), pw).text(), r#"{"state":"OK"}"#);
    }

    #[test]
    fn positions() {
        let store = SyncStore::in_memory().unwrap();
        let auth = Some(("jamie", "k"));
        let none = handle(&req("GET", "/syncs/progress/abc123", auth, b""), Some(&store), None);
        assert_eq!((none.status, none.text()), (200, "{}".to_owned()));

        let body = br#"{"document":"abc123","progress":"/body/DocFragment[12]/body/p[3]/text().42","percentage":0.4231,
            "device":"Kobo Libra 2","device_id":"DEV1","metadata":{"title":"Dune","authors":"Frank Herbert","filename":"Dune.epub"}}"#;
        let put = handle(&req("PUT", "/syncs/progress", auth, body), Some(&store), None);
        assert_eq!(put.status, 200);
        assert_eq!(json_of(&put)["document"], "abc123");
        assert!(json_of(&put)["timestamp"].as_i64().unwrap() > 1_700_000_000);

        let got = json_of(&handle(&req("GET", "/syncs/progress/abc123", auth, b""), Some(&store), None));
        assert_eq!(got["progress"], "/body/DocFragment[12]/body/p[3]/text().42");
        assert_eq!((got["percentage"].as_f64(), got["device"].as_str()), (Some(0.4231), Some("Kobo Libra 2")));
        assert_eq!(got["device_id"], "DEV1");

        // Someone else's positions are their own.
        let other = handle(&req("GET", "/syncs/progress/abc123", Some(("kid", "k")), b""), Some(&store), None);
        assert_eq!(other.text(), "{}");

        // A later push replaces it; metadata sent once is kept.
        let body2 = br#"{"document":"abc123","progress":"57","percentage":"0.5","device":"Mac","device_id":"DEV2"}"#;
        assert_eq!(handle(&req("PUT", "//syncs/progress", auth, body2), Some(&store), None).status, 200);
        let entries = store.entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!((entries[0].position.percentage, entries[0].title.as_deref()), (0.5, Some("Dune")));

        // Bad requests.
        let no_doc =
            handle(&req("PUT", "/syncs/progress", auth, br#"{"progress":"1","percentage":0.1}"#), Some(&store), None);
        assert_eq!((no_doc.status, json_of(&no_doc)["code"].as_u64()), (403, Some(2004)));
        assert_eq!(handle(&req("PUT", "/syncs/progress", auth, b"nope"), Some(&store), None).status, 403);
        assert_eq!(handle(&req("PUT", "/syncs/progress", None, body2), Some(&store), None).status, 401);
        assert_eq!(handle(&req("DELETE", "/syncs/progress", auth, b""), Some(&store), None).status, 405);
        // Sync off.
        assert_eq!(handle(&req("GET", "/healthcheck", None, b""), None, None).status, 404);
    }
}
