//! Custom fields: ones people define themselves ("Pages", "Read", "Shelf"),
//! as calibre's custom columns, which import maps here.
//!
//! A field has a key (its lookup name: `#pages` in search), a display name, a
//! kind (fixed once made) and, for choices, the allowed values. Values are
//! edited with the rest of a book ([`crate::BookEdit::custom`]) and arrive as
//! [`FieldValue`]s, checked against the field's kind before anything is saved.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use magnitide_catalog::{CustomColumn, CustomValue, Datatype};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::catalog::custom_columns;
use crate::error::{Error, Result};
use crate::import::calibre::insert_custom_value;
use crate::library::{BookId, Library, now};

/// What a custom field holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum FieldKind {
    /// Short text; with `multiple`, several values (like tags).
    Text,
    /// Formatted text (HTML, sanitized), like a description.
    LongText,
    /// A whole number ("Pages").
    Integer,
    /// A decimal number.
    Number,
    /// Yes or no.
    Bool,
    Date,
    /// 1–10 half stars.
    Rating,
    /// One of a fixed list (`choices`).
    Choice,
    /// A series name with a number.
    Series,
}

impl FieldKind {
    /// The name stored in `custom_fields.kind`.
    fn db_name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::LongText => "long_text",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::Bool => "bool",
            Self::Date => "date",
            Self::Rating => "rating",
            Self::Choice => "choice",
            Self::Series => "series",
        }
    }

    fn of(datatype: &Datatype) -> Option<Self> {
        Some(match datatype {
            Datatype::Text => Self::Text,
            Datatype::Comments => Self::LongText,
            Datatype::Int => Self::Integer,
            Datatype::Float => Self::Number,
            Datatype::Bool => Self::Bool,
            Datatype::Datetime => Self::Date,
            Datatype::Rating => Self::Rating,
            Datatype::Enumeration => Self::Choice,
            Datatype::Series => Self::Series,
            Datatype::Composite | Datatype::Unknown(_) => return None,
        })
    }

    fn datatype(self) -> Datatype {
        match self {
            Self::Text => Datatype::Text,
            Self::LongText => Datatype::Comments,
            Self::Integer => Datatype::Int,
            Self::Number => Datatype::Float,
            Self::Bool => Datatype::Bool,
            Self::Date => Datatype::Datetime,
            Self::Rating => Datatype::Rating,
            Self::Choice => Datatype::Enumeration,
            Self::Series => Datatype::Series,
        }
    }
}

/// A custom field, as Settings shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct CustomField {
    pub id: i64,
    /// Lookup name without the '#': "pages" (searched as `#pages:`).
    pub key: String,
    pub name: String,
    pub kind: FieldKind,
    /// Text only: several values per book.
    pub multiple: bool,
    /// Choice only: the allowed values, in order.
    pub choices: Vec<String>,
}

/// A field to make.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct NewField {
    pub name: String,
    /// Made from the name when empty ("Date Read" → "date_read").
    #[serde(default)]
    pub key: String,
    pub kind: Option<FieldKind>,
    #[serde(default)]
    pub multiple: bool,
    #[serde(default)]
    pub choices: Vec<String>,
}

/// A custom value as the edit form sends it; read according to the field's kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(untagged)]
pub enum FieldValue {
    Bool(bool),
    Number(f64),
    /// Text, a choice, or a date ("YYYY-MM-DD").
    Text(String),
    /// Several text values.
    List(Vec<String>),
    Series {
        name: String,
        index: f64,
    },
}

impl CustomField {
    /// A catalog's column as a field (any library's, calibre's included).
    /// None for kinds that hold no values of their own (calibre's computed columns).
    pub fn from_column(c: &CustomColumn) -> Option<Self> {
        let kind = FieldKind::of(&c.datatype)?;
        // Ours keep the choices at the top; imported ones in calibre's settings.
        let choices = c
            .display
            .get("choices")
            .or_else(|| c.display.get("enum_values"))
            .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok())
            .unwrap_or_default();
        Some(Self { id: c.id, key: c.label.clone(), name: c.name.clone(), kind, multiple: c.is_multiple, choices })
    }
}

impl Library {
    /// Every custom field, in the order they were made.
    pub fn custom_fields(&self) -> Result<Vec<CustomField>> {
        Ok(custom_columns(&self.conn)?.iter().filter_map(CustomField::from_column).collect())
    }

    /// Make a custom field.
    pub fn add_custom_field(&mut self, new: &NewField) -> Result<CustomField> {
        let name = new.name.trim();
        if name.is_empty() {
            return Err(Error::InvalidName("a field needs a name".into()));
        }
        let kind = new.kind.ok_or_else(|| Error::InvalidName("choose what kind of field it is".into()))?;
        let key = match new.key.trim() {
            "" => key_from(name),
            k => k.trim_start_matches('#').to_lowercase(),
        };
        if key.is_empty() || !key.starts_with(|c: char| c.is_ascii_lowercase()) || !key.chars().all(valid_key_char) {
            return Err(Error::InvalidName(format!(
                "“{key}” can't be a lookup name: use letters, digits and _, starting with a letter"
            )));
        }
        let taken: Option<i64> =
            self.conn.query_row("SELECT id FROM custom_fields WHERE key = ?", [&key], |r| r.get(0)).optional()?;
        if taken.is_some() {
            return Err(Error::InvalidName(format!("there's already a field #{key}")));
        }
        let choices = clean_choices(&new.choices);
        if kind == FieldKind::Choice && choices.is_empty() {
            return Err(Error::InvalidName("a choice field needs at least one choice".into()));
        }
        let config =
            if kind == FieldKind::Choice { serde_json::json!({ "choices": choices }) } else { serde_json::json!({}) };
        self.conn.execute(
            "INSERT INTO custom_fields (key, name, kind, multiple, config) VALUES (?, ?, ?, ?, ?)",
            params![key, name, kind.db_name(), new.multiple && kind == FieldKind::Text, config.to_string()],
        )?;
        let id = self.conn.last_insert_rowid();
        self.custom_field(id)
    }

    /// Rename a field and, for a choice field, change its choices. Books keep
    /// their values even if a choice is taken out.
    pub fn update_custom_field(&mut self, id: i64, name: &str, choices: Option<&[String]>) -> Result<CustomField> {
        let field = self.custom_field(id)?;
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::InvalidName("a field needs a name".into()));
        }
        let tx = self.conn.transaction()?;
        tx.execute("UPDATE custom_fields SET name = ? WHERE id = ?", params![name, id])?;
        if let (FieldKind::Choice, Some(choices)) = (field.kind, choices) {
            let choices = clean_choices(choices);
            if choices.is_empty() {
                return Err(Error::InvalidName("a choice field needs at least one choice".into()));
            }
            let config: String = tx.query_row("SELECT config FROM custom_fields WHERE id = ?", [id], |r| r.get(0))?;
            let mut config: serde_json::Value = serde_json::from_str(&config).unwrap_or_else(|_| serde_json::json!({}));
            if !config.is_object() {
                config = serde_json::json!({});
            }
            config["choices"] = serde_json::json!(choices);
            tx.execute("UPDATE custom_fields SET config = ? WHERE id = ?", params![config.to_string(), id])?;
        }
        tx.commit()?;
        self.custom_field(id)
    }

    /// Delete a field and every book's value for it.
    pub fn delete_custom_field(&mut self, id: i64) -> Result<()> {
        self.custom_field(id)?;
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM custom_values WHERE field_id = ?", [id])?;
        tx.execute("DELETE FROM custom_fields WHERE id = ?", [id])?;
        tx.commit()?;
        Ok(())
    }

    fn custom_field(&self, id: i64) -> Result<CustomField> {
        self.custom_fields()?
            .into_iter()
            .find(|f| f.id == id)
            .ok_or_else(|| Error::InvalidName(format!("no custom field {id}")))
    }

    /// A book's custom values, by key, as the edit form shows them.
    pub(crate) fn custom_edit_values(&self, id: BookId) -> Result<BTreeMap<String, Option<FieldValue>>> {
        let catalog_book = self.load_catalog_book_custom(id)?;
        let mut out = BTreeMap::new();
        for field in self.custom_fields()? {
            let value = catalog_book.get(&format!("#{}", field.key)).map(to_field_value);
            out.insert(field.key, value);
        }
        Ok(out)
    }

    fn load_catalog_book_custom(&self, id: BookId) -> Result<BTreeMap<String, CustomValue>> {
        // One book's values, read the way the catalog reads them.
        let fields: BTreeMap<i64, (String, Datatype, bool)> = custom_columns(&self.conn)?
            .into_iter()
            .map(|c| (c.id, (c.lookup_name(), c.datatype, c.is_multiple)))
            .collect();
        let mut stmt = self.conn.prepare_cached(
            "SELECT field_id, text_value, num_value FROM custom_values WHERE book_id = ? ORDER BY field_id, position",
        )?;
        let mut rows = stmt.query([id])?;
        let mut out: BTreeMap<String, CustomValue> = BTreeMap::new();
        while let Some(r) = rows.next()? {
            let Some((key, datatype, multiple)) = fields.get(&r.get::<_, i64>(0)?) else { continue };
            let text: Option<String> = r.get(1)?;
            let num: Option<f64> = r.get(2)?;
            let value = match datatype {
                Datatype::Text if *multiple => {
                    if let (Some(t), CustomValue::TextList(list)) =
                        (text, out.entry(key.clone()).or_insert(CustomValue::TextList(Vec::new())))
                    {
                        list.push(t);
                    }
                    continue;
                }
                Datatype::Series => text.map(|name| CustomValue::Series { name, index: num.unwrap_or(1.0) }),
                Datatype::Int => num.map(|n| CustomValue::Int(n as i64)),
                Datatype::Float => num.map(CustomValue::Float),
                Datatype::Bool => num.map(|n| CustomValue::Bool(n != 0.0)),
                Datatype::Rating => num.map(|n| CustomValue::Rating(n as u8)),
                Datatype::Datetime => text.and_then(|t| t.parse().ok()).map(CustomValue::Datetime),
                _ => text.map(CustomValue::Text),
            };
            if let Some(v) = value {
                out.insert(key.clone(), v);
            }
        }
        Ok(out)
    }

    /// Check custom values from an edit against their fields: (field id,
    /// type, value to store or None to clear). Unknown keys are refused.
    pub(crate) fn clean_custom(
        &self,
        values: &BTreeMap<String, Option<FieldValue>>,
    ) -> Result<Vec<(i64, Datatype, Option<CustomValue>)>> {
        if values.is_empty() {
            return Ok(Vec::new());
        }
        let fields = self.custom_fields()?;
        values
            .iter()
            .map(|(key, value)| {
                let key = key.trim_start_matches('#');
                let field = fields
                    .iter()
                    .find(|f| f.key.eq_ignore_ascii_case(key))
                    .ok_or_else(|| Error::InvalidBook(format!("there's no field #{key}")))?;
                let value = match value {
                    Some(v) => to_custom_value(field, v)?,
                    None => None,
                };
                Ok((field.id, field.kind.datatype(), value))
            })
            .collect()
    }
}

/// Replace a book's values for these fields (within an edit's transaction).
pub(crate) fn write_custom(
    tx: &Transaction<'_>,
    id: BookId,
    values: &[(i64, Datatype, Option<CustomValue>)],
) -> Result<()> {
    for (field, datatype, value) in values {
        tx.prepare_cached("DELETE FROM custom_values WHERE book_id = ? AND field_id = ?")?
            .execute(params![id, field])?;
        if let Some(value) = value {
            insert_custom_value(tx, id, *field, datatype, value)?;
        }
    }
    if !values.is_empty() {
        tx.prepare_cached("UPDATE books SET modified_at = ? WHERE id = ?")?.execute(params![now(), id])?;
    }
    Ok(())
}

/// What the edit form gets for a stored value.
fn to_field_value(v: &CustomValue) -> FieldValue {
    match v {
        CustomValue::Text(s) => FieldValue::Text(s.clone()),
        CustomValue::TextList(l) => FieldValue::List(l.clone()),
        CustomValue::Series { name, index } => FieldValue::Series { name: name.clone(), index: *index },
        CustomValue::Int(i) => FieldValue::Number(*i as f64),
        CustomValue::Float(f) => FieldValue::Number(*f),
        CustomValue::Bool(b) => FieldValue::Bool(*b),
        CustomValue::Rating(r) => FieldValue::Number(f64::from(*r)),
        CustomValue::Datetime(d) => FieldValue::Text(d.date_naive().to_string()),
    }
}

/// An edit's value as stored, checked against the field. None: empty (cleared).
fn to_custom_value(field: &CustomField, v: &FieldValue) -> Result<Option<CustomValue>> {
    let bad = |what: &str| Error::InvalidBook(format!("{}: {what}", field.name));
    let text = |v: &FieldValue| -> Option<String> {
        match v {
            FieldValue::Text(s) => Some(s.trim().to_owned()).filter(|s| !s.is_empty()),
            FieldValue::Number(n) => Some(n.to_string()),
            _ => None,
        }
    };
    let number = |v: &FieldValue| -> Result<Option<f64>> {
        match v {
            FieldValue::Number(n) if n.is_finite() => Ok(Some(*n)),
            FieldValue::Number(_) => Err(bad("not a number")),
            FieldValue::Text(s) if s.trim().is_empty() => Ok(None),
            FieldValue::Text(s) => {
                s.trim().parse().map(Some).map_err(|_| bad(&format!("“{}” isn't a number", s.trim())))
            }
            _ => Err(bad("expected a number")),
        }
    };
    Ok(match field.kind {
        FieldKind::Text if field.multiple => {
            let items: Vec<String> = match v {
                FieldValue::List(l) => l.clone(),
                FieldValue::Text(s) => s.split(',').map(str::to_owned).collect(),
                _ => return Err(bad("expected text")),
            };
            let mut out: Vec<String> = Vec::new();
            for item in items.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
                if !out.iter().any(|o| o.eq_ignore_ascii_case(item)) {
                    out.push(item.to_owned());
                }
            }
            (!out.is_empty()).then_some(CustomValue::TextList(out))
        }
        FieldKind::Text | FieldKind::LongText => text(v).map(CustomValue::Text),
        FieldKind::Integer => match number(v)? {
            Some(n) if n.fract() != 0.0 => return Err(bad("expected a whole number")),
            Some(n) => Some(CustomValue::Int(n as i64)),
            None => None,
        },
        FieldKind::Number => number(v)?.map(CustomValue::Float),
        FieldKind::Bool => match v {
            FieldValue::Bool(b) => Some(CustomValue::Bool(*b)),
            FieldValue::Text(s) if s.trim().is_empty() => None,
            _ => return Err(bad("expected yes or no")),
        },
        FieldKind::Date => match text(v) {
            Some(d) => Some(CustomValue::Datetime(
                d.parse::<NaiveDate>()
                    .map_err(|_| bad(&format!("“{d}” isn't a date (expected YYYY-MM-DD)")))?
                    .and_hms_opt(0, 0, 0)
                    .expect("midnight exists")
                    .and_utc(),
            )),
            None => None,
        },
        FieldKind::Rating => match number(v)? {
            Some(0.0) => None,
            Some(n) if n.fract() == 0.0 && (1.0..=10.0).contains(&n) => Some(CustomValue::Rating(n as u8)),
            Some(n) => return Err(bad(&format!("rating {n} is outside 1–10"))),
            None => None,
        },
        FieldKind::Choice => match text(v) {
            Some(c) => {
                let choice = field.choices.iter().find(|x| x.eq_ignore_ascii_case(&c));
                match choice {
                    Some(choice) => Some(CustomValue::Text(choice.clone())),
                    None if field.choices.is_empty() => Some(CustomValue::Text(c)),
                    None => return Err(bad(&format!("“{c}” isn't one of its choices"))),
                }
            }
            None => None,
        },
        FieldKind::Series => match v {
            FieldValue::Series { name, index } => {
                let name = name.trim();
                if !index.is_finite() || *index < 0.0 {
                    return Err(bad("the number must be zero or more"));
                }
                (!name.is_empty()).then(|| CustomValue::Series { name: name.to_owned(), index: *index })
            }
            FieldValue::Text(s) if s.trim().is_empty() => None,
            FieldValue::Text(s) => Some(CustomValue::Series { name: s.trim().to_owned(), index: 1.0 }),
            _ => return Err(bad("expected a series")),
        },
    })
}

fn valid_key_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'
}

/// "Date Read" → "date_read"; "Pages (print)" → "pages_print".
fn key_from(name: &str) -> String {
    let mut out = String::new();
    for c in deunicode::deunicode(name).chars().flat_map(char::to_lowercase) {
        if valid_key_char(c) && c != '_' {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_end_matches('_').to_owned();
    // A lookup name starts with a letter.
    if out.starts_with(|c: char| c.is_ascii_digit()) { format!("f_{out}") } else { out }
}

fn clean_choices(choices: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in choices.iter().map(|c| c.trim()).filter(|c| !c.is_empty()) {
        if !out.iter().any(|o| o.eq_ignore_ascii_case(c)) {
            out.push(c.to_owned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewBook;

    fn field(name: &str, kind: FieldKind) -> NewField {
        NewField { name: name.into(), kind: Some(kind), ..Default::default() }
    }

    fn library() -> (tempfile::TempDir, Library, BookId) {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = Library::create(dir.path(), "Test").unwrap();
        let id = lib.add_book(&NewBook { title: "Dune".into(), ..Default::default() }).unwrap();
        (dir, lib, id)
    }

    #[test]
    fn makes_renames_and_deletes_fields() {
        let (_dir, mut lib, id) = library();
        let pages = lib.add_custom_field(&field("Pages", FieldKind::Integer)).unwrap();
        assert_eq!((pages.key.as_str(), pages.name.as_str(), pages.kind), ("pages", "Pages", FieldKind::Integer));
        let read = lib.add_custom_field(&field("Date Read", FieldKind::Date)).unwrap();
        assert_eq!(read.key, "date_read");
        let err = lib.add_custom_field(&field("pages", FieldKind::Text)).unwrap_err().to_string();
        assert!(err.contains("already a field #pages"), "{err}");
        let err = lib.add_custom_field(&NewField { key: "9lives".into(), ..field("X", FieldKind::Text) }).unwrap_err();
        assert!(err.to_string().contains("lookup name"), "{err}");
        assert!(lib.add_custom_field(&field("Shelf", FieldKind::Choice)).is_err(), "a choice needs choices");
        let shelf = lib
            .add_custom_field(&NewField {
                choices: vec!["To Read".into(), " Read ".into(), "read".into()],
                ..field("Shelf", FieldKind::Choice)
            })
            .unwrap();
        assert_eq!(shelf.choices, ["To Read", "Read"]);

        let renamed = lib.update_custom_field(pages.id, "Page Count", None).unwrap();
        assert_eq!((renamed.key.as_str(), renamed.name.as_str()), ("pages", "Page Count"), "the key stays");
        let more = lib.update_custom_field(shelf.id, "Shelf", Some(&["To Read".into(), "Reading".into()])).unwrap();
        assert_eq!(more.choices, ["To Read", "Reading"]);

        let mut e = lib.book_edit(id).unwrap();
        e.custom.insert("pages".into(), Some(FieldValue::Number(412.0)));
        lib.update_book(id, &e).unwrap();
        lib.delete_custom_field(pages.id).unwrap();
        let keys: Vec<String> = lib.custom_fields().unwrap().into_iter().map(|f| f.key).collect();
        assert_eq!(keys, ["date_read", "shelf"]);
        let values: i64 = lib.conn.query_row("SELECT count(*) FROM custom_values", [], |r| r.get(0)).unwrap();
        assert_eq!(values, 0, "its values went with it");
    }

    #[test]
    fn values_are_edited_with_the_book() {
        let (_dir, mut lib, id) = library();
        for (name, kind) in [
            ("Pages", FieldKind::Integer),
            ("Score", FieldKind::Number),
            ("Read", FieldKind::Bool),
            ("Finished", FieldKind::Date),
            ("Mine", FieldKind::Rating),
            ("Notes", FieldKind::LongText),
            ("Order", FieldKind::Series),
        ] {
            lib.add_custom_field(&field(name, kind)).unwrap();
        }
        lib.add_custom_field(&NewField { multiple: true, ..field("Genre", FieldKind::Text) }).unwrap();
        lib.add_custom_field(&NewField {
            choices: vec!["To Read".into(), "Read".into()],
            ..field("Shelf", FieldKind::Choice)
        })
        .unwrap();

        let mut e = lib.book_edit(id).unwrap();
        assert_eq!(e.custom.len(), 9);
        assert!(e.custom.values().all(Option::is_none), "nothing set yet");
        let set = |k: &str, v: FieldValue| (k.to_owned(), Some(v));
        e.custom.extend([
            set("pages", FieldValue::Number(412.0)),
            set("score", FieldValue::Text("4.5".into())),
            set("read", FieldValue::Bool(true)),
            set("finished", FieldValue::Text("2024-06-01".into())),
            set("mine", FieldValue::Number(8.0)),
            set("notes", FieldValue::Text("<p>Re<script>x</script>read</p>".into())),
            set("order", FieldValue::Series { name: "Dune".into(), index: 1.0 }),
            set(
                "genre",
                FieldValue::List(vec![
                    "Science Fiction".into(),
                    " ".into(),
                    "science fiction".into(),
                    "Classic".into(),
                ]),
            ),
            set("shelf", FieldValue::Text("read".into())),
        ]);
        lib.update_book(id, &e).unwrap();

        let back = lib.book_edit(id).unwrap().custom;
        assert_eq!(back["pages"], Some(FieldValue::Number(412.0)));
        assert_eq!(back["score"], Some(FieldValue::Number(4.5)));
        assert_eq!(back["read"], Some(FieldValue::Bool(true)));
        assert_eq!(back["finished"], Some(FieldValue::Text("2024-06-01".into())));
        assert_eq!(back["mine"], Some(FieldValue::Number(8.0)));
        assert!(!matches!(&back["notes"], Some(FieldValue::Text(t)) if t.contains("script")), "sanitized");
        assert_eq!(back["genre"], Some(FieldValue::List(vec!["Science Fiction".into(), "Classic".into()])));
        assert_eq!(back["shelf"], Some(FieldValue::Text("Read".into())), "the choice as spelled");
        let cat = lib.load_catalog().unwrap();
        assert_eq!(cat.get(id).unwrap().custom.get("#pages"), Some(&CustomValue::Int(412)));

        // Clearing, and bad values (nothing saved when one is wrong).
        let mut e = lib.book_edit(id).unwrap();
        e.custom.insert("pages".into(), None);
        e.custom.insert("mine".into(), Some(FieldValue::Number(0.0)));
        lib.update_book(id, &e).unwrap();
        let back = lib.book_edit(id).unwrap().custom;
        assert_eq!((&back["pages"], &back["mine"]), (&None, &None));
        for (key, bad) in [
            ("pages", FieldValue::Number(1.5)),
            ("finished", FieldValue::Text("June".into())),
            ("mine", FieldValue::Number(11.0)),
            ("shelf", FieldValue::Text("Lost".into())),
            ("read", FieldValue::Number(1.0)),
        ] {
            let mut e = lib.book_edit(id).unwrap();
            e.title = "Changed".into();
            e.custom.insert(key.into(), Some(bad));
            let err = lib.update_book(id, &e).unwrap_err();
            assert!(matches!(err, Error::InvalidBook(_)), "{key}: {err}");
            assert_eq!(lib.book(id).unwrap().title, "Dune", "{key}: nothing saved");
        }
        let mut e = lib.book_edit(id).unwrap();
        e.custom.insert("nope".into(), Some(FieldValue::Bool(true)));
        assert!(lib.update_book(id, &e).is_err());
    }

    #[test]
    fn keys_from_names() {
        assert_eq!(key_from("Date Read"), "date_read");
        assert_eq!(key_from("  Pages (print) "), "pages_print");
        assert_eq!(key_from("Lü's Notes"), "lu_s_notes");
        assert_eq!(key_from("2nd Author"), "f_2nd_author");
    }
}
