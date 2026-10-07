//! Book metadata lookup from online sources: Open Library and Google Books
//! (built in), and calibre metadata-source plugins (see magnitide-plugins),
//! whose records join the same ranking.
//!
//! Nothing here runs unless the user asks (a "Find Metadata" search, a cover
//! download). Requests carry only what was searched for, identify Magnitide
//! by name, and are throttled per host. Results are ranked by plain string
//! matching ([`magnitide_catalog::matching`]); no learned ranking.
//!
//! Results come back as raw records; descriptions are untrusted HTML and
//! must be sanitized before display.

use std::collections::BTreeMap;

use magnitide_catalog::matching::{self, MatchKind};
use serde::{Deserialize, Serialize};

pub mod google;
mod http;
pub mod openlibrary;
mod text;

pub use http::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// A metadata source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum SourceId {
    OpenLibrary,
    Google,
}

impl SourceId {
    pub const ALL: [SourceId; 2] = [SourceId::OpenLibrary, SourceId::Google];

    pub fn name(self) -> &'static str {
        match self {
            SourceId::OpenLibrary => "Open Library",
            SourceId::Google => "Google Books",
        }
    }

    /// Whether the source only works with the user's own API key.
    pub fn needs_key(self) -> bool {
        // Google Books no longer serves anonymous requests (daily quota 0).
        matches!(self, SourceId::Google)
    }
}

/// What to look for. Any field may be empty; an ISBN is tried first.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Lookup {
    pub title: String,
    pub authors: Vec<String>,
    pub isbn: Option<String>,
}

/// One source's description of a book.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct SourceRecord {
    /// A built-in source; None for a record from a calibre plugin.
    pub source: Option<SourceId>,
    /// What to call the source: "Open Library", or the plugin's name.
    #[serde(default)]
    pub source_name: String,
    /// The source's id for it (Open Library work key, Google volume id).
    pub source_key: String,
    /// The book's page on the source's website.
    pub link: Option<String>,
    pub title: String,
    pub subtitle: Option<String>,
    pub authors: Vec<String>,
    pub publisher: Option<String>,
    /// ISO date, "YYYY-MM-DD". Year-only dates are 1 January.
    pub published: Option<String>,
    /// ISO 639-2 codes ("eng").
    pub languages: Vec<String>,
    /// "isbn" (ISBN-13), plus the source's own id ("google", "openlibrary").
    pub identifiers: BTreeMap<String, String>,
    pub tags: Vec<String>,
    /// Untrusted HTML: sanitize before display or saving.
    pub description_html: Option<String>,
    pub cover_url: Option<String>,
    pub pages: Option<u32>,
    /// More (the description) can be fetched with [`details`].
    pub has_details: bool,
}

/// Records from one or more sources that describe the same book.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Match {
    /// In source order (Open Library, Google Books, then plugins).
    pub records: Vec<SourceRecord>,
    /// 0–1.
    pub score: f64,
    pub kind: MatchKind,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    /// Best first.
    pub matches: Vec<Match>,
    /// Sources that failed, as "Google Books: …"; the others still answer.
    pub errors: Vec<String>,
}

/// Which sources to ask, and their keys.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub disabled: Vec<SourceId>,
    pub google_key: Option<String>,
}

impl Settings {
    pub fn enabled(&self, id: SourceId) -> bool {
        !self.disabled.contains(&id) && (!id.needs_key() || self.key(id).is_some())
    }

    pub fn key(&self, id: SourceId) -> Option<&str> {
        match id {
            SourceId::Google => self.google_key.as_deref().filter(|k| !k.trim().is_empty()),
            SourceId::OpenLibrary => None,
        }
    }
}

/// Ask every enabled source at once and group what they say into matches.
pub fn search(lookup: &Lookup, settings: &Settings) -> SearchResult {
    let (records, errors) = search_records(lookup, settings);
    rank(lookup, records, errors)
}

/// Ask every enabled built-in source at once: their records, and the errors
/// of those that failed ("Google Books: …").
pub fn search_records(lookup: &Lookup, settings: &Settings) -> (Vec<SourceRecord>, Vec<String>) {
    let sources: Vec<SourceId> = SourceId::ALL.into_iter().filter(|s| settings.enabled(*s)).collect();
    let answers: Vec<(SourceId, Result<Vec<SourceRecord>>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = sources
            .iter()
            .map(|&s| {
                (
                    s,
                    scope.spawn(move || match s {
                        SourceId::OpenLibrary => openlibrary::search(lookup),
                        SourceId::Google => google::search(lookup, settings.key(s).unwrap_or_default()),
                    }),
                )
            })
            .collect();
        handles.into_iter().map(|(s, h)| (s, h.join().unwrap_or_else(|_| Err(Error::new("crashed"))))).collect()
    });

    let mut records = Vec::new();
    let mut errors = Vec::new();
    for (source, answer) in answers {
        match answer {
            Ok(r) => records.extend(r),
            Err(e) => errors.push(format!("{}: {e}", source.name())),
        }
    }
    (records, errors)
}

/// Group records describing the same book (from any sources, plugins
/// included) and rank the groups against the lookup.
pub fn rank(lookup: &Lookup, records: Vec<SourceRecord>, errors: Vec<String>) -> SearchResult {
    SearchResult { matches: group(lookup, records), errors }
}

/// Fill in what the first search didn't return (Open Library descriptions).
pub fn details(record: &SourceRecord) -> Result<SourceRecord> {
    match record.source {
        Some(SourceId::OpenLibrary) if record.has_details => openlibrary::details(record),
        _ => Ok(record.clone()),
    }
}

/// Is this a cover image URL one of our sources gave out? Only these are
/// ever downloaded.
pub fn is_cover_url(url: &str) -> bool {
    ["https://covers.openlibrary.org/b/", "https://books.google.com/books/content?"]
        .iter()
        .any(|prefix| url.starts_with(prefix))
        && !url.contains(['\\', '@', '#'])
}

/// Download a cover image (at most 20 MB) from a source's cover URL.
pub fn download_cover(url: &str) -> Result<Vec<u8>> {
    if !is_cover_url(url) {
        return Err(Error::new("not a cover address from a metadata source"));
    }
    http::get_bytes(url, 20 << 20)
}

/// Group records describing the same book (same ISBN, or same title and
/// first author) and rank the groups against the lookup.
fn group(lookup: &Lookup, records: Vec<SourceRecord>) -> Vec<Match> {
    let key = |r: &SourceRecord| {
        (matching::normalize_title(&r.title), r.authors.first().map(|a| matching::surname(a)).unwrap_or_default())
    };
    let mut groups: Vec<Vec<SourceRecord>> = Vec::new();
    for r in records {
        let isbn = r.identifiers.get("isbn").cloned();
        let k = key(&r);
        let same = groups.iter_mut().find(|g| {
            g.iter().any(|o| {
                (isbn.is_some() && o.identifiers.get("isbn") == isbn.as_ref())
                    // One record per source and title: different editions
                    // from the same source stay separate.
                    || (o.source != r.source && key(o) == k)
            })
        });
        match same {
            Some(g) if !g.iter().any(|o| o.source == r.source) => g.push(r),
            _ => groups.push(vec![r]),
        }
    }

    let want_isbn = lookup.isbn.as_deref();
    let mut matches: Vec<Match> = groups
        .into_iter()
        .map(|mut records| {
            records.sort_by_key(|r| r.source);
            let (score, kind) = records
                .iter()
                .map(|r| {
                    let isbns: Vec<String> = r.identifiers.get("isbn").cloned().into_iter().collect();
                    matching::score(&lookup.title, &lookup.authors, want_isbn, &r.title, &r.authors, &isbns)
                })
                .max_by(|a, b| a.1.cmp(&b.1).then(a.0.total_cmp(&b.0)))
                .unwrap_or((0.0, MatchKind::Possible));
            Match { records, score, kind }
        })
        .collect();
    matches.sort_by(|a, b| {
        b.kind.cmp(&a.kind).then(b.score.total_cmp(&a.score)).then(b.records.len().cmp(&a.records.len()))
    });
    matches.truncate(20);
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(source: SourceId, title: &str, author: &str, isbn: Option<&str>) -> SourceRecord {
        SourceRecord {
            source: Some(source),
            title: title.into(),
            authors: vec![author.into()],
            identifiers: isbn.map(|i| ("isbn".to_owned(), i.to_owned())).into_iter().collect(),
            ..Default::default()
        }
    }

    #[test]
    fn groups_across_sources_and_ranks() {
        let lookup = Lookup { title: "The Dispossessed".into(), authors: vec!["Ursula K. Le Guin".into()], isbn: None };
        let matches = group(
            &lookup,
            vec![
                record(SourceId::Google, "Study Guide to The Dispossessed", "A Student", None),
                record(SourceId::OpenLibrary, "The Dispossessed", "Ursula K. Le Guin", Some("9780061054884")),
                record(
                    SourceId::Google,
                    "The Dispossessed: An Ambiguous Utopia",
                    "Ursula K. Le Guin",
                    Some("9780061054884"),
                ),
                record(SourceId::OpenLibrary, "The dispossessed", "Ursula K. Le Guin", Some("9781473228412")),
            ],
        );
        assert_eq!(matches.len(), 3, "{matches:#?}");
        assert_eq!(matches[0].kind, MatchKind::Strong);
        assert_eq!(
            matches[0].records.iter().map(|r| r.source.unwrap()).collect::<Vec<_>>(),
            [SourceId::OpenLibrary, SourceId::Google]
        );
        assert!(matches[2].records[0].title.starts_with("Study Guide"));
    }

    #[test]
    fn isbn_matches_rank_first() {
        let lookup = Lookup { title: "Wrong Title".into(), authors: vec![], isbn: Some("0-06-105488-7".into()) };
        let matches = group(
            &lookup,
            vec![
                record(SourceId::OpenLibrary, "Wrong Title", "X", None),
                record(SourceId::OpenLibrary, "The Dispossessed", "Ursula K. Le Guin", Some("9780061054884")),
            ],
        );
        assert_eq!(matches[0].kind, MatchKind::Isbn);
        assert_eq!(matches[0].records[0].title, "The Dispossessed");
    }

    #[test]
    fn only_source_cover_urls_are_downloadable() {
        assert!(is_cover_url("https://covers.openlibrary.org/b/id/6979680-L.jpg?default=false"));
        assert!(is_cover_url("https://books.google.com/books/content?id=x&printsec=frontcover&img=1"));
        assert!(!is_cover_url("http://covers.openlibrary.org/b/id/1-L.jpg"));
        assert!(!is_cover_url("https://covers.openlibrary.org.evil.example/b/id/1.jpg"));
        assert!(!is_cover_url("https://example.com/?https://covers.openlibrary.org/b/"));
    }

    #[test]
    fn google_needs_a_key() {
        let mut s = Settings::default();
        assert!(s.enabled(SourceId::OpenLibrary));
        assert!(!s.enabled(SourceId::Google));
        s.google_key = Some("k".into());
        assert!(s.enabled(SourceId::Google));
        s.disabled.push(SourceId::OpenLibrary);
        assert!(!s.enabled(SourceId::OpenLibrary));
    }
}
