//! Small helpers: URLs, escaping, MIME types, paging.

use magnitide_catalog::Book;

/// Decode %XX escapes (and '+' as a space, for form values).
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                        continue;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b'+' => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn parse_query(q: &str) -> Vec<(String, String)> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect()
}

/// For a query parameter value.
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Escape for HTML text and attributes, and for XML.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            // Control characters aren't allowed in XML.
            c if (c as u32) < 0x20 && !matches!(c, '\n' | '\t' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

/// A format's MIME type.
pub fn mime(format: &str) -> &'static str {
    match format {
        "EPUB" | "KEPUB" => "application/epub+zip",
        "PDF" => "application/pdf",
        "MOBI" | "AZW" => "application/x-mobipocket-ebook",
        "AZW3" => "application/vnd.amazon.ebook",
        "CBZ" => "application/vnd.comicbook+zip",
        "CBR" => "application/vnd.comicbook-rar",
        "FB2" => "application/x-fictionbook+xml",
        "TXT" => "text/plain",
        "RTF" => "application/rtf",
        "DJVU" => "image/vnd.djvu",
        _ => "application/octet-stream",
    }
}

/// "Title - Author.epub"; a KEPUB is ".kepub.epub", which Kobos need to treat it as one.
pub fn download_name(book: &Book, format: &str) -> String {
    let author = book.authors.first().map_or("Unknown", String::as_str);
    let stem: String = format!("{} - {author}", book.title)
        .chars()
        .map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c })
        .take(150)
        .collect();
    let ext = if format == "KEPUB" { "kepub.epub".to_owned() } else { format.to_lowercase() };
    format!("{}.{ext}", stem.trim())
}

/// A Content-Disposition header: a plain ASCII name for old clients, and the
/// real (UTF-8) one for the rest.
pub fn attachment(name: &str) -> String {
    let ascii: String = name.chars().map(|c| if c.is_ascii() && c != '"' && c != '\\' { c } else { '_' }).collect();
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{}", percent_encode(name))
}

pub const PAGE_SIZE: usize = 25;

/// Page `page` (1-based) of `items`, and whether there are more after it.
pub fn page<T: Clone>(items: &[T], page: usize, size: usize) -> (Vec<T>, bool) {
    let start = (page.max(1) - 1) * size;
    let slice = items.iter().skip(start).take(size).cloned().collect();
    (slice, items.len() > start + size)
}

/// Text from HTML (a description, for OPDS summaries): tags dropped, entities for &, <, > decoded.
pub fn text_of(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(percent_decode("To%20Read+now%2F%E2%9C%93"), "To Read now/✓");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(
            parse_query("q=shelf%3A%22To+Read%22&page=2"),
            [("q".into(), "shelf:\"To Read\"".into()), ("page".into(), "2".into())]
        );
        assert_eq!(percent_encode("To Read/✓"), "To%20Read%2F%E2%9C%93");
    }

    #[test]
    fn escaping_and_text() {
        assert_eq!(escape("<b>\"Tom & Jerry's\"</b>\u{1}"), "&lt;b&gt;&quot;Tom &amp; Jerry&#39;s&quot;&lt;/b&gt;");
        assert_eq!(text_of("<p>One &amp; <i>two</i></p><p>three</p>"), "One & two three");
        assert_eq!(
            attachment("Café: A/B.epub"),
            "attachment; filename=\"Caf_: A/B.epub\"; filename*=UTF-8''Caf%C3%A9%3A%20A%2FB.epub"
        );
    }

    #[test]
    fn paging() {
        let items: Vec<usize> = (0..60).collect();
        assert_eq!(page(&items, 1, 25), ((0..25).collect(), true));
        assert_eq!(page(&items, 3, 25), ((50..60).collect(), false));
        assert_eq!(page(&items, 0, 25).0.len(), 25, "page 0 means 1");
    }
}
