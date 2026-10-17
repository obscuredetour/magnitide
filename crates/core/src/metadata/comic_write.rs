//! Writing a comic's metadata into its CBZ as ComicInfo.xml, the reverse of
//! [`super::comic`]. Only the tags Magnitide manages are set; every other one
//! the file has (Pages, which marks the front cover, Notes, Web, ratings, …)
//! is kept as it is, and the result follows the schema's element order. The
//! pages are copied into the new ZIP as they are, never recompressed.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::comic::{ComicDetails, issue_title};
use super::{BookMetadata, language_tag};

/// ComicInfo.xml's elements, in the order its schema (an xs:sequence) gives them.
const SCHEMA_ORDER: &[&str] = &[
    "Title",
    "Series",
    "Number",
    "Count",
    "Volume",
    "AlternateSeries",
    "AlternateNumber",
    "AlternateCount",
    "Summary",
    "Notes",
    "Year",
    "Month",
    "Day",
    "Writer",
    "Penciller",
    "Inker",
    "Colorist",
    "Letterer",
    "CoverArtist",
    "Editor",
    "Translator",
    "Publisher",
    "Imprint",
    "Genre",
    "Tags",
    "Web",
    "PageCount",
    "LanguageISO",
    "Format",
    "BlackAndWhite",
    "Manga",
    "Characters",
    "Teams",
    "Locations",
    "ScanInformation",
    "StoryArc",
    "StoryArcNumber",
    "SeriesGroup",
    "AgeRating",
    "Pages",
    "CommunityRating",
    "MainCharacterOrTeam",
    "Review",
    "GTIN",
];

/// The elements Magnitide sets; the others are the file's own.
const MANAGED: &[&str] = &[
    "Title",
    "Series",
    "Number",
    "Volume",
    "Summary",
    "Year",
    "Month",
    "Day",
    "Writer",
    "Penciller",
    "Inker",
    "Colorist",
    "Letterer",
    "CoverArtist",
    "Editor",
    "Publisher",
    "Genre",
    "Tags",
    "LanguageISO",
    "Format",
    "Manga",
    "Characters",
    "Teams",
    "Locations",
    "StoryArc",
    "AgeRating",
    "GTIN",
];

/// Whether the CBZ's ComicInfo.xml already says what the library says.
pub fn is_current(path: &Path, meta: &BookMetadata, comic: &ComicDetails) -> bool {
    let Some(existing) = read_existing(path) else { return false };
    let old = existing.as_deref().map(elements).unwrap_or_default();
    let want = wanted(meta, comic, &old);
    let squash = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let have = |tag: &str| old.get(tag).map(|e| squash(&e.text)).unwrap_or_default();
    let wants = |tag: &str| want.get(tag).map(|v| squash(v)).unwrap_or_default();
    // Genres and tags as one set (a tag can be in both, or move between them),
    // and the ISBN by its digits: how the reader takes them in.
    let tag_set = |genre: String, tags: String| {
        let mut set: Vec<String> = [genre, tags]
            .iter()
            .flat_map(|l| l.split(','))
            .map(|t| t.trim().to_lowercase())
            .filter(|t| !t.is_empty())
            .collect();
        set.sort();
        set.dedup();
        set
    };
    let digits = |s: String| s.chars().filter(char::is_ascii_alphanumeric).collect::<String>();
    MANAGED.iter().all(|tag| match *tag {
        "Genre" | "Tags" => tag_set(have("Genre"), have("Tags")) == tag_set(wants("Genre"), wants("Tags")),
        "GTIN" => digits(have("GTIN")) == digits(wants("GTIN")),
        tag => have(tag) == wants(tag),
    })
}

/// Write the library's metadata into the CBZ's ComicInfo.xml (made if it has
/// none), beside the file and then renamed over it, so a failure leaves it as it was.
pub fn write(path: &Path, meta: &BookMetadata, comic: &ComicDetails) -> Result<(), String> {
    let existing = read_existing(path).ok_or("it isn't a ZIP archive")?;
    let xml = render(existing.as_deref(), meta, comic);

    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.incoming"));
    let result = (|| -> Result<(), String> {
        let mut src =
            ZipArchive::new(BufReader::new(File::open(path).map_err(|e| e.to_string())?)).map_err(|e| e.to_string())?;
        let mut out = ZipWriter::new(BufWriter::new(File::create(&tmp).map_err(|e| e.to_string())?));
        for i in 0..src.len() {
            let entry = src.by_index_raw(i).map_err(|e| e.to_string())?;
            if is_comic_info(entry.name()) {
                continue;
            }
            out.raw_copy_file(entry).map_err(|e| e.to_string())?;
        }
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        out.start_file("ComicInfo.xml", options).map_err(|e| e.to_string())?;
        out.write_all(xml.as_bytes()).map_err(|e| e.to_string())?;
        // A ComicBookInfo comment stays: readers take ComicInfo.xml first.
        if !src.comment().is_empty() {
            out.set_comment(String::from_utf8_lossy(src.comment()).into_owned()).map_err(|e| e.to_string())?;
        }
        out.finish().map_err(|e| e.to_string())?.flush().map_err(|e| e.to_string())
    })();
    match result {
        Ok(()) => std::fs::rename(&tmp, path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            e.to_string()
        }),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

fn is_comic_info(name: &str) -> bool {
    name.rsplit(['/', '\\']).next().is_some_and(|n| n.eq_ignore_ascii_case("ComicInfo.xml"))
}

/// The ZIP's ComicInfo.xml (the one nearest the top), Some(None) if it has
/// none; None if the file isn't a ZIP.
fn read_existing(path: &Path) -> Option<Option<String>> {
    let mut zip = ZipArchive::new(BufReader::new(File::open(path).ok()?)).ok()?;
    let name = zip.file_names().filter(|n| is_comic_info(n)).min_by_key(|n| n.len()).map(str::to_owned);
    let Some(name) = name else { return Some(None) };
    let mut xml = String::new();
    zip.by_name(&name).ok()?.take(1 << 20).read_to_string(&mut xml).ok()?;
    Some(Some(xml))
}

/// An element of the file's ComicInfo.xml: its text, and its markup as written.
#[derive(Debug, Clone, Default)]
struct Element {
    text: String,
    raw: String,
}

/// The root's child elements by name (the first of each), in document order with them.
fn elements(xml: &str) -> BTreeMap<String, Element> {
    let mut out = BTreeMap::new();
    let xml = xml.trim_start_matches('\u{feff}');
    let Ok(doc) = roxmltree::Document::parse(xml) else { return out };
    for node in doc.root_element().children().filter(|n| n.is_element()) {
        let name = node.tag_name().name().to_owned();
        out.entry(name).or_insert_with(|| Element {
            text: node.text().unwrap_or_default().trim().to_owned(),
            raw: xml[node.range()].to_owned(),
        });
    }
    out
}

/// The managed elements' values from the library; empty ones are left out.
fn wanted(meta: &BookMetadata, c: &ComicDetails, old: &BTreeMap<String, Element>) -> BTreeMap<&'static str, String> {
    let mut out = BTreeMap::new();
    let mut put = |tag: &'static str, value: Option<String>| {
        if let Some(v) = value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()) {
            out.insert(tag, v);
        }
    };
    let list = |v: &[String]| (!v.is_empty()).then(|| v.join(", "));

    let series = meta.series.as_ref();
    let number = c.issue.clone().or_else(|| {
        series.map(|(_, i)| magnitide_catalog::format_number(*i, c.issue_end)).map(|n| n.replace('–', "-"))
    });
    // A title that's only "Series #Number" is what readers make anyway: leave it unwritten.
    let made_up = series.map(|(s, _)| issue_title(s, number.as_deref()));
    put("Title", meta.title.clone().filter(|t| Some(t) != made_up.as_ref()));
    put("Series", series.map(|(s, _)| s.clone()));
    put("Number", number);
    put("Volume", c.volume.map(|v| v.to_string()));
    put("Summary", meta.description.as_deref().map(html_to_text));
    if let Some(d) = meta.published {
        use chrono::Datelike;
        put("Year", Some(d.year().to_string()));
        put("Month", Some(d.month().to_string()));
        put("Day", Some(d.day().to_string()));
    }
    put("Writer", list(&meta.authors.iter().map(|a| a.name.clone()).collect::<Vec<_>>()));
    put("Penciller", list(&c.artists));
    put("Inker", list(&c.inkers));
    put("Colorist", list(&c.colorists));
    put("Letterer", list(&c.letterers));
    put("CoverArtist", list(&c.cover_artists));
    put("Editor", list(&c.editors));
    put("Publisher", meta.publisher.clone());
    // The file's genres stay genres (those still tagged); the other tags go in Tags.
    let genres: Vec<String> = old
        .get("Genre")
        .map(|g| g.text.split(',').map(str::trim).filter(|g| !g.is_empty()).map(str::to_owned).collect())
        .unwrap_or_default();
    let is_genre = |t: &str| genres.iter().any(|g| g.eq_ignore_ascii_case(t));
    put("Genre", list(&meta.tags.iter().filter(|t| is_genre(t)).cloned().collect::<Vec<_>>()));
    put("Tags", list(&meta.tags.iter().filter(|t| !is_genre(t)).cloned().collect::<Vec<_>>()));
    put("LanguageISO", meta.languages.first().map(|l| language_tag(l)));
    put("Format", c.format.map(str::to_owned));
    // Manga beyond right to left (Yes, No) is the file's to say.
    let old_manga = old.get("Manga").map(|m| m.text.clone());
    put(
        "Manga",
        if c.right_to_left {
            Some("YesAndRightToLeft".into())
        } else if old_manga.as_deref().is_some_and(|m| m.eq_ignore_ascii_case("YesAndRightToLeft")) {
            Some("Yes".into())
        } else {
            old_manga
        },
    );
    put("Characters", list(&c.characters));
    put("Teams", list(&c.teams));
    put("Locations", list(&c.locations));
    put("StoryArc", list(&c.story_arcs));
    put("AgeRating", c.age_rating.clone());
    put("GTIN", meta.identifiers.get("isbn").cloned());
    out
}

/// The new ComicInfo.xml: managed elements from the library, the rest as the
/// file had them, in schema order (elements the schema doesn't know last).
fn render(existing: Option<&str>, meta: &BookMetadata, comic: &ComicDetails) -> String {
    let old = existing.map(elements).unwrap_or_default();
    let want = wanted(meta, comic, &old);
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<ComicInfo xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" \
         xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\n",
    );
    let unknown = old.keys().filter(|k| !SCHEMA_ORDER.contains(&k.as_str()));
    for tag in SCHEMA_ORDER.iter().copied().chain(unknown.map(String::as_str)) {
        if MANAGED.contains(&tag) {
            if let Some(value) = want.get(tag) {
                xml.push_str(&format!("  <{tag}>{}</{tag}>\n", escape(value)));
            }
        } else if let Some(e) = old.get(tag) {
            xml.push_str("  ");
            xml.push_str(&e.raw);
            xml.push('\n');
        }
    }
    xml.push_str("</ComicInfo>\n");
    xml
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// A description's HTML as ComicInfo's plain-text Summary: paragraphs and
/// line breaks become new lines, other tags go, entities are decoded.
fn html_to_text(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('>') else {
            rest = "";
            break;
        };
        let tag = rest[start + 1..start + end].trim().to_ascii_lowercase();
        let name = tag.trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
        if matches!(name, "br" | "p" | "div" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6") && !out.ends_with('\n') {
            out.push('\n');
        }
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    let text = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&");
    text.lines().map(str::trim).collect::<Vec<_>>().join("\n").trim().to_owned()
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;
    use crate::metadata::Author;
    use crate::metadata::comic::tests::make_cbz;

    fn saga() -> (BookMetadata, ComicDetails) {
        let meta = BookMetadata {
            title: Some("Chapter Seventeen".into()),
            authors: vec![Author { name: "Brian K. Vaughan".into(), sort: None }],
            series: Some(("Saga".into(), 17.0)),
            publisher: Some("Image".into()),
            published: NaiveDate::from_ymd_opt(2013, 12, 11),
            languages: vec!["eng".into()],
            description: Some("<p>Prince Robot IV gets everyone into <em>trouble</em> &amp; more.</p>".into()),
            tags: vec!["Science Fiction".into(), "space opera".into()],
            ..Default::default()
        };
        let comic = ComicDetails {
            artists: vec!["Fiona Staples".into()],
            cover_artists: vec!["Fiona Staples".into()],
            letterers: vec!["Fonografiks".into()],
            format: Some("Issue"),
            right_to_left: false,
            ..Default::default()
        };
        (meta, comic)
    }

    fn comic_info(path: &Path) -> String {
        read_existing(path).unwrap().unwrap()
    }

    #[test]
    fn writes_comic_info_keeping_the_files_own_elements() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("saga17.cbz");
        let old = "<ComicInfo><Title>Old</Title><Genre>Science Fiction</Genre><Web>https://comicvine.example/17</Web>\
                   <Notes>Tagged by hand</Notes><Pages><Page Image=\"0\" Type=\"FrontCover\"/></Pages>\
                   <Custom>kept</Custom></ComicInfo>";
        make_cbz(&path, &[("01.jpg", b"page one"), ("ComicInfo.xml", old.as_bytes())], Some("a comment"));
        let (meta, comic) = saga();
        assert!(!is_current(&path, &meta, &comic));

        write(&path, &meta, &comic).unwrap();
        let xml = comic_info(&path);
        // Managed elements from the library, in schema order, the file's own kept.
        let order: Vec<&str> = [
            "<Title>",
            "<Series>",
            "<Number>17",
            "<Summary>",
            "<Notes>",
            "<Year>2013",
            "<Writer>",
            "<Penciller>",
            "<Letterer>",
            "<CoverArtist>",
            "<Publisher>",
            "<Genre>Science Fiction",
            "<Tags>space opera",
            "<Web>",
            "<LanguageISO>en",
            "<Format>Issue",
            "<Pages>",
            "<Custom>kept",
        ]
        .to_vec();
        let mut at = 0;
        for piece in order {
            let found = xml[at..].find(piece).unwrap_or_else(|| panic!("{piece} after {at} in\n{xml}"));
            at += found;
        }
        assert!(xml.contains("<Summary>Prince Robot IV gets everyone into trouble &amp; more.</Summary>"), "{xml}");
        assert!(!xml.contains("Old"));

        // The pages are untouched, the comment kept, and it reads back as the library says.
        let mut zip = ZipArchive::new(File::open(&path).unwrap()).unwrap();
        let mut page = String::new();
        zip.by_name("01.jpg").unwrap().read_to_string(&mut page).unwrap();
        assert_eq!((page.as_str(), zip.comment()), ("page one", &b"a comment"[..]));
        assert_eq!(zip.file_names().filter(|n| is_comic_info(n)).count(), 1);
        let back = crate::metadata::comic::read(&path).unwrap();
        assert_eq!(back.title.as_deref(), Some("Chapter Seventeen"));
        assert_eq!(back.series, Some(("Saga".into(), 17.0)));
        assert_eq!(back.comic.unwrap().artists, ["Fiona Staples"]);
        assert!(is_current(&path, &meta, &comic));
    }

    #[test]
    fn makes_comic_info_for_a_cbz_without_one() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("bare.cbz");
        make_cbz(&path, &[("01.jpg", b"one")], None);
        let (mut meta, comic) = saga();
        meta.title = Some("Saga #17".into()); // what a reader would make: not written
        write(&path, &meta, &ComicDetails { right_to_left: true, ..comic.clone() }).unwrap();
        let xml = comic_info(&path);
        assert!(!xml.contains("<Title>"), "{xml}");
        assert!(xml.contains("<Manga>YesAndRightToLeft</Manga>"));
        assert_eq!(crate::metadata::comic::read(&path).unwrap().title.as_deref(), Some("Saga #17"));
    }

    #[test]
    fn issue_text_and_ranges_are_the_number() {
        let (meta, comic) = saga();
        let w = wanted(&meta, &ComicDetails { issue: Some("17.MU".into()), ..comic.clone() }, &BTreeMap::new());
        assert_eq!(w["Number"], "17.MU");
        let tpb = BookMetadata { series: Some(("Saga TPB".into(), 1.0)), ..meta };
        let w = wanted(&tpb, &ComicDetails { issue_end: Some(6.0), ..comic }, &BTreeMap::new());
        assert_eq!(w["Number"], "1-6");
    }
}
