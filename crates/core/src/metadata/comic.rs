//! Comic book archives: CBZ, CB7, and the many CBRs that are really ZIPs
//! (true RAR archives can't be read here). Metadata comes from ComicInfo.xml
//! (ComicRack's, which ComicTagger, Mylar, Kavita and Komga read and write)
//! or else ComicBookInfo (JSON in the ZIP's comment); the cover is the page
//! ComicInfo marks as the front cover, else the first image.

use std::cmp::Ordering;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use chrono::NaiveDate;
use zip::ZipArchive;

use super::{Author, BookMetadata, language_code};

/// What a comic carries beyond a book's metadata. Stored in the "comic"
/// group of custom fields (see `Library::ensure_comic_fields`).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ComicDetails {
    /// The issue number as printed: "1", "1.MU", "½", "Annual 1".
    pub issue: Option<String>,
    /// Where a collected edition's issues end ("1-6" → 6); the series index is where they start.
    pub issue_end: Option<f64>,
    /// Which run of the series: ComicInfo's Volume, often the year it began.
    pub volume: Option<i64>,
    /// What kind of publication it is: one of [`FORMATS`] ("Issue", "TPB", …).
    pub format: Option<&'static str>,
    /// The art: ComicInfo's Penciller, and Artist or Illustrator credits elsewhere.
    pub artists: Vec<String>,
    pub inkers: Vec<String>,
    pub colorists: Vec<String>,
    pub letterers: Vec<String>,
    pub cover_artists: Vec<String>,
    pub editors: Vec<String>,
    pub story_arcs: Vec<String>,
    pub characters: Vec<String>,
    pub teams: Vec<String>,
    pub locations: Vec<String>,
    pub age_rating: Option<String>,
    /// Read right to left (manga): ComicInfo's Manga "YesAndRightToLeft".
    pub right_to_left: bool,
}

/// The kinds of comic publication, as the Format field offers them. The
/// details panel words a comic's place in its series by these: "Issue 17",
/// "Volume 3", "Annual 1".
pub const FORMATS: &[&str] =
    &["Issue", "Annual", "Special", "One-Shot", "Graphic Novel", "TPB", "Hardcover", "Omnibus", "Manga Volume"];

/// ComicInfo's Format (free text: "Series", "Trade Paperback", "HC", …) as one
/// of [`FORMATS`]; None for kinds it doesn't name ("Preview", "Variant").
pub fn comic_format(raw: &str) -> Option<&'static str> {
    let key: String = raw.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase();
    Some(match key.as_str() {
        "issue" | "series" | "ongoing" | "ongoingseries" | "limitedseries" | "miniseries" | "maxiseries"
        | "digitalchapter" | "webcomic" => "Issue",
        "annual" => "Annual",
        "special" | "giant" | "giantsize" => "Special",
        "oneshot" => "One-Shot",
        "graphicnovel" | "gn" | "ogn" | "originalgraphicnovel" => "Graphic Novel",
        "tpb" | "tradepaperback" | "trade" | "softcover" | "sc" => "TPB",
        "hc" | "hardcover" | "hardback" => "Hardcover",
        "omnibus" => "Omnibus",
        "tankobon" | "mangavolume" => "Manga Volume",
        _ => return None,
    })
}

/// The formats this module reads.
pub fn is_comic_format(format: &str) -> bool {
    matches!(format, "CBZ" | "CBR" | "CB7")
}

/// A comic's metadata and cover; `meta.comic` is always set. None when the
/// archive can't be opened (a real RAR, or a damaged file).
pub fn read(path: &Path) -> Option<BookMetadata> {
    let mut archive = Archive::open(path)?;
    let names = archive.names();
    let info = names
        .iter()
        .filter(|n| base(n).eq_ignore_ascii_case("ComicInfo.xml"))
        .min_by_key(|n| n.len())
        .and_then(|n| archive.read(n, MAX_INFO))
        .and_then(|bytes| String::from_utf8(bytes).ok());
    let (mut meta, front_cover) = match info.as_deref().and_then(parse_comic_info) {
        Some(found) => found,
        None => (archive.comment().and_then(parse_comic_book_info).unwrap_or_default(), None),
    };
    meta.comic.get_or_insert_with(Default::default);

    let mut pages: Vec<&String> = names.iter().filter(|n| is_page(n)).collect();
    pages.sort_by(|a, b| natural_cmp(a, b));
    let cover = front_cover.and_then(|i| pages.get(i)).or(pages.first());
    meta.cover = cover.and_then(|n| archive.read(n, MAX_PAGE));
    Some(meta)
}

/// ComicInfo.xml is a few kilobytes; a page rarely more than tens of megabytes.
const MAX_INFO: u64 = 1 << 20;
const MAX_PAGE: u64 = 64 << 20;

enum Archive {
    Zip(ZipArchive<BufReader<File>>),
    SevenZ(Box<sevenz_rust2::ArchiveReader<File>>),
}

impl Archive {
    /// By content, not extension: CBRs are often ZIPs, and CBZs sometimes 7z.
    fn open(path: &Path) -> Option<Self> {
        let mut magic = [0u8; 6];
        File::open(path).ok()?.read_exact(&mut magic).ok()?;
        if magic.starts_with(b"PK\x03\x04") {
            ZipArchive::new(BufReader::new(File::open(path).ok()?)).ok().map(Self::Zip)
        } else if magic == *b"7z\xbc\xaf\x27\x1c" {
            let reader = sevenz_rust2::ArchiveReader::open(path, sevenz_rust2::Password::empty()).ok()?;
            Some(Self::SevenZ(Box::new(reader)))
        } else {
            None
        }
    }

    fn names(&self) -> Vec<String> {
        match self {
            Self::Zip(z) => z.file_names().filter(|n| !n.ends_with('/')).map(str::to_owned).collect(),
            Self::SevenZ(r) => {
                r.archive().files.iter().filter(|e| !e.is_directory()).map(|e| e.name().to_owned()).collect()
            }
        }
    }

    /// An entry's bytes, if it's no bigger than `max`.
    fn read(&mut self, name: &str, max: u64) -> Option<Vec<u8>> {
        match self {
            Self::Zip(z) => {
                let entry = z.by_name(name).ok()?;
                if entry.size() > max {
                    return None;
                }
                let mut out = Vec::with_capacity(entry.size() as usize);
                entry.take(max).read_to_end(&mut out).ok()?;
                Some(out)
            }
            Self::SevenZ(r) => {
                let size = r.archive().files.iter().find(|e| e.name() == name)?.size();
                if size > max {
                    return None;
                }
                r.read_file(name).ok()
            }
        }
    }

    fn comment(&self) -> Option<&str> {
        match self {
            Self::Zip(z) => std::str::from_utf8(z.comment()).ok().filter(|c| !c.trim().is_empty()),
            Self::SevenZ(_) => None,
        }
    }
}

fn base(name: &str) -> &str {
    name.rsplit(['/', '\\']).next().unwrap_or(name)
}

/// An image that's a page: not in macOS's resource folder or a hidden file.
fn is_page(name: &str) -> bool {
    let hidden = name.split(['/', '\\']).any(|part| part.starts_with('.') || part == "__MACOSX");
    let ext = base(name).rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    !hidden && matches!(ext.as_deref(), Some("jpg" | "jpeg" | "png" | "webp" | "gif"))
}

/// Order names as people number pages: "page2" before "page10", case aside.
pub(crate) fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars<'_>>| {
                    let mut digits = String::new();
                    while let Some(c) = it.next_if(char::is_ascii_digit) {
                        digits.push(c);
                    }
                    digits
                };
                let (da, db) = (take(&mut a), take(&mut b));
                let (ta, tb) = (da.trim_start_matches('0'), db.trim_start_matches('0'));
                match ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb)).then_with(|| da.len().cmp(&db.len())) {
                    Ordering::Equal => {}
                    other => return other,
                }
            }
            (Some(x), Some(y)) => {
                match x.to_lowercase().cmp(y.to_lowercase()) {
                    Ordering::Equal => {}
                    other => return other,
                }
                a.next();
                b.next();
            }
        }
    }
}

/// ComicInfo.xml's fields, and which page is the front cover (an index into the pages in order).
fn parse_comic_info(xml: &str) -> Option<(BookMetadata, Option<usize>)> {
    let doc = roxmltree::Document::parse(xml.trim_start_matches('\u{feff}')).ok()?;
    let root = doc.root_element();
    if !root.tag_name().name().eq_ignore_ascii_case("ComicInfo") {
        return None;
    }
    let text = |tag: &str| {
        root.children()
            .find(|n| n.is_element() && n.tag_name().name() == tag)
            .and_then(|n| n.text())
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
    };
    let list = |tag: &str| text(tag).map(|t| split_list(&t)).unwrap_or_default();
    let int = |tag: &str| text(tag).and_then(|t| t.parse::<i64>().ok()).filter(|n| *n > 0);

    let series = text("Series");
    let number = text("Number");
    let (start, end) = number.as_deref().map(issue_range).unwrap_or((None, None));
    let title = text("Title").or_else(|| series.as_ref().map(|s| issue_title(s, number.as_deref())));
    let mut tags = list("Genre");
    for tag in list("Tags") {
        if !tags.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
            tags.push(tag);
        }
    }
    let mut identifiers = std::collections::BTreeMap::new();
    if let Some(isbn) = text("GTIN").map(|g| g.replace(['-', ' '], "")).filter(|g| is_isbn13(g)) {
        identifiers.insert("isbn".to_owned(), isbn);
    }
    let published = int("Year").and_then(|y| {
        let month = int("Month").unwrap_or(1) as u32;
        let day = int("Day").unwrap_or(1) as u32;
        NaiveDate::from_ymd_opt(y as i32, month, day).or_else(|| NaiveDate::from_ymd_opt(y as i32, 1, 1))
    });
    let front_cover = root
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "Pages")
        .into_iter()
        .flat_map(|pages| pages.children().filter(|n| n.is_element()))
        .find(|p| p.attribute("Type").is_some_and(|t| t.eq_ignore_ascii_case("FrontCover")))
        .and_then(|p| p.attribute("Image")?.trim().parse::<usize>().ok());

    let comic = ComicDetails {
        issue: number,
        issue_end: end,
        volume: int("Volume"),
        format: text("Format").and_then(|f| comic_format(&f)),
        artists: list("Penciller"),
        inkers: list("Inker"),
        colorists: list("Colorist"),
        letterers: list("Letterer"),
        cover_artists: list("CoverArtist"),
        editors: list("Editor"),
        story_arcs: list("StoryArc"),
        characters: list("Characters"),
        teams: list("Teams"),
        locations: list("Locations"),
        age_rating: text("AgeRating").filter(|r| !r.eq_ignore_ascii_case("Unknown")),
        right_to_left: text("Manga").is_some_and(|m| m.eq_ignore_ascii_case("YesAndRightToLeft")),
    };
    let meta = BookMetadata {
        title,
        authors: authors(list("Writer")),
        series: series.map(|s| (s, start.unwrap_or(1.0))),
        publisher: text("Publisher"),
        published,
        languages: text("LanguageISO").map(|l| vec![language_code(&l)]).unwrap_or_default(),
        identifiers,
        description: text("Summary"),
        tags,
        comic: Some(Box::new(comic)),
        ..Default::default()
    };
    Some((meta, front_cover))
}

/// ComicBookInfo/1.0, the JSON ComicBookLover kept in the ZIP comment.
fn parse_comic_book_info(comment: &str) -> Option<BookMetadata> {
    let json: serde_json::Value = serde_json::from_str(comment).ok()?;
    let info = json.get("ComicBookInfo/1.0")?;
    let text = |key: &str| match info.get(key)? {
        serde_json::Value::String(s) => Some(s.trim().to_owned()).filter(|s| !s.is_empty()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    };
    let int = |key: &str| info.get(key).and_then(serde_json::Value::as_i64).filter(|n| *n > 0);

    let mut comic = ComicDetails { volume: int("volume"), ..Default::default() };
    let mut writers = Vec::new();
    for credit in info.get("credits").and_then(|c| c.as_array()).into_iter().flatten() {
        let (Some(person), Some(role)) = (
            credit.get("person").and_then(|p| p.as_str()).map(str::trim).filter(|p| !p.is_empty()),
            credit.get("role").and_then(|r| r.as_str()),
        ) else {
            continue;
        };
        let into = match role.to_ascii_lowercase().replace(' ', "").as_str() {
            "writer" | "plotter" | "scripter" => &mut writers,
            "penciller" | "penciler" | "pencils" | "artist" | "artwork" | "illustrator" => &mut comic.artists,
            "inker" => &mut comic.inkers,
            "colorist" | "colourist" | "colorer" => &mut comic.colorists,
            "letterer" => &mut comic.letterers,
            "cover" | "coverartist" => &mut comic.cover_artists,
            "editor" => &mut comic.editors,
            _ => continue,
        };
        if !into.iter().any(|p| p == person) {
            into.push(person.to_owned());
        }
    }

    let series = text("series");
    let number = text("issue");
    let (start, end) = number.as_deref().map(issue_range).unwrap_or((None, None));
    (comic.issue, comic.issue_end) = (number.clone(), end);
    let published = int("publicationYear")
        .and_then(|y| NaiveDate::from_ymd_opt(y as i32, int("publicationMonth").unwrap_or(1) as u32, 1));
    let mut tags: Vec<String> = text("genre").map(|g| split_list(&g)).unwrap_or_default();
    for tag in info.get("tags").and_then(|t| t.as_array()).into_iter().flatten().filter_map(|t| t.as_str()) {
        let tag = tag.trim();
        if !tag.is_empty() && !tags.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
            tags.push(tag.to_owned());
        }
    }
    Some(BookMetadata {
        title: text("title").or_else(|| series.as_ref().map(|s| issue_title(s, number.as_deref()))),
        authors: authors(writers),
        series: series.map(|s| (s, start.unwrap_or(1.0))),
        publisher: text("publisher"),
        published,
        languages: text("language").filter(|l| l.len() <= 3).map(|l| vec![language_code(&l)]).unwrap_or_default(),
        description: text("comments"),
        tags,
        comic: Some(Box::new(comic)),
        ..Default::default()
    })
}

fn authors(names: Vec<String>) -> Vec<Author> {
    names.into_iter().map(|name| Author { name, sort: None }).collect()
}

/// ComicInfo separates names and lists with commas.
fn split_list(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in s.split([',', ';']).map(str::trim).filter(|i| !i.is_empty()) {
        if !out.iter().any(|o| o.eq_ignore_ascii_case(item)) {
            out.push(item.to_owned());
        }
    }
    out
}

/// "Saga #1", for an issue with no title of its own.
fn issue_title(series: &str, number: Option<&str>) -> String {
    match number {
        Some(n) => format!("{series} #{n}"),
        None => series.to_owned(),
    }
}

/// An issue number as a series index, and where a range of them ends:
/// "3" → 3, "001" → 1, "1.5" → 1.5, "½" → 0.5, "1.MU" → 1, "1-6" → 1 to 6.
/// None for numbers that aren't ("Annual 1").
pub(crate) fn issue_range(number: &str) -> (Option<f64>, Option<f64>) {
    let n = number.trim();
    for sep in ['-', '–', '—'] {
        if let Some((a, b)) = n.split_once(sep)
            && let (Some(start), Some(end)) = (leading_number(a), leading_number(b))
            && end > start
        {
            return (Some(start), Some(end));
        }
    }
    (leading_number(n), None)
}

fn leading_number(s: &str) -> Option<f64> {
    let s = s.trim();
    if s == "½" {
        return Some(0.5);
    }
    let (whole, half) = s.strip_suffix('½').map_or((s, 0.0), |w| (w, 0.5));
    let end = whole.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(whole.len());
    let digits = whole[..end].trim_end_matches('.');
    if digits.is_empty() {
        return (half > 0.0).then_some(half);
    }
    digits.parse::<f64>().ok().map(|n| n + half)
}

fn is_isbn13(s: &str) -> bool {
    s.len() == 13 && s.chars().all(|c| c.is_ascii_digit()) && (s.starts_with("978") || s.starts_with("979"))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write;

    use super::*;

    pub(crate) const COMIC_INFO: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<ComicInfo xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Series>Saga</Series>
  <Number>1</Number>
  <Volume>2012</Volume>
  <Summary>When two soldiers from opposite sides fall in love.</Summary>
  <Year>2012</Year><Month>3</Month><Day>14</Day>
  <Writer>Brian K. Vaughan</Writer>
  <Penciller>Fiona Staples</Penciller>
  <Colorist>Fiona Staples</Colorist>
  <Letterer>Fonografiks</Letterer>
  <CoverArtist>Fiona Staples</CoverArtist>
  <Editor>Eric Stephenson, Fiona Staples</Editor>
  <Publisher>Image</Publisher>
  <Genre>Science Fiction, Fantasy</Genre>
  <Tags>space opera, fantasy</Tags>
  <LanguageISO>en</LanguageISO>
  <GTIN>978-1-60706-601-9</GTIN>
  <Characters>Alana, Marko, Hazel</Characters>
  <Teams>The Will</Teams>
  <StoryArc>Chapter One</StoryArc>
  <AgeRating>M</AgeRating>
  <Manga>No</Manga>
  <Pages><Page Image="0" Type="Story"/><Page Image="1" Type="FrontCover"/></Pages>
</ComicInfo>"#;

    pub(crate) fn make_cbz(path: &Path, entries: &[(&str, &[u8])], comment: Option<&str>) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, body) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(body).unwrap();
        }
        if let Some(c) = comment {
            zip.set_comment(c).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn reads_comic_info_and_its_front_cover() {
        let tmp = tempfile::tempdir().unwrap();
        // The second page in order is marked as the front cover; names sort naturally.
        let path = tmp.path().join("saga.cbr"); // a ZIP named .cbr, as many are
        make_cbz(
            &path,
            &[
                ("Saga/page10.jpg", b"ten"),
                ("Saga/page2.jpg", b"two"),
                ("Saga/page1.jpg", b"one"),
                ("__MACOSX/Saga/._page1.jpg", b"junk"),
                ("ComicInfo.xml", COMIC_INFO.as_bytes()),
            ],
            None,
        );
        let m = read(&path).unwrap();
        assert_eq!(m.title.as_deref(), Some("Saga #1"));
        assert_eq!(m.authors, [Author { name: "Brian K. Vaughan".into(), sort: None }]);
        assert_eq!(m.series, Some(("Saga".into(), 1.0)));
        assert_eq!(m.publisher.as_deref(), Some("Image"));
        assert_eq!(m.published, NaiveDate::from_ymd_opt(2012, 3, 14));
        assert_eq!(m.languages, ["eng"]);
        assert_eq!(m.identifiers.get("isbn").map(String::as_str), Some("9781607066019"));
        assert_eq!(m.tags, ["Science Fiction", "Fantasy", "space opera"]);
        assert_eq!(m.cover.as_deref(), Some(&b"two"[..]), "page 1 in natural order: page1, page2, page10");
        let c = m.comic.unwrap();
        assert_eq!((c.issue.as_deref(), c.issue_end, c.volume), (Some("1"), None, Some(2012)));
        assert_eq!(c.artists, ["Fiona Staples"]);
        assert_eq!(c.editors, ["Eric Stephenson", "Fiona Staples"]);
        assert_eq!(c.characters, ["Alana", "Marko", "Hazel"]);
        assert_eq!((c.story_arcs, c.teams), (vec!["Chapter One".to_owned()], vec!["The Will".to_owned()]));
        assert_eq!((c.age_rating.as_deref(), c.right_to_left), (Some("M"), false));
    }

    #[test]
    fn a_collected_edition_and_manga() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tpb.cbz");
        let xml = "<ComicInfo><Title>Saga, Volume One</Title><Series>Saga</Series><Number>1-6</Number>\
                   <Format>Trade Paperback</Format><Manga>YesAndRightToLeft</Manga></ComicInfo>";
        make_cbz(&path, &[("01.png", b"cover"), ("comicinfo.xml", xml.as_bytes())], None);
        let m = read(&path).unwrap();
        assert_eq!(m.title.as_deref(), Some("Saga, Volume One"));
        assert_eq!(m.series, Some(("Saga".into(), 1.0)));
        let c = m.comic.unwrap();
        assert_eq!((c.issue.as_deref(), c.issue_end, c.right_to_left), (Some("1-6"), Some(6.0), true));
        assert_eq!(c.format, Some("TPB"));
        assert_eq!(m.cover.as_deref(), Some(&b"cover"[..]));
    }

    #[test]
    fn reads_comic_book_info_from_the_zip_comment() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("old.cbz");
        let comment = r#"{"appID":"ComicTagger/1.0","ComicBookInfo/1.0":{"series":"Hellboy","title":"Seed of Destruction",
            "issue":"2","volume":1994,"publisher":"Dark Horse","publicationYear":1994,"publicationMonth":4,
            "language":"en","tags":["horror"],"credits":[{"person":"Mike Mignola","role":"Writer"},
            {"person":"John Byrne","role":"Writer"},{"person":"Mike Mignola","role":"Illustrator"},
            {"person":"Mark Chiarello","role":"Colorer"}]}}"#;
        make_cbz(&path, &[("1.jpg", b"one")], Some(comment));
        let m = read(&path).unwrap();
        assert_eq!(m.title.as_deref(), Some("Seed of Destruction"));
        assert_eq!(m.authors.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["Mike Mignola", "John Byrne"]);
        assert_eq!(m.series, Some(("Hellboy".into(), 2.0)));
        assert_eq!(m.published, NaiveDate::from_ymd_opt(1994, 4, 1));
        let c = m.comic.unwrap();
        assert_eq!(
            (c.artists, c.colorists, c.volume),
            (vec!["Mike Mignola".into()], vec!["Mark Chiarello".into()], Some(1994))
        );
    }

    #[test]
    fn a_comic_with_no_metadata_still_gets_a_cover() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("bare.cbz");
        make_cbz(&path, &[("b/002.jpg", b"two"), ("a/001.jpg", b"one"), ("notes.txt", b"x")], None);
        let m = read(&path).unwrap();
        assert_eq!((m.title, m.cover.as_deref()), (None, Some(&b"one"[..])));
        assert_eq!(m.comic.as_deref(), Some(&ComicDetails::default()));
        // Not an archive this module reads (a real RAR): nothing.
        let rar = tmp.path().join("real.cbr");
        std::fs::write(&rar, b"Rar!\x1a\x07\x01\x00rest").unwrap();
        assert!(read(&rar).is_none());
    }

    #[test]
    fn issue_numbers() {
        assert_eq!(issue_range("3"), (Some(3.0), None));
        assert_eq!(issue_range("001"), (Some(1.0), None));
        assert_eq!(issue_range("1.5"), (Some(1.5), None));
        assert_eq!(issue_range("½"), (Some(0.5), None));
        assert_eq!(issue_range("1½"), (Some(1.5), None));
        assert_eq!(issue_range("1.MU"), (Some(1.0), None));
        assert_eq!(issue_range("1-6"), (Some(1.0), Some(6.0)));
        assert_eq!(issue_range("1 – 6"), (Some(1.0), Some(6.0)));
        assert_eq!(issue_range("Annual 1"), (None, None));
        assert_eq!(issue_range("-1"), (None, None));
    }

    #[test]
    fn formats() {
        assert_eq!(comic_format("Series"), Some("Issue"));
        assert_eq!(comic_format("Limited Series"), Some("Issue"));
        assert_eq!(comic_format("HC"), Some("Hardcover"));
        assert_eq!(comic_format("one-shot"), Some("One-Shot"));
        assert_eq!(comic_format("Graphic Novel"), Some("Graphic Novel"));
        assert_eq!(comic_format("Preview"), None);
        assert!(FORMATS.iter().all(|f| comic_format(f) == Some(*f)), "each choice reads back as itself");
    }

    #[test]
    fn natural_order() {
        let mut names = vec!["p10.jpg", "P2.jpg", "p1.jpg", "p01b.jpg", "cover.jpg"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["cover.jpg", "p1.jpg", "p01b.jpg", "P2.jpg", "p10.jpg"]);
    }
}
