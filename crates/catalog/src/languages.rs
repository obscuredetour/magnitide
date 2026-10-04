//! Language codes. Libraries store ISO 639-2 ("eng"), as calibre does.

/// ISO 639-1 codes and the ISO 639-2 codes calibre (and Magnitide) store.
const LANGUAGES: &[(&str, &str)] = &[
    ("en", "eng"),
    ("de", "deu"),
    ("fr", "fra"),
    ("es", "spa"),
    ("it", "ita"),
    ("nl", "nld"),
    ("pt", "por"),
    ("ru", "rus"),
    ("ja", "jpn"),
    ("zh", "zho"),
    ("ko", "kor"),
    ("sv", "swe"),
    ("no", "nor"),
    ("nb", "nor"),
    ("da", "dan"),
    ("fi", "fin"),
    ("pl", "pol"),
    ("cs", "ces"),
    ("tr", "tur"),
    ("el", "ell"),
    ("he", "heb"),
    ("ar", "ara"),
    ("hi", "hin"),
    ("uk", "ukr"),
    ("hu", "hun"),
    ("ro", "ron"),
    ("ca", "cat"),
    ("la", "lat"),
];

/// ISO 639-1 ("en", "en-GB") to the ISO 639-2 codes calibre uses ("eng").
/// Already three-letter codes pass through; unknown ones are kept as given.
pub fn language_code(code: &str) -> String {
    let base = code.trim().split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
    if let Some((_, three)) = LANGUAGES.iter().find(|(two, _)| *two == base) {
        return (*three).to_owned();
    }
    // MARC/bibliographic codes (Open Library, library catalogues) to ISO 639-2/T.
    MARC.iter().find(|(b, _)| *b == base).map_or(base, |(_, t)| (*t).to_owned())
}

const MARC: &[(&str, &str)] = &[
    ("fre", "fra"),
    ("ger", "deu"),
    ("dut", "nld"),
    ("chi", "zho"),
    ("gre", "ell"),
    ("cze", "ces"),
    ("rum", "ron"),
    ("per", "fas"),
    ("arm", "hye"),
    ("baq", "eus"),
    ("bur", "mya"),
    ("geo", "kat"),
    ("ice", "isl"),
    ("mac", "mkd"),
    ("mao", "mri"),
    ("may", "msa"),
    ("slo", "slk"),
    ("tib", "bod"),
    ("wel", "cym"),
    ("alb", "sqi"),
];

/// The reverse, for writing into files: EPUB wants BCP 47 tags, which use the
/// two-letter code where one exists ("eng" → "en").
pub fn language_tag(code: &str) -> String {
    let code = code.trim().to_ascii_lowercase();
    LANGUAGES.iter().find(|(_, three)| *three == code).map_or(code, |(two, _)| (*two).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_language_codes() {
        assert_eq!(language_code("en-GB"), "eng");
        assert_eq!(language_code("de"), "deu");
        assert_eq!(language_code("eng"), "eng");
        assert_eq!(language_code("tlh"), "tlh");
        assert_eq!(language_code("ger"), "deu");
        assert_eq!(language_tag("eng"), "en");
        assert_eq!(language_tag("nor"), "no");
        assert_eq!(language_tag("tlh"), "tlh");
    }
}
