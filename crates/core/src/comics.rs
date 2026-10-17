//! Comic books' own fields: issue, volume, the art team by role, story arcs,
//! characters… They're ordinary custom fields in a "comic" group, made the
//! first time a comic is added (or a book is marked as one), so searching,
//! sorting and editing them work like any other field. A Bool field, #comic,
//! marks which books are comics; the editor shows the group for those.

use std::collections::BTreeMap;

use rusqlite::{OptionalExtension, params};

use crate::custom::{CustomField, FieldKind, FieldValue};
use crate::error::Result;
use crate::library::Library;
use crate::metadata::comic::{ComicDetails, FORMATS};

/// The group the comic fields are in (`CustomField::group`).
pub const COMIC_GROUP: &str = "comic";

/// The field that marks a book as a comic.
pub const COMIC_KEY: &str = "comic";

/// Key, name, kind and whether it holds a list, in the order they're shown.
pub const COMIC_FIELDS: &[(&str, &str, FieldKind, bool)] = &[
    (COMIC_KEY, "Comic", FieldKind::Bool, false),
    ("issue", "Issue", FieldKind::Text, false),
    ("format", "Format", FieldKind::Choice, false),
    ("volume", "Volume", FieldKind::Integer, false),
    ("artists", "Artists", FieldKind::Text, true),
    ("inkers", "Inkers", FieldKind::Text, true),
    ("colorists", "Colorists", FieldKind::Text, true),
    ("letterers", "Letterers", FieldKind::Text, true),
    ("cover_artists", "Cover Artists", FieldKind::Text, true),
    ("editors", "Editors", FieldKind::Text, true),
    ("story_arcs", "Story Arcs", FieldKind::Text, true),
    ("characters", "Characters", FieldKind::Text, true),
    ("teams", "Teams", FieldKind::Text, true),
    ("locations", "Locations", FieldKind::Text, true),
    ("age_rating", "Age Rating", FieldKind::Text, false),
    ("right_to_left", "Right to Left", FieldKind::Bool, false),
];

impl Library {
    /// Make the comic fields this library doesn't have yet. A field of the same
    /// name the user made themselves is kept as it is (and used if it's the
    /// same kind). Returns the comic fields that can hold comic values.
    pub fn ensure_comic_fields(&mut self) -> Result<Vec<CustomField>> {
        let tx = self.conn.transaction()?;
        for (key, name, kind, multiple) in COMIC_FIELDS {
            let config = match kind {
                FieldKind::Choice => serde_json::json!({ "group": COMIC_GROUP, "choices": FORMATS }),
                _ => serde_json::json!({ "group": COMIC_GROUP }),
            }
            .to_string();
            let taken: Option<i64> =
                tx.query_row("SELECT id FROM custom_fields WHERE key = ?", [key], |r| r.get(0)).optional()?;
            if taken.is_none() {
                tx.execute(
                    "INSERT INTO custom_fields (key, name, kind, multiple, config) VALUES (?, ?, ?, ?, ?)",
                    params![key, name, kind.db_name(), multiple, config],
                )?;
            }
        }
        tx.commit()?;
        Ok(self
            .custom_fields()?
            .into_iter()
            .filter(|f| {
                COMIC_FIELDS.iter().any(|(key, _, kind, multiple)| {
                    f.key == *key && f.kind == *kind && (f.multiple == *multiple || f.kind != FieldKind::Text)
                })
            })
            .collect())
    }
}

/// Whether CBR comics added to this library are repacked as CBZ on the way in,
/// so the library keeps the open, writable format. Off unless turned on.
const CONVERT_CBR: &str = "convert_cbr_on_add";

impl Library {
    pub fn convert_cbr_on_add(&self) -> Result<bool> {
        let value: Option<String> = self
            .conn
            .query_row("SELECT value FROM library_info WHERE key = ?", [CONVERT_CBR], |r| r.get(0))
            .optional()?;
        Ok(value.as_deref() == Some("true"))
    }

    pub fn set_convert_cbr_on_add(&mut self, on: bool) -> Result<()> {
        self.conn.execute(
            "INSERT INTO library_info (key, value) VALUES (?, ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![CONVERT_CBR, on.to_string()],
        )?;
        Ok(())
    }
}

impl Library {
    /// A book's comic values, from its comic fields (a field of the user's own
    /// with the same name doesn't count). The range end isn't a field: it's the
    /// book's `series_end`.
    pub(crate) fn comic_details(&self, id: crate::library::BookId) -> Result<ComicDetails> {
        let fields = self.custom_fields()?;
        let values = self.custom_edit_values(id)?;
        let get = |key: &str| {
            let ours = fields.iter().any(|f| f.key == key && f.group.as_deref() == Some(COMIC_GROUP));
            if ours { values.get(key).cloned().flatten() } else { None }
        };
        let text = |key: &str| match get(key) {
            Some(FieldValue::Text(t)) => Some(t).filter(|t| !t.trim().is_empty()),
            _ => None,
        };
        let list = |key: &str| match get(key) {
            Some(FieldValue::List(v)) => v,
            Some(FieldValue::Text(t)) => {
                t.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect()
            }
            _ => Vec::new(),
        };
        Ok(ComicDetails {
            issue: text("issue"),
            issue_end: None,
            volume: match get("volume") {
                Some(FieldValue::Number(n)) => Some(n as i64),
                _ => None,
            },
            format: text("format").and_then(|f| crate::metadata::comic::comic_format(&f)),
            artists: list("artists"),
            inkers: list("inkers"),
            colorists: list("colorists"),
            letterers: list("letterers"),
            cover_artists: list("cover_artists"),
            editors: list("editors"),
            story_arcs: list("story_arcs"),
            characters: list("characters"),
            teams: list("teams"),
            locations: list("locations"),
            age_rating: text("age_rating"),
            right_to_left: get("right_to_left") == Some(FieldValue::Bool(true)),
        })
    }
}

/// A comic's values for the comic fields, by key: #comic is always set.
pub fn comic_values(c: &ComicDetails) -> BTreeMap<String, Option<FieldValue>> {
    let list = |v: &[String]| (!v.is_empty()).then(|| FieldValue::List(v.to_vec()));
    let text = |v: &Option<String>| v.clone().map(FieldValue::Text);
    BTreeMap::from([
        (COMIC_KEY.to_owned(), Some(FieldValue::Bool(true))),
        ("issue".to_owned(), text(&c.issue)),
        ("format".to_owned(), c.format.map(|f| FieldValue::Text(f.to_owned()))),
        ("volume".to_owned(), c.volume.map(|v| FieldValue::Number(v as f64))),
        ("artists".to_owned(), list(&c.artists)),
        ("inkers".to_owned(), list(&c.inkers)),
        ("colorists".to_owned(), list(&c.colorists)),
        ("letterers".to_owned(), list(&c.letterers)),
        ("cover_artists".to_owned(), list(&c.cover_artists)),
        ("editors".to_owned(), list(&c.editors)),
        ("story_arcs".to_owned(), list(&c.story_arcs)),
        ("characters".to_owned(), list(&c.characters)),
        ("teams".to_owned(), list(&c.teams)),
        ("locations".to_owned(), list(&c.locations)),
        ("age_rating".to_owned(), text(&c.age_rating)),
        ("right_to_left".to_owned(), c.right_to_left.then_some(FieldValue::Bool(true))),
    ])
}
