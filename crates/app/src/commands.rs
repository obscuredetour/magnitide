use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::Instant;

use calibre_compat::{Book, Library, Query, SortKey, locate, sort_books};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::{AppState, Loaded};

type CmdResult<T> = Result<T, String>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryInfo {
    path: String,
    name: String,
    book_count: usize,
    load_ms: u128,
    warnings: Vec<String>,
}

/// Open `path`, or calibre's current/default library when `path` is None.
#[tauri::command]
pub async fn open_library(app: AppHandle, state: State<'_, AppState>, path: Option<String>) -> CmdResult<LibraryInfo> {
    let path = match path {
        Some(p) => PathBuf::from(p),
        None => locate::default_library().ok_or("No calibre library found. Choose a library folder to open.")?,
    };
    let cache_root = app.path().app_cache_dir().map_err(|e| e.to_string())?;

    let (loaded, info) = tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<_> {
        let t = Instant::now();
        let lib = Library::open(&path)?;
        let snap = lib.load()?;
        let root = lib.root().to_path_buf();
        let cache_key = snap.library_uuid.clone().unwrap_or_else(|| {
            let mut h = DefaultHasher::new();
            root.hash(&mut h);
            format!("path-{:016x}", h.finish())
        });
        let info = LibraryInfo {
            path: root.display().to_string(),
            name: root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            book_count: snap.books.len(),
            load_ms: t.elapsed().as_millis(),
            warnings: snap.warnings.clone(),
        };
        Ok((Loaded { root, snap, thumb_dir: cache_root.join("thumbs").join(cache_key) }, info))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("{e:#}"))?;

    state.replace(loaded);
    Ok(info)
}

/// One tile in the grid; deliberately small so 50k of them serialize quickly.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GridItem {
    id: i64,
    title: String,
    authors: String,
    series: Option<String>,
    /// Formatted for display, e.g. "3" or "2.5".
    series_index: Option<String>,
    has_cover: bool,
}

#[tauri::command]
pub async fn list_books(state: State<'_, AppState>, query: String, sort: SortKey) -> CmdResult<Vec<GridItem>> {
    let lib = state.current().ok_or("No library is open")?;
    let query = query.trim();
    let query = (!query.is_empty()).then(|| Query::parse(query)).transpose().map_err(|e| e.to_string())?;
    let mut books: Vec<&Book> = lib.snap.books.iter().filter(|b| query.as_ref().is_none_or(|q| q.matches(b))).collect();
    sort_books(&mut books, sort);
    Ok(books
        .into_iter()
        .map(|b| GridItem {
            id: b.id,
            title: b.title.clone(),
            authors: b.authors.join(" & "),
            series: b.series.clone(),
            series_index: b.series.as_ref().map(|_| fmt_index(b.series_index)),
            has_cover: b.has_cover,
        })
        .collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookDetails {
    id: i64,
    title: String,
    authors: Vec<String>,
    series: Option<String>,
    stars: Option<f32>,
    tags: Vec<String>,
    publisher: Option<String>,
    published: Option<String>,
    added: Option<String>,
    languages: Vec<String>,
    identifiers: Vec<(String, String)>,
    formats: Vec<FormatInfo>,
    /// Sanitized: scripts, styles, event handlers and images removed.
    comments_html: Option<String>,
    has_cover: bool,
    folder: String,
}

#[derive(Serialize)]
pub struct FormatInfo {
    format: String,
    size: u64,
    present: bool,
}

#[tauri::command]
pub async fn book_details(state: State<'_, AppState>, id: i64) -> CmdResult<BookDetails> {
    let lib = state.current().ok_or("No library is open")?;
    let b = lib.snap.get(id).ok_or_else(|| format!("No book with id {id}"))?;
    Ok(BookDetails {
        id: b.id,
        title: b.title.clone(),
        authors: b.authors.clone(),
        series: b.series.as_ref().map(|s| format!("Book {} of {s}", fmt_index(b.series_index))),
        stars: b.stars(),
        tags: b.tags.clone(),
        publisher: b.publisher.clone(),
        published: b.pubdate.map(|d| d.format("%-d %B %Y").to_string()),
        added: b.timestamp.map(|d| d.format("%-d %B %Y").to_string()),
        languages: b.languages.clone(),
        identifiers: b.identifiers.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        formats: b
            .formats
            .iter()
            .map(|f| FormatInfo {
                format: f.format.clone(),
                size: f.size,
                present: b.format_path(&lib.root, f).is_file(),
            })
            .collect(),
        comments_html: b.comments.as_deref().map(sanitize),
        has_cover: b.has_cover,
        folder: b.dir(&lib.root).display().to_string(),
    })
}

/// Comments are HTML from arbitrary sources (metadata downloads, publishers).
/// Anything that could run script or load remote content is stripped.
fn sanitize(html: &str) -> String {
    ammonia::Builder::default().rm_tags(["img"]).clean(html).to_string()
}

fn fmt_index(i: f64) -> String {
    if i.fract() == 0.0 { format!("{}", i as i64) } else { format!("{i}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_comments() {
        let dirty = r#"<p onclick="x()">Hi<script>alert(1)</script><img src="http://evil/x.png"><a href="javascript:x()">l</a></p>"#;
        let clean = sanitize(dirty);
        assert!(
            !clean.contains("script")
                && !clean.contains("onclick")
                && !clean.contains("img")
                && !clean.contains("javascript")
        );
        assert!(clean.contains("Hi"));
    }

    #[test]
    fn formats_series_index() {
        assert_eq!(fmt_index(3.0), "3");
        assert_eq!(fmt_index(2.5), "2.5");
    }
}
