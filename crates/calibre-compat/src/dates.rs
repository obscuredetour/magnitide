//! calibre stores timestamps as text, e.g. `2011-09-05 21:06:00.055300+00:00`.
//!
//! calibre's "undefined" date is 0101-01-01; any year <= 101 is treated as
//! "no value" (see `calibre.utils.date.is_date_undefined`).

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};

const UNDEFINED_YEAR_MAX: i32 = 101;

pub fn parse(raw: &str) -> Option<DateTime<Utc>> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    let dt = DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f%:z")
        .or_else(|_| DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f%:z"))
        .map(|d| d.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            // Naive timestamps (no offset) are UTC in calibre.
            NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f")
                .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f"))
                .ok()
                .or_else(|| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()?.and_hms_opt(0, 0, 0))
                .map(|n| Utc.from_utc_datetime(&n))
        })?;
    use chrono::Datelike;
    (dt.year() > UNDEFINED_YEAR_MAX).then_some(dt)
}

/// Format the way calibre writes timestamps.
pub fn format(dt: &DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S%.6f%:z").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_calibre_formats() {
        let d = parse("2011-09-05 21:06:00.055300+00:00").unwrap();
        assert_eq!(d.to_rfc3339(), "2011-09-05T21:06:00.055300+00:00");
        assert!(parse("2011-09-05T21:06:00+02:00").is_some());
        assert!(parse("2011-09-05 21:06:00").is_some());
        assert!(parse("2011-09-05").is_some());
    }

    #[test]
    fn undefined_date_is_none() {
        assert_eq!(parse("0101-01-01 00:00:00+00:00"), None);
        assert_eq!(parse(""), None);
        assert_eq!(parse("garbage"), None);
    }
}
