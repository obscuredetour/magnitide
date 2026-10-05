//! The OPDS 1.2 catalogue (Atom), for reading apps such as KOReader:
//! navigation feeds for the lists of lists, acquisition feeds for books.

use chrono::{SecondsFormat, Utc};
use magnitide_catalog::Book;

use crate::routes::{Items, Listing};
use crate::util::{self, escape};
use crate::{Reply, Request, Served};

const NAVIGATION: &str = "application/atom+xml;profile=opds-catalog;kind=navigation";
const ACQUISITION: &str = "application/atom+xml;profile=opds-catalog;kind=acquisition";

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// A feed: `kind` is NAVIGATION or ACQUISITION; `link` is the feed's own
/// address (under /opds); `extra` holds paging links; `entries` is XML.
fn document(lib: &dyn Served, id: &str, title: &str, kind: &str, link: &str, extra: &str, entries: &str) -> Reply {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <feed xmlns=\"http://www.w3.org/2005/Atom\" xmlns:dc=\"http://purl.org/dc/terms/\" xmlns:opds=\"http://opds-spec.org/2010/catalog\">\
         <id>urn:magnitide:{id}</id><title>{title}</title><updated>{now}</updated>\
         <author><name>{name}</name></author>\
         <link rel=\"self\" href=\"{link}\" type=\"{kind}\"/>\
         <link rel=\"start\" href=\"/opds\" type=\"{NAVIGATION}\"/>\
         <link rel=\"up\" href=\"/opds\" type=\"{NAVIGATION}\"/>\
         <link rel=\"search\" href=\"/opds/search.xml\" type=\"application/opensearchdescription+xml\"/>\
         <link rel=\"search\" href=\"/opds/search?q={{searchTerms}}\" type=\"{ACQUISITION}\"/>\
         {extra}{entries}</feed>",
        id = escape(id),
        title = escape(title),
        now = now(),
        name = escape(lib.name()),
        link = escape(link),
    );
    Reply::new(200, &format!("{kind};charset=utf-8"), xml)
}

fn nav_entry(id: &str, title: &str, href: &str, kind: &str, content: &str) -> String {
    format!(
        "<entry><title>{}</title><id>urn:magnitide:{}</id><updated>{}</updated>\
         <link rel=\"subsection\" href=\"{}\" type=\"{kind}\"/><content type=\"text\">{}</content></entry>",
        escape(title),
        escape(id),
        now(),
        escape(href),
        escape(content)
    )
}

pub fn root(lib: &dyn Served) -> Reply {
    let count = lib.catalog().books.len();
    let entries = [
        ("recent", "Recently Added", "/opds/recent", ACQUISITION, "The newest books first".to_owned()),
        ("books", "All Books", "/opds/books", ACQUISITION, format!("{count} books by title")),
        ("shelves", "Shelves", "/opds/shelves", NAVIGATION, "Books by shelf".to_owned()),
        ("authors", "Authors", "/opds/authors", NAVIGATION, "Books by author".to_owned()),
        ("series", "Series", "/opds/series", NAVIGATION, "Books by series".to_owned()),
    ]
    .iter()
    .map(|(id, title, href, kind, content)| nav_entry(id, title, href, kind, content))
    .collect::<String>();
    document(lib, "root", lib.name(), NAVIGATION, "/opds", "", &entries)
}

pub fn open_search(lib: &dyn Served) -> Reply {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <OpenSearchDescription xmlns=\"http://a9.com/-/spec/opensearch/1.1/\">\
         <ShortName>{name}</ShortName><Description>Search {name}</Description>\
         <InputEncoding>UTF-8</InputEncoding><OutputEncoding>UTF-8</OutputEncoding>\
         <Url type=\"{ACQUISITION}\" template=\"/opds/search?q={{searchTerms}}\"/></OpenSearchDescription>",
        name = escape(lib.name())
    );
    Reply::new(200, "application/opensearchdescription+xml; charset=utf-8", xml)
}

fn book_entry(b: &Book) -> String {
    let mut e = format!("<entry><title>{}</title>", escape(&b.title));
    let id = b.uuid.as_deref().map_or_else(|| format!("urn:magnitide:book:{}", b.id), |u| format!("urn:uuid:{u}"));
    e.push_str(&format!("<id>{}</id>", escape(&id)));
    let updated = b.last_modified.or(b.timestamp).unwrap_or_else(Utc::now);
    e.push_str(&format!("<updated>{}</updated>", updated.to_rfc3339_opts(SecondsFormat::Secs, true)));
    for a in &b.authors {
        e.push_str(&format!("<author><name>{}</name></author>", escape(a)));
    }
    if let Some(p) = &b.publisher {
        e.push_str(&format!("<dc:publisher>{}</dc:publisher>", escape(p)));
    }
    if let Some(d) = b.pubdate {
        e.push_str(&format!("<dc:issued>{}</dc:issued>", d.format("%Y-%m-%d")));
    }
    for l in &b.languages {
        e.push_str(&format!("<dc:language>{}</dc:language>", escape(l)));
    }
    if let Some(isbn) = b.identifiers.get("isbn") {
        e.push_str(&format!("<dc:identifier>urn:isbn:{}</dc:identifier>", escape(isbn)));
    }
    for t in &b.tags {
        e.push_str(&format!("<category term=\"{0}\" label=\"{0}\"/>", escape(t)));
    }
    // Series in the summary, since OPDS 1.2 has no element for it.
    let mut summary = String::new();
    if let Some(s) = &b.series {
        summary.push_str(&format!("{s} #{} — ", magnitide_catalog::format_number(b.series_index, b.series_end)));
    }
    let description = b.comments.as_deref().map(magnitide_core::html::sanitize).unwrap_or_default();
    summary.push_str(&util::text_of(&description));
    let summary = summary.trim_end_matches(" — ").trim();
    if !summary.is_empty() {
        e.push_str(&format!("<summary type=\"text\">{}</summary>", escape(summary)));
    }
    if !description.trim().is_empty() {
        e.push_str(&format!("<content type=\"html\">{}</content>", escape(&description)));
    }
    if b.has_cover {
        e.push_str(&format!(
            "<link rel=\"http://opds-spec.org/image\" href=\"/cover/{0}\" type=\"image/jpeg\"/>\
             <link rel=\"http://opds-spec.org/image/thumbnail\" href=\"/thumb/{0}\" type=\"image/jpeg\"/>",
            b.id
        ));
    }
    for f in &b.formats {
        e.push_str(&format!(
            "<link rel=\"http://opds-spec.org/acquisition\" href=\"/download/{}/{}\" type=\"{}\" title=\"{}\" length=\"{}\"/>",
            b.id,
            escape(&f.format),
            util::mime(&f.format),
            escape(&f.format),
            f.size
        ));
    }
    e.push_str("</entry>");
    e
}

pub fn feed(lib: &dyn Served, l: &Listing, _req: &Request) -> Reply {
    let link = format!("/opds{}", l.link);
    let sep = if link.contains('?') { '&' } else { '?' };
    let (kind, entries) = match &l.items {
        Items::Books(books) => (ACQUISITION, books.iter().map(|b| book_entry(b)).collect::<String>()),
        Items::Groups(groups) => (
            NAVIGATION,
            groups
                .iter()
                .map(|(name, count, href)| {
                    let noun = if *count == 1 { "book" } else { "books" };
                    nav_entry(
                        &format!("{}:{name}", l.link),
                        name,
                        &format!("/opds{href}"),
                        ACQUISITION,
                        &format!("{count} {noun}"),
                    )
                })
                .collect(),
        ),
    };
    let mut extra = String::new();
    if l.page > 1 {
        extra.push_str(&format!("<link rel=\"first\" href=\"{}\" type=\"{kind}\"/>", escape(&link)));
        extra.push_str(&format!(
            "<link rel=\"previous\" href=\"{}\" type=\"{kind}\"/>",
            escape(&format!("{link}{sep}page={}", l.page - 1))
        ));
    }
    if l.more {
        extra.push_str(&format!(
            "<link rel=\"next\" href=\"{}\" type=\"{kind}\"/>",
            escape(&format!("{link}{sep}page={}", l.page + 1))
        ));
    }
    let self_link = if l.page > 1 { format!("{link}{sep}page={}", l.page) } else { link.clone() };
    document(lib, &l.link, &l.title, kind, &self_link, &extra, &entries)
}
