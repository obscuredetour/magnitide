//! A deliberately tiny subset of calibre's search syntax.
//!
//! Terms are ANDed. Each term is `value`, `field:value`, or either form
//! prefixed with `-` to negate. Values may be quoted: `series:"The Expanse"`.
//! Matching is a case-insensitive substring match.
//!
//! Not implemented (yet): OR/parentheses, `=` exact match, regex `~`,
//! relational operators on numbers and dates, accent folding.

use anyhow::{Result, bail};

use crate::model::Book;

#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    terms: Vec<Term>,
}

#[derive(Debug, Clone, PartialEq)]
struct Term {
    field: Option<Field>,
    needle: String,
    negate: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum Field {
    Title,
    Authors,
    Tags,
    Series,
    Shelves,
    Publisher,
    Languages,
    Identifiers,
    Formats,
    Comments,
    Custom(String),
}

impl Field {
    fn parse(name: &str) -> Result<Self> {
        Ok(match name.to_lowercase().as_str() {
            "title" => Self::Title,
            "author" | "authors" => Self::Authors,
            "tag" | "tags" => Self::Tags,
            "series" => Self::Series,
            "shelf" | "shelves" => Self::Shelves,
            "publisher" => Self::Publisher,
            "language" | "languages" => Self::Languages,
            "identifier" | "identifiers" | "id" => Self::Identifiers,
            "format" | "formats" => Self::Formats,
            "comments" => Self::Comments,
            n if n.starts_with('#') && n.len() > 1 => Self::Custom(n.to_owned()),
            other => bail!("unknown search field {other:?}"),
        })
    }
}

impl Query {
    pub fn parse(input: &str) -> Result<Self> {
        let terms = tokenize(input)?
            .into_iter()
            .map(|tok| {
                let (negate, tok) = match tok.strip_prefix('-') {
                    Some(rest) if !rest.is_empty() => (true, rest.to_owned()),
                    _ => (false, tok),
                };
                let (field, needle) = match tok.split_once(':') {
                    Some((f, v)) if !f.is_empty() && !f.contains(' ') => (Some(Field::parse(f)?), v.to_owned()),
                    _ => (None, tok),
                };
                Ok(Term { field, needle: needle.to_lowercase(), negate })
            })
            .collect::<Result<_>>()?;
        Ok(Self { terms })
    }

    pub fn matches(&self, book: &Book) -> bool {
        self.terms.iter().all(|t| t.matches(book) != t.negate)
    }
}

impl Term {
    fn matches(&self, b: &Book) -> bool {
        let hit = |s: &str| s.to_lowercase().contains(&self.needle);
        let any = |v: &[String]| v.iter().any(|s| hit(s));
        let opt = |o: &Option<String>| o.as_deref().is_some_and(hit);
        match &self.field {
            Some(Field::Title) => hit(&b.title),
            Some(Field::Authors) => any(&b.authors),
            Some(Field::Tags) => any(&b.tags),
            Some(Field::Series) => opt(&b.series) || b.more_series.iter().any(|s| hit(&s.name)),
            Some(Field::Shelves) => any(&b.shelves),
            Some(Field::Publisher) => opt(&b.publisher),
            Some(Field::Languages) => any(&b.languages),
            Some(Field::Identifiers) => b.identifiers.iter().any(|(k, v)| hit(&format!("{k}:{v}"))),
            Some(Field::Formats) => b.formats.iter().any(|f| hit(&f.format)),
            Some(Field::Comments) => opt(&b.comments),
            Some(Field::Custom(k)) => b.custom.get(k).is_some_and(|v| hit(&v.to_string())),
            None => {
                hit(&b.title)
                    || any(&b.authors)
                    || any(&b.tags)
                    || opt(&b.series)
                    || b.more_series.iter().any(|s| hit(&s.name))
                    || opt(&b.publisher)
                    || b.identifiers.values().any(|v| hit(v))
                    || b.custom.values().any(|v| hit(&v.to_string()))
            }
        }
    }
}

/// Split on whitespace, keeping double-quoted sections together and
/// dropping the quotes: `series:"The Expanse" -tag:read` → 2 tokens.
fn tokenize(input: &str) -> Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut quoted = false;
    for c in input.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                quoted = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if !cur.is_empty() || quoted {
                    tokens.push(std::mem::take(&mut cur));
                }
                quoted = false;
            }
            c => cur.push(c),
        }
    }
    if in_quotes {
        bail!("unterminated quote in search");
    }
    if !cur.is_empty() || quoted {
        tokens.push(cur);
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_quotes() {
        assert_eq!(
            tokenize(r#"series:"The Expanse" -tag:read  dune"#).unwrap(),
            vec!["series:The Expanse", "-tag:read", "dune"]
        );
        assert!(tokenize(r#"title:"oops"#).is_err());
    }

    #[test]
    fn parses_fields_and_negation() {
        let q = Query::parse(r#"author:"Le Guin" -#read:yes"#).unwrap();
        assert_eq!(q.terms.len(), 2);
        assert_eq!(q.terms[1].field, Some(Field::Custom("#read".into())));
        assert!(q.terms[1].negate);
        assert!(Query::parse("bogus:x").is_err());
    }
}
