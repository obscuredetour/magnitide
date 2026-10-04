use std::sync::LazyLock;

/// Built once: an ammonia Builder allocates its tag and attribute sets.
static CLEANER: LazyLock<ammonia::Builder<'static>> = LazyLock::new(|| {
    let mut b = ammonia::Builder::default();
    b.rm_tags(["img"]);
    b
});

/// Book descriptions are HTML from arbitrary sources (publishers, metadata
/// downloads, other apps). Keep formatting; drop anything that could run
/// script or load remote content.
pub fn sanitize(html: &str) -> String {
    CLEANER.clean(html).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_active_content_keeps_formatting() {
        let clean = sanitize(r#"<p onclick="x()"><b>Bold</b><script>alert(1)</script><img src="http://x/y.png"></p>"#);
        assert_eq!(clean, "<p><b>Bold</b></p>");
    }
}
