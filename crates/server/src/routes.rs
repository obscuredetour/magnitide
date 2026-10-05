//! Routes. Every page exists twice: as HTML (`/…`, for browsers) and as OPDS
//! (`/opds/…`, for reading apps), both drawn from the same [`Listing`].

use std::collections::BTreeMap;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use magnitide_catalog::{Book, Query, SortDir, SortKey, sort_books};

use crate::util::{self, PAGE_SIZE};
use crate::{Body, Reply, Request, Served, Source, html, opds};

/// What a page lists.
pub(crate) struct Listing<'a> {
    pub title: String,
    /// The page's own address without `page=` ("/shelf?name=To%20Read").
    pub link: String,
    pub page: usize,
    pub more: bool,
    pub items: Items<'a>,
}

pub(crate) enum Items<'a> {
    Books(Vec<&'a Book>),
    /// Named groups (shelves, authors, series): name, book count, address.
    Groups(Vec<(String, usize, String)>),
}

/// Handle one request.
pub fn handle(req: &Request, source: &dyn Source, cache_dir: &Path) -> Reply {
    let Some(lib) = source.current() else {
        return Reply::new(503, "text/plain; charset=utf-8", "No library is open in Magnitide right now.");
    };
    let lib = lib.as_ref();
    let (opds, path) = match req.path.strip_prefix("/opds") {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => (true, rest),
        _ => (false, req.path.as_str()),
    };
    let path = path.trim_end_matches('/');
    let page: usize = req.param("page").and_then(|p| p.parse().ok()).unwrap_or(1).max(1);

    if let Some(rest) = path.strip_prefix("/download/") {
        return download(lib, rest, cache_dir);
    }
    if let Some(id) = path.strip_prefix("/cover/") {
        return cover(lib, id, None);
    }
    if let Some(id) = path.strip_prefix("/thumb/") {
        return cover(lib, id, Some(cache_dir));
    }
    if let Some(id) = path.strip_prefix("/book/") {
        return match id.parse().ok().and_then(|id| lib.catalog().get(id)) {
            Some(book) if !opds => html::book_page(lib, book, req),
            _ => not_found(opds),
        };
    }
    if path.is_empty() {
        return if opds { opds::root(lib) } else { html::home(lib, req) };
    }
    if opds && path == "/search.xml" {
        return opds::open_search(lib);
    }

    let listing = match path {
        "/books" => Some(books(lib, "All Books", "/books".into(), page, |_| true, SortKey::Title, SortDir::Asc)),
        "/recent" => {
            Some(books(lib, "Recently Added", "/recent".into(), page, |_| true, SortKey::Added, SortDir::Desc))
        }
        "/search" => {
            let q = req.param("q").unwrap_or("").trim().to_owned();
            match Query::parse(&q) {
                Ok(query) => Some(books(
                    lib,
                    &format!("Search: {q}"),
                    format!("/search?q={}", util::percent_encode(&q)),
                    page,
                    |b| query.matches(b),
                    SortKey::Title,
                    SortDir::Asc,
                )),
                Err(e) => return Reply::new(400, "text/plain; charset=utf-8", format!("That search didn't work: {e}")),
            }
        }
        "/shelves" => Some(groups(lib, "Shelves", "/shelves", "/shelf", page, |b| b.shelves.clone())),
        "/authors" => Some(groups(lib, "Authors", "/authors", "/author", page, |b| b.authors.clone())),
        "/series" if req.param("name").is_none() => Some(groups(lib, "Series", "/series", "/series", page, |b| {
            b.series.iter().cloned().chain(b.more_series.iter().map(|s| s.name.clone())).collect()
        })),
        "/shelf" | "/author" | "/series" => req.param("name").map(|name| {
            let link = format!("{path}?name={}", util::percent_encode(name));
            match path {
                "/shelf" => {
                    books(lib, name, link, page, |b| b.shelves.iter().any(|s| s == name), SortKey::Title, SortDir::Asc)
                }
                "/author" => {
                    books(lib, name, link, page, |b| b.authors.iter().any(|a| a == name), SortKey::Series, SortDir::Asc)
                }
                _ => books(
                    lib,
                    name,
                    link,
                    page,
                    |b| b.series.as_deref() == Some(name) || b.more_series.iter().any(|s| s.name == name),
                    SortKey::Series,
                    SortDir::Asc,
                ),
            }
        }),
        _ => None,
    };
    match listing {
        Some(l) if opds => opds::feed(lib, &l, req),
        Some(l) => html::listing(lib, &l, req),
        None => not_found(opds),
    }
}

fn not_found(opds: bool) -> Reply {
    if opds { Reply::new(404, "text/plain; charset=utf-8", "Not found.") } else { html::not_found() }
}

fn books<'a>(
    lib: &'a dyn Served,
    title: &str,
    link: String,
    page: usize,
    keep: impl Fn(&Book) -> bool,
    key: SortKey,
    dir: SortDir,
) -> Listing<'a> {
    let mut list: Vec<&Book> = lib.catalog().books.iter().filter(|b| keep(b)).collect();
    sort_books(&mut list, key, dir);
    let (items, more) = util::page(&list, page, PAGE_SIZE);
    Listing { title: title.to_owned(), link, page, more, items: Items::Books(items) }
}

fn groups<'a>(
    lib: &'a dyn Served,
    title: &str,
    link: &str,
    each: &str,
    page: usize,
    names: impl Fn(&Book) -> Vec<String>,
) -> Listing<'a> {
    // Case-insensitive order, counting books per name.
    let mut counts: BTreeMap<(String, String), usize> = BTreeMap::new();
    for b in &lib.catalog().books {
        for n in names(b) {
            *counts.entry((n.to_lowercase(), n)).or_default() += 1;
        }
    }
    let all: Vec<(String, usize, String)> = counts
        .into_iter()
        .map(|((_, name), n)| {
            let href = format!("{each}?name={}", util::percent_encode(&name));
            (name, n, href)
        })
        .collect();
    let (items, more) = util::page(&all, page, PAGE_SIZE * 2);
    Listing { title: title.to_owned(), link: link.to_owned(), page, more, items: Items::Groups(items) }
}

/// `/download/<id>/<FORMAT>`. A KEPUB the book doesn't have is made from its
/// EPUB (for Kobos) and kept in the cache.
fn download(lib: &dyn Served, rest: &str, cache_dir: &Path) -> Reply {
    let Some((id, format)) = rest.split_once('/') else { return html::not_found() };
    let format = format.to_uppercase();
    let Some(book) = id.parse().ok().and_then(|id| lib.catalog().get(id)) else { return html::not_found() };
    let file = match book.formats.iter().find(|f| f.format == format) {
        Some(f) => book.format_path(lib.root(), f),
        None if format == "KEPUB" => match book.formats.iter().find(|f| f.format == "EPUB") {
            Some(epub) => match kepub_for(lib, book, &book.format_path(lib.root(), epub), cache_dir) {
                Ok(p) => p,
                Err(e) => return Reply::new(500, "text/plain; charset=utf-8", format!("Couldn't make a KEPUB: {e}")),
            },
            None => return html::not_found(),
        },
        None => return html::not_found(),
    };
    if !file.is_file() {
        return Reply::new(404, "text/plain; charset=utf-8", "That file is missing from the library.");
    }
    Reply { status: 200, content_type: util::mime(&format).to_owned(), headers: Vec::new(), body: Body::File(file) }
        .header("Content-Disposition", util::attachment(&util::download_name(book, &format)))
}

/// A cache folder for this library (ids are only unique within one).
fn library_cache(lib: &dyn Served, cache_dir: &Path, kind: &str) -> PathBuf {
    let mut h = DefaultHasher::new();
    lib.root().hash(&mut h);
    cache_dir.join(kind).join(format!("{:016x}", h.finish()))
}

fn kepub_for(lib: &dyn Served, book: &Book, epub: &Path, cache_dir: &Path) -> Result<PathBuf, String> {
    let meta = fs::metadata(epub).map_err(|e| e.to_string())?;
    let stamp =
        meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    let dir = library_cache(lib, cache_dir, "kepub");
    let dest = dir.join(format!("{}-{stamp}-{}.kepub.epub", book.id, meta.len()));
    if dest.is_file() {
        return Ok(dest);
    }
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Older conversions of this book go.
    if let Ok(entries) = fs::read_dir(&dir) {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().starts_with(&format!("{}-", book.id)) {
                let _ = fs::remove_file(e.path());
            }
        }
    }
    let tmp = dir.join(format!(".{}-{}.tmp", book.id, std::process::id()));
    let _ = fs::remove_file(&tmp);
    magnitide_core::convert::kepub::convert(epub, &tmp).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    Ok(dest)
}

/// `/cover/<id>` (the cover as stored) or `/thumb/<id>` (small, cached).
fn cover(lib: &dyn Served, id: &str, thumbs: Option<&Path>) -> Reply {
    let Some(path) = id.parse().ok().and_then(|id| lib.catalog().get(id)).and_then(|b| b.cover_path(lib.root())) else {
        return Reply::new(404, "text/plain; charset=utf-8", "No cover.");
    };
    if !path.is_file() {
        return Reply::new(404, "text/plain; charset=utf-8", "No cover.");
    }
    let file = match thumbs {
        None => path,
        Some(cache) => match thumbnail(lib, id, &path, cache) {
            Some(t) => t,
            None => path,
        },
    };
    Reply { status: 200, content_type: "image/jpeg".into(), headers: Vec::new(), body: Body::File(file) }
        .header("Cache-Control", "max-age=3600")
}

const THUMB: (u32, u32) = (200, 300);

fn thumbnail(lib: &dyn Served, id: &str, cover: &Path, cache_dir: &Path) -> Option<PathBuf> {
    let stamp = fs::metadata(cover).ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    let dir = library_cache(lib, cache_dir, "thumbs");
    let dest = dir.join(format!("{id}-{stamp}.jpg"));
    if dest.is_file() {
        return Some(dest);
    }
    let img = image::open(cover).ok()?.thumbnail(THUMB.0, THUMB.1).to_rgb8();
    fs::create_dir_all(&dir).ok()?;
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 82).encode_image(&img).ok()?;
    let tmp = dir.join(format!(".{id}-{}.tmp", std::process::id()));
    fs::write(&tmp, out).ok()?;
    fs::rename(&tmp, &dest).ok()?;
    Some(dest)
}
