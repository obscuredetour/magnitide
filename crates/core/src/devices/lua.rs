//! Just enough Lua to read and write KOReader's settings files (`return { … }`
//! tables of strings, numbers, booleans and tables), keeping everything we
//! don't change exactly as it was. Writes in the layout KOReader's own
//! `dump()` uses: `["key"] = value,` with keys sorted and 4-space indents.

use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    /// As written (so 1 stays 1 and 2.5 stays 2.5).
    Number(String),
    Str(String),
    Table(Table),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Key {
    Int(i64),
    Str(String),
}

/// A Lua table: its fields in the order read (or added).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Table(pub Vec<(Key, Value)>);

impl Table {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.iter().find(|(k, _)| matches!(k, Key::Str(s) if s == key)).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.0.iter_mut().find(|(k, _)| matches!(k, Key::Str(s) if s == key)).map(|(_, v)| v)
    }

    pub fn set(&mut self, key: &str, value: Value) {
        match self.get_mut(key) {
            Some(v) => *v = value,
            None => self.0.push((Key::Str(key.to_owned()), value)),
        }
    }

    /// The array part: values at 1, 2, 3… in order.
    pub fn array(&self) -> Vec<&Value> {
        let mut items: Vec<(i64, &Value)> =
            self.0.iter().filter_map(|(k, v)| if let Key::Int(i) = k { Some((*i, v)) } else { None }).collect();
        items.sort_by_key(|(i, _)| *i);
        items.into_iter().map(|(_, v)| v).collect()
    }

    /// Replace the array part, keeping the named fields.
    pub fn set_array(&mut self, items: Vec<Value>) {
        self.0.retain(|(k, _)| !matches!(k, Key::Int(_)));
        for (i, v) in items.into_iter().enumerate() {
            self.0.push((Key::Int(i as i64 + 1), v));
        }
    }
}

impl Value {
    pub fn as_table(&self) -> Option<&Table> {
        if let Value::Table(t) = self { Some(t) } else { None }
    }

    pub fn as_str(&self) -> Option<&str> {
        if let Value::Str(s) = self { Some(s) } else { None }
    }

    pub fn as_f64(&self) -> Option<f64> {
        if let Value::Number(n) = self { n.parse().ok() } else { None }
    }
}

/// Parse a settings file: optional comments, then `return <value>`.
pub fn parse(text: &str) -> Result<Value, String> {
    let mut p = Parser { s: text.as_bytes(), i: 0 };
    p.skip();
    if !p.eat_word("return") {
        return Err(p.error("expected `return`"));
    }
    let v = p.value()?;
    p.skip();
    if p.i != p.s.len() {
        return Err(p.error("unexpected text after the table"));
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn error(&self, what: &str) -> String {
        let line = self.s[..self.i.min(self.s.len())].iter().filter(|&&b| b == b'\n').count() + 1;
        format!("{what} (line {line})")
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    /// Skip whitespace and comments (`-- …` and `--[[ … ]]`).
    fn skip(&mut self) {
        loop {
            while self.peek().is_some_and(|b| b.is_ascii_whitespace()) {
                self.i += 1;
            }
            if self.s[self.i..].starts_with(b"--") {
                self.i += 2;
                if let Some(level) = self.long_bracket_level() {
                    let _ = self.long_string(level);
                } else {
                    while self.peek().is_some_and(|b| b != b'\n') {
                        self.i += 1;
                    }
                }
                continue;
            }
            break;
        }
    }

    fn eat(&mut self, b: u8) -> bool {
        self.skip();
        if self.peek() == Some(b) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn eat_word(&mut self, w: &str) -> bool {
        self.skip();
        let end = self.i + w.len();
        let boundary = self.s.get(end).is_none_or(|b| !(b.is_ascii_alphanumeric() || *b == b'_'));
        if self.s.get(self.i..end) == Some(w.as_bytes()) && boundary {
            self.i = end;
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        self.skip();
        match self.peek() {
            Some(b'{') => self.table().map(Value::Table),
            Some(b'"' | b'\'') => self.string().map(Value::Str),
            Some(b'[') if self.long_bracket_level().is_some() => {
                let level = self.long_bracket_level().unwrap_or(0);
                self.long_string(level).map(Value::Str)
            }
            Some(b'-' | b'.' | b'0'..=b'9') => self.number().map(Value::Number),
            _ if self.eat_word("true") => Ok(Value::Bool(true)),
            _ if self.eat_word("false") => Ok(Value::Bool(false)),
            _ if self.eat_word("nil") => Ok(Value::Nil),
            _ => Err(self.error("expected a value")),
        }
    }

    fn number(&mut self) -> Result<String, String> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        while self.peek().is_some_and(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'+' || b == b'-') {
            // A sign only right after an exponent marker.
            if matches!(self.peek(), Some(b'+' | b'-')) && !matches!(self.s[self.i - 1], b'e' | b'E' | b'p' | b'P') {
                break;
            }
            self.i += 1;
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).map_err(|_| self.error("bad number"))?;
        let valid = text.parse::<f64>().is_ok()
            || text.trim_start_matches('-').strip_prefix("0x").is_some_and(|h| i64::from_str_radix(h, 16).is_ok());
        if !valid {
            return Err(self.error(&format!("bad number {text:?}")));
        }
        Ok(text.to_owned())
    }

    fn table(&mut self) -> Result<Table, String> {
        self.i += 1; // {
        let mut t = Table::default();
        let mut next_index = 1;
        loop {
            if self.eat(b'}') {
                return Ok(t);
            }
            self.skip();
            let key = if self.peek() == Some(b'[') && self.long_bracket_level().is_none() {
                self.i += 1;
                let k = match self.value()? {
                    Value::Str(s) => Key::Str(s),
                    Value::Number(n) => Key::Int(n.parse().map_err(|_| self.error("only whole-number keys"))?),
                    _ => return Err(self.error("unsupported key")),
                };
                if !self.eat(b']') || !self.eat(b'=') {
                    return Err(self.error("expected `] =`"));
                }
                Some(k)
            } else if self.peek().is_some_and(|b| b.is_ascii_alphabetic() || b == b'_') {
                // name = value (but not true/false/nil as values)
                let start = self.i;
                while self.peek().is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_') {
                    self.i += 1;
                }
                let name = String::from_utf8_lossy(&self.s[start..self.i]).into_owned();
                if self.eat(b'=') {
                    Some(Key::Str(name))
                } else {
                    self.i = start;
                    None
                }
            } else {
                None
            };
            let value = self.value()?;
            let key = key.unwrap_or_else(|| {
                let k = Key::Int(next_index);
                next_index += 1;
                k
            });
            if value != Value::Nil {
                t.0.push((key, value));
            }
            if !(self.eat(b',') || self.eat(b';')) {
                if self.eat(b'}') {
                    return Ok(t);
                }
                return Err(self.error("expected `,` or `}`"));
            }
        }
    }

    /// `[[`, `[=[`, … at the cursor: the number of `=`.
    fn long_bracket_level(&self) -> Option<usize> {
        let rest = &self.s[self.i..];
        if rest.first() != Some(&b'[') {
            return None;
        }
        let eq = rest[1..].iter().take_while(|&&b| b == b'=').count();
        (rest.get(1 + eq) == Some(&b'[')).then_some(eq)
    }

    fn long_string(&mut self, level: usize) -> Result<String, String> {
        self.i += level + 2;
        // A newline right after the opening bracket is skipped.
        if self.peek() == Some(b'\r') {
            self.i += 1;
        }
        if self.peek() == Some(b'\n') {
            self.i += 1;
        }
        let close = format!("]{}]", "=".repeat(level));
        let rest = &self.s[self.i..];
        let end = rest
            .windows(close.len())
            .position(|w| w == close.as_bytes())
            .ok_or_else(|| self.error("unclosed long string"))?;
        let text = String::from_utf8_lossy(&rest[..end]).into_owned();
        self.i += end + close.len();
        Ok(text)
    }

    fn string(&mut self) -> Result<String, String> {
        let quote = self.s[self.i];
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let b = self.peek().ok_or_else(|| self.error("unclosed string"))?;
            self.i += 1;
            match b {
                _ if b == quote => break,
                b'\n' => return Err(self.error("newline in string")),
                b'\\' => {
                    let e = self.peek().ok_or_else(|| self.error("unclosed string"))?;
                    self.i += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'a' => out.push(7),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'v' => out.push(11),
                        b'\\' | b'"' | b'\'' => out.push(e),
                        b'\n' => out.push(b'\n'),
                        b'z' => {
                            while self.peek().is_some_and(|b| b.is_ascii_whitespace()) {
                                self.i += 1;
                            }
                        }
                        b'x' => {
                            let hex = self.s.get(self.i..self.i + 2).ok_or_else(|| self.error("bad \\x"))?;
                            let v = u8::from_str_radix(std::str::from_utf8(hex).unwrap_or(""), 16)
                                .map_err(|_| self.error("bad \\x"))?;
                            out.push(v);
                            self.i += 2;
                        }
                        b'0'..=b'9' => {
                            let mut v: u32 = u32::from(e - b'0');
                            for _ in 0..2 {
                                match self.peek() {
                                    Some(d @ b'0'..=b'9') => {
                                        v = v * 10 + u32::from(d - b'0');
                                        self.i += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push(u8::try_from(v).map_err(|_| self.error("bad \\ddd"))?);
                        }
                        b'u' => {
                            if self.peek() != Some(b'{') {
                                return Err(self.error("bad \\u"));
                            }
                            let end = self.s[self.i..]
                                .iter()
                                .position(|&b| b == b'}')
                                .ok_or_else(|| self.error("bad \\u"))?;
                            let hex = std::str::from_utf8(&self.s[self.i + 1..self.i + end]).unwrap_or("");
                            let c = u32::from_str_radix(hex, 16)
                                .ok()
                                .and_then(char::from_u32)
                                .ok_or_else(|| self.error("bad \\u"))?;
                            out.extend_from_slice(c.to_string().as_bytes());
                            self.i += end + 1;
                        }
                        _ => return Err(self.error("unknown escape")),
                    }
                }
                _ => out.push(b),
            }
        }
        String::from_utf8(out).map_err(|_| self.error("string isn't UTF-8"))
    }
}

/// Write `value` as a settings file, the way KOReader does.
pub fn write(path_comment: &str, value: &Value) -> String {
    let mut out = format!("-- {path_comment}\nreturn ");
    dump(value, 0, &mut out);
    out.push('\n');
    out
}

fn dump(value: &Value, indent: usize, out: &mut String) {
    match value {
        Value::Nil => out.push_str("nil"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(n),
        Value::Str(s) => quote(s, out),
        Value::Table(t) if t.0.is_empty() => out.push_str("{}"),
        Value::Table(t) => {
            // Like KOReader's orderedPairs: numbers first (in order), then names.
            let mut fields: Vec<&(Key, Value)> = t.0.iter().collect();
            fields.sort_by(|(a, _), (b, _)| match (a, b) {
                (Key::Int(x), Key::Int(y)) => x.cmp(y),
                (Key::Int(_), Key::Str(_)) => std::cmp::Ordering::Less,
                (Key::Str(_), Key::Int(_)) => std::cmp::Ordering::Greater,
                (Key::Str(x), Key::Str(y)) => x.cmp(y),
            });
            out.push_str("{\n");
            for (k, v) in fields {
                out.push_str(&"    ".repeat(indent + 1));
                out.push('[');
                match k {
                    Key::Int(i) => {
                        let _ = write!(out, "{i}");
                    }
                    Key::Str(s) => quote(s, out),
                }
                out.push_str("] = ");
                dump(v, indent + 1, out);
                out.push_str(",\n");
            }
            out.push_str(&"    ".repeat(indent));
            out.push('}');
        }
    }
}

/// A double-quoted Lua string that reads back as exactly `s`.
fn quote(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                let _ = write!(out, "\\{:03}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"-- ./settings/collection.lua
return {
    ["Sci-Fi"] = {
        [1] = {
            ["file"] = "/mnt/onboard/Asimov, Isaac/Foundation.kepub.epub",
            ["order"] = 1,
        },
        ["settings"] = {
            ["collate"] = "title",
            ["folders"] = {
                ["/mnt/onboard/Sci-Fi"] = {
                    ["subfolders"] = true,
                },
            },
            ["order"] = 2,
        },
    },
    ["favorites"] = {
        ["settings"] = {
            ["order"] = 1,
        },
    },
}
"#;

    #[test]
    fn round_trips_koreaders_format_exactly() {
        let v = parse(SAMPLE).unwrap();
        assert_eq!(write("./settings/collection.lua", &v), SAMPLE);
    }

    #[test]
    fn reads_other_lua_spellings() {
        let v = parse("return { a = 1, ['b'] = 'it\\'s', 'x', \"y\", [3] = -2.5e3, n = nil, t = true; z = [[long\nstring]] } -- end").unwrap();
        let t = v.as_table().unwrap();
        assert_eq!(t.get("a"), Some(&Value::Number("1".into())));
        assert_eq!(t.get("b").and_then(Value::as_str), Some("it's"));
        assert_eq!(t.array().len(), 3, "x, y and [3]");
        assert_eq!(t.get("n"), None, "nil fields are absent");
        assert_eq!(t.get("z").and_then(Value::as_str), Some("long\nstring"));
    }

    #[test]
    fn strings_survive_any_characters() {
        let tricky = "Quote \" back\\slash\nnew line\ttab é ✓ \u{1}";
        let v = Value::Table(Table(vec![(Key::Str("k".into()), Value::Str(tricky.into()))]));
        let back = parse(&write("x", &v)).unwrap();
        assert_eq!(back.as_table().unwrap().get("k").and_then(Value::as_str), Some(tricky));
        // Escapes KOReader (LuaJIT %q) writes, too.
        assert_eq!(
            parse(
                r#"return "a\
b\0\065\x41\u{e9}""#
            )
            .unwrap(),
            Value::Str("a\nb\0AA\u{e9}".into())
        );
    }

    #[test]
    fn refuses_what_it_cannot_read() {
        for bad in
            ["{}", "return {", "return { [1] = }", "return { x = function() end }", "return {} junk", "return \"open"]
        {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
