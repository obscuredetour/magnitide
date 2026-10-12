//! Metron (metron.cloud), a community comic database: issues by series and
//! number, or by Metron id. It needs the user's own account (HTTP Basic) and
//! allows 20 requests a minute, so a search asks only for the list of
//! matching issues; an issue's credits, arcs and the rest come with
//! [`details`], when it's picked.

use serde_json::Value;

use crate::text::text_to_html;
use crate::{ComicRecord, Lookup, Result, SourceId, SourceRecord, http};

const API: &str = "https://metron.cloud/api";

/// Issues matching the lookup's series and issue number (the number may be
/// left out), or the one with its Metron id.
pub(crate) fn search(lookup: &Lookup, login: (&str, &str)) -> Result<Vec<SourceRecord>> {
    if let Some(id) = lookup.identifiers.get("metron").filter(|id| id.chars().all(|c| c.is_ascii_digit())) {
        return Ok(vec![issue(id, login)?]);
    }
    let Some(series) = lookup.series.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(Vec::new());
    };
    let mut query = vec![("series_name", series)];
    if let Some(number) = lookup.issue.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        query.push(("number", number));
    }
    let list = http::get_json_with(&format!("{API}/issue/"), &query, Some(login))?;
    Ok(parse_list(&list))
}

/// An issue's full record: its credits, story arcs, characters, …
pub(crate) fn details(record: &SourceRecord, login: (&str, &str)) -> Result<SourceRecord> {
    issue(&record.source_key, login)
}

fn issue(id: &str, login: (&str, &str)) -> Result<SourceRecord> {
    parse_issue(&http::get_json_with(&format!("{API}/issue/{id}/"), &[], Some(login))?)
        .ok_or_else(|| crate::Error::new("the answer wasn't readable"))
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

fn names(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| str_of(item, "name").map(str::to_owned))
        .collect()
}

/// "Saga #17": what an issue's called when it has no title of its own (as
/// Magnitide names comics read from their files).
fn issue_title(series: &str, number: Option<&str>) -> String {
    match number {
        Some(n) => format!("{series} #{n}"),
        None => series.to_owned(),
    }
}

/// A list item: enough to choose by (series, number, date, cover).
fn parse_list(v: &Value) -> Vec<SourceRecord> {
    v.get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(20)
        .filter_map(|item| {
            let id = item.get("id")?.as_i64()?.to_string();
            let series = item.get("series")?;
            let name = str_of(series, "name")?;
            let number = str_of(item, "number");
            Some(SourceRecord {
                source: Some(SourceId::Metron),
                source_name: SourceId::Metron.name().into(),
                link: Some(format!("https://metron.cloud/issue/{id}/")),
                title: issue_title(name, number),
                published: str_of(item, "cover_date").and_then(crate::text::iso_date),
                identifiers: [("metron".to_owned(), id.clone())].into(),
                cover_url: str_of(item, "image").map(str::to_owned),
                series: Some(name.to_owned()),
                series_index: number.and_then(leading_number),
                comic: Some(ComicRecord {
                    issue: number.map(str::to_owned),
                    volume: series.get("year_began").and_then(Value::as_i64),
                    ..Default::default()
                }),
                source_key: id,
                has_details: true,
                ..Default::default()
            })
        })
        .collect()
}

/// An issue's full record.
fn parse_issue(v: &Value) -> Option<SourceRecord> {
    let id = v.get("id")?.as_i64()?.to_string();
    let series = v.get("series")?;
    let name = str_of(series, "name")?;
    let number = str_of(v, "number");
    // Its own title: a collection's ("Volume One"), else its first story's ("Chapter Seventeen").
    let title = str_of(v, "title")
        .map(str::to_owned)
        .or_else(|| {
            v.get("name")?
                .as_array()?
                .iter()
                .find_map(|n| n.as_str().map(str::trim).filter(|n| !n.is_empty()).map(str::to_owned))
        })
        .unwrap_or_else(|| issue_title(name, number));

    let mut comic = ComicRecord {
        issue: number.map(str::to_owned),
        format: series.get("series_type").and_then(|t| str_of(t, "name")).map(str::to_owned),
        volume: series.get("year_began").and_then(Value::as_i64),
        story_arcs: names(v, "arcs"),
        characters: names(v, "characters"),
        teams: names(v, "teams"),
        age_rating: v
            .get("rating")
            .and_then(|r| str_of(r, "name"))
            .filter(|r| !r.eq_ignore_ascii_case("Unknown"))
            .map(str::to_owned),
        ..Default::default()
    };
    let mut writers = Vec::new();
    for credit in v.get("credits").and_then(Value::as_array).into_iter().flatten() {
        let Some(person) = str_of(credit, "creator") else { continue };
        for role in credit.get("role").and_then(Value::as_array).into_iter().flatten() {
            let Some(role) = str_of(role, "name") else { continue };
            let into = match role {
                "Writer" | "Script" | "Scripter" | "Story" | "Plot" => &mut writers,
                "Artist" | "Penciller" | "Illustrator" | "Breakdowns" | "Layouts" => &mut comic.artists,
                "Inker" | "Embellisher" | "Finishes" => &mut comic.inkers,
                "Colorist" | "Color Separations" => &mut comic.colorists,
                "Letterer" => &mut comic.letterers,
                "Cover" => &mut comic.cover_artists,
                r if r == "Editor" || r.ends_with(" Editor") || r.starts_with("Editor ") => &mut comic.editors,
                _ => continue,
            };
            if !into.iter().any(|p| p == person) {
                into.push(person.to_owned());
            }
        }
    }

    let mut identifiers = std::collections::BTreeMap::from([("metron".to_owned(), id.clone())]);
    if let Some(cv) = v.get("cv_id").and_then(Value::as_i64) {
        identifiers.insert("comicvine".into(), cv.to_string());
    }
    if let Some(isbn) =
        str_of(v, "isbn").map(|i| i.replace('-', "")).filter(|i| i.len() == 13 && i.chars().all(|c| c.is_ascii_digit()))
    {
        identifiers.insert("isbn".into(), isbn);
    }
    if let Some(upc) = str_of(v, "upc") {
        identifiers.insert("upc".into(), upc.to_owned());
    }

    Some(SourceRecord {
        source: Some(SourceId::Metron),
        source_name: SourceId::Metron.name().into(),
        link: str_of(v, "resource_url")
            .map(str::to_owned)
            .or_else(|| Some(format!("https://metron.cloud/issue/{id}/"))),
        title,
        authors: writers,
        publisher: v.get("publisher").and_then(|p| str_of(p, "name")).map(str::to_owned),
        published: str_of(v, "cover_date").and_then(crate::text::iso_date),
        languages: str_of(series, "language")
            .map(|l| vec![magnitide_catalog::languages::language_code(l)])
            .unwrap_or_default(),
        identifiers,
        tags: names(series, "genres"),
        description_html: str_of(v, "desc").and_then(text_to_html),
        cover_url: str_of(v, "image").map(str::to_owned),
        pages: v.get("page").and_then(Value::as_u64).and_then(|p| u32::try_from(p).ok()),
        series: Some(name.to_owned()),
        series_index: number.and_then(leading_number),
        comic: Some(comic),
        source_key: id,
        has_details: false,
        ..Default::default()
    })
}

/// "17" → 17, "1.5" → 1.5, "½" → 0.5; None for "Annual 1".
fn leading_number(n: &str) -> Option<f64> {
    if n == "½" {
        return Some(0.5);
    }
    let end = n.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(n.len());
    n[..end].trim_end_matches('.').parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn lists_issues_by_series_and_number() {
        let records = parse_list(&fixture("metron-issue-list.json"));
        assert_eq!(records.len(), 2);
        let saga = &records[0];
        assert_eq!((saga.title.as_str(), saga.source_key.as_str()), ("Saga #17", "4921"));
        assert_eq!((saga.series.as_deref(), saga.series_index), (Some("Saga"), Some(17.0)));
        assert_eq!(saga.published.as_deref(), Some("2014-02-01"));
        assert!(saga.has_details && saga.cover_url.as_deref().unwrap().starts_with("https://static.metron.cloud/"));
        assert_eq!(saga.comic.as_ref().unwrap().volume, Some(2012));
        assert_eq!(records[1].cover_url, None);
    }

    #[test]
    fn reads_an_issues_credits_and_the_rest() {
        let r = parse_issue(&fixture("metron-issue.json")).unwrap();
        assert_eq!(r.title, "Chapter Seventeen");
        assert_eq!(r.authors, ["Brian K. Vaughan"]);
        assert_eq!((r.publisher.as_deref(), r.languages.as_slice()), (Some("Image"), &["eng".to_owned()][..]));
        assert_eq!(r.tags, ["Science Fiction", "Fantasy"]);
        assert_eq!(r.identifiers.get("comicvine").map(String::as_str), Some("431234"));
        assert_eq!(r.identifiers.get("metron").map(String::as_str), Some("4921"));
        assert!(!r.identifiers.contains_key("isbn"), "an empty ISBN isn't one");
        assert_eq!(
            r.description_html.as_deref(),
            Some("<p>Prince Robot IV gets everyone into trouble.</p><p>Meanwhile, Alana and Marko make a choice.</p>")
        );
        let c = r.comic.unwrap();
        assert_eq!(c.issue.as_deref(), Some("17"));
        assert_eq!(c.format.as_deref(), Some("Ongoing Series"));
        assert_eq!(
            (c.artists, c.colorists, c.cover_artists),
            (vec!["Fiona Staples".into()], vec!["Fiona Staples".into()], vec!["Fiona Staples".into()])
        );
        assert_eq!((c.letterers, c.editors), (vec!["Fonografiks".into()], vec!["Eric Stephenson".into()]));
        assert_eq!(c.story_arcs, ["Chapter Three"]);
        assert_eq!(c.characters, ["Alana", "Marko", "Prince Robot IV"]);
        assert_eq!(c.age_rating.as_deref(), Some("Mature"));
        assert!(!r.has_details);
    }

    #[test]
    fn an_issue_with_no_title_is_called_by_its_number() {
        let mut v = fixture("metron-issue.json");
        v["name"] = serde_json::json!([]);
        assert_eq!(parse_issue(&v).unwrap().title, "Saga #17");
        v["title"] = serde_json::json!("Saga, Volume Three");
        assert_eq!(parse_issue(&v).unwrap().title, "Saga, Volume Three");
    }

    /// Against the real Metron, with your own account:
    /// `METRON_USERNAME=… METRON_PASSWORD=… cargo test -p magnitide-sources -- --ignored metron`
    #[test]
    #[ignore = "uses the network and a Metron account"]
    fn live_metron() {
        let (Ok(user), Ok(password)) = (std::env::var("METRON_USERNAME"), std::env::var("METRON_PASSWORD")) else {
            panic!("set METRON_USERNAME and METRON_PASSWORD");
        };
        let lookup =
            Lookup { comic: true, series: Some("Saga".into()), issue: Some("17".into()), ..Default::default() };
        let found = search(&lookup, (&user, &password)).unwrap();
        let saga = found.iter().find(|r| r.series.as_deref() == Some("Saga")).expect("Saga #17 is on Metron");
        let full = details(saga, (&user, &password)).unwrap();
        assert!(full.authors.iter().any(|a| a.contains("Vaughan")), "{full:#?}");
        assert!(full.comic.as_ref().is_some_and(|c| !c.artists.is_empty()), "{full:#?}");
    }

    #[test]
    fn issue_numbers() {
        assert_eq!(leading_number("17"), Some(17.0));
        assert_eq!(leading_number("1.MU"), Some(1.0));
        assert_eq!(leading_number("½"), Some(0.5));
        assert_eq!(leading_number("Annual 1"), None);
    }
}
