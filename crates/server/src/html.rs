//! The web page, for e-reader browsers (Kobo's, Kindle's, a phone's): plain
//! server-rendered HTML with no scripts, high contrast and big tap targets.

use magnitide_catalog::Book;

use crate::routes::{Items, Listing};
use crate::util::{escape, percent_encode};
use crate::{Reply, Request, Served};

const STYLE: &str = "\
body{font-family:Georgia,serif;font-size:1.15em;line-height:1.4;margin:0 auto;max-width:48em;padding:0.6em 0.9em;background:#fff;color:#000}\
a{color:#000}\
h1{font-size:1.4em;margin:0.4em 0}\
h2{font-size:1.15em;margin:1em 0 0.3em}\
nav{border-bottom:2px solid #000;padding-bottom:0.4em;margin-bottom:0.4em}\
nav a{display:inline-block;padding:0.35em 0.5em 0.35em 0;margin-right:0.6em}\
form{display:flex;margin:0.5em 0}\
input[type=search]{flex:1;margin-right:0.4em;font-size:1em;padding:0.4em;border:2px solid #000;border-radius:0}\
button,.button{font-size:1em;padding:0.4em 0.9em;border:2px solid #000;background:#fff;color:#000;text-decoration:none;display:inline-block;border-radius:0}\
ul{list-style:none;margin:0;padding:0}\
li{border-bottom:1px solid #000;padding:0.55em 0;display:flex;align-items:flex-start}\
li img,.nocover{width:3.4em;flex:none;border:1px solid #000;margin-right:0.7em}.nocover{height:5.1em;box-sizing:border-box}\
.book{flex:1;min-width:0}\
.title{font-weight:bold;display:block}\
.meta{font-size:0.9em}\
.get{flex:none;align-self:center;margin-left:0.7em}\
.pages{display:flex;justify-content:space-between;margin:0.9em 0}\
.cover{float:right;width:38%;max-width:14em;margin:0 0 0.8em 0.9em;border:1px solid #000}\
.formats{margin:0.8em 0}.formats a{margin:0 0.5em 0.5em 0}\
dl{margin:0.5em 0}dt{font-weight:bold;float:left;clear:left;width:6.5em}dd{margin:0 0 0.2em 6.5em}\
.muted{font-style:italic}\
";

fn page(lib: &dyn Served, title: &str, body: &str) -> Reply {
    let html = format!(
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{title} · {name}</title><style>{STYLE}</style></head><body>\
         <nav><a href=\"/\"><b>{name}</b></a><a href=\"/recent\">Recent</a><a href=\"/books\">All</a>\
         <a href=\"/shelves\">Shelves</a><a href=\"/authors\">Authors</a><a href=\"/series\">Series</a></nav>{body}</body></html>",
        title = escape(title),
        name = escape(lib.name()),
    );
    Reply::new(200, "text/html; charset=utf-8", html)
}

fn search_form(value: &str) -> String {
    format!(
        "<form action=\"/search\" method=\"get\"><input type=\"search\" name=\"q\" value=\"{}\" placeholder=\"Title, author, series…\" aria-label=\"Search\"><button type=\"submit\">Search</button></form>",
        escape(value)
    )
}

pub fn home(lib: &dyn Served, _req: &Request) -> Reply {
    let books = &lib.catalog().books;
    let shelves: std::collections::BTreeSet<&str> =
        books.iter().flat_map(|b| b.shelves.iter().map(String::as_str)).collect();
    let mut body = format!("<h1>{}</h1>{}<ul>", escape(lib.name()), search_form(""));
    let rows = [
        ("/recent", "Recently added", String::new()),
        ("/books", "All books", format!("{}", books.len())),
        ("/shelves", "Shelves", format!("{}", shelves.len())),
        ("/authors", "Authors", String::new()),
        ("/series", "Series", String::new()),
    ];
    for (href, label, count) in rows {
        body.push_str(&format!(
            "<li><a class=\"book\" href=\"{href}\"><span class=\"title\">{label}</span></a><span class=\"meta\">{count}</span></li>"
        ));
    }
    body.push_str("</ul>");
    page(lib, "Library", &body)
}

/// The format to offer first: KEPUB for Kobo's browser (made from the EPUB if
/// need be), else EPUB, else whatever there is.
pub fn preferred(book: &Book, kobo: bool) -> Option<String> {
    let has = |f: &str| book.formats.iter().any(|x| x.format == f);
    if kobo && (has("KEPUB") || has("EPUB")) {
        return Some("KEPUB".into());
    }
    if has("EPUB") {
        return Some("EPUB".into());
    }
    book.formats.first().map(|f| f.format.clone())
}

fn byline(book: &Book) -> String {
    let mut parts = Vec::new();
    if !book.authors.is_empty() {
        parts.push(escape(&book.authors.join(", ")));
    }
    if let Some(series) = &book.series {
        parts.push(format!(
            "{} #{}",
            escape(series),
            magnitide_catalog::format_number(book.series_index, book.series_end)
        ));
    }
    parts.join(" · ")
}

fn label(format: &str) -> &str {
    if format == "KEPUB" { "KEPUB (Kobo)" } else { format }
}

pub fn listing(lib: &dyn Served, l: &Listing, req: &Request) -> Reply {
    let mut body = format!("<h1>{}</h1>", escape(&l.title));
    if l.link.starts_with("/search") {
        body.push_str(&search_form(req.param("q").unwrap_or("")));
    }
    body.push_str("<ul>");
    match &l.items {
        Items::Books(books) if books.is_empty() => body.push_str("<li class=\"muted\">No books here.</li>"),
        Items::Groups(groups) if groups.is_empty() => body.push_str("<li class=\"muted\">Nothing here yet.</li>"),
        Items::Books(books) => {
            for b in books {
                let thumb = if b.has_cover {
                    format!("<img src=\"/thumb/{}\" alt=\"\">", b.id)
                } else {
                    "<span class=\"nocover\"></span>".to_owned()
                };
                let get = preferred(b, req.from_kobo())
                    .map(|f| format!("<a class=\"button get\" href=\"/download/{}/{f}\">{}</a>", b.id, label(&f)))
                    .unwrap_or_default();
                body.push_str(&format!(
                    "<li>{thumb}<a class=\"book\" href=\"/book/{id}\"><span class=\"title\">{title}</span><span class=\"meta\">{by}</span></a>{get}</li>",
                    id = b.id,
                    title = escape(&b.title),
                    by = byline(b),
                ));
            }
        }
        Items::Groups(groups) => {
            for (name, count, href) in groups {
                body.push_str(&format!(
                    "<li><a class=\"book\" href=\"{}\"><span class=\"title\">{}</span></a><span class=\"meta\">{count}</span></li>",
                    escape(href),
                    escape(name)
                ));
            }
        }
    }
    body.push_str("</ul>");
    if l.page > 1 || l.more {
        let sep = if l.link.contains('?') { '&' } else { '?' };
        let prev = if l.page > 1 {
            format!("<a class=\"button\" href=\"{}{sep}page={}\">‹ Previous</a>", escape(&l.link), l.page - 1)
        } else {
            "<span></span>".into()
        };
        let next = if l.more {
            format!("<a class=\"button\" href=\"{}{sep}page={}\">Next ›</a>", escape(&l.link), l.page + 1)
        } else {
            "<span></span>".into()
        };
        body.push_str(&format!("<div class=\"pages\">{prev}<span>Page {}</span>{next}</div>", l.page));
    }
    page(lib, &l.title, &body)
}

pub fn book_page(lib: &dyn Served, b: &Book, req: &Request) -> Reply {
    let mut body = String::new();
    if b.has_cover {
        body.push_str(&format!("<img class=\"cover\" src=\"/thumb/{}\" alt=\"\">", b.id));
    }
    body.push_str(&format!("<h1>{}</h1>", escape(&b.title)));
    let link =
        |base: &str, name: &str| format!("<a href=\"{base}?name={}\">{}</a>", percent_encode(name), escape(name));

    body.push_str("<dl>");
    if !b.authors.is_empty() {
        let authors: Vec<String> = b.authors.iter().map(|a| link("/author", a)).collect();
        body.push_str(&format!("<dt>By</dt><dd>{}</dd>", authors.join(", ")));
    }
    if let Some(series) = &b.series {
        body.push_str(&format!(
            "<dt>Series</dt><dd>{} #{}</dd>",
            link("/series", series),
            magnitide_catalog::format_number(b.series_index, b.series_end)
        ));
    }
    if let Some(p) = &b.publisher {
        body.push_str(&format!("<dt>Publisher</dt><dd>{}</dd>", escape(p)));
    }
    if let Some(d) = b.pubdate {
        body.push_str(&format!("<dt>Published</dt><dd>{}</dd>", d.format("%Y")));
    }
    if !b.shelves.is_empty() {
        let shelves: Vec<String> = b.shelves.iter().map(|s| link("/shelf", s)).collect();
        body.push_str(&format!("<dt>Shelves</dt><dd>{}</dd>", shelves.join(", ")));
    }
    body.push_str("</dl>");

    // Every format the book has, plus a KEPUB made from its EPUB.
    let mut formats: Vec<String> = b.formats.iter().map(|f| f.format.clone()).collect();
    if formats.iter().any(|f| f == "EPUB") && !formats.iter().any(|f| f == "KEPUB") {
        formats.push("KEPUB".into());
    }
    let first = preferred(b, req.from_kobo());
    formats.sort_by_key(|f| Some(f) != first.as_ref());
    if formats.is_empty() {
        body.push_str("<p class=\"muted\">This book has no files.</p>");
    } else {
        body.push_str("<div class=\"formats\">");
        for f in &formats {
            body.push_str(&format!("<a class=\"button\" href=\"/download/{}/{f}\">Download {}</a>", b.id, label(f)));
        }
        body.push_str("</div>");
    }

    match b.comments.as_deref().map(magnitide_core::html::sanitize).filter(|c| !c.trim().is_empty()) {
        Some(c) => body.push_str(&format!("<h2>Description</h2><div>{c}</div>")),
        None => body.push_str("<p class=\"muted\">No description available.</p>"),
    }
    page(lib, &b.title, &body)
}

pub fn not_found() -> Reply {
    Reply::new(
        404,
        "text/html; charset=utf-8",
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Not found</title></head><body style=\"font-family:Georgia,serif\"><h1>Not found</h1><p><a href=\"/\">Back to the library</a></p></body></html>",
    )
}
