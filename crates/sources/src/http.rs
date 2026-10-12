//! The one HTTP client: Magnitide's user agent, timeouts, size limits and a
//! polite gap between requests to the same host.

use std::collections::HashMap;
use std::fmt;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use ureq::Agent;

const USER_AGENT: &str =
    concat!("Magnitide/", env!("CARGO_PKG_VERSION"), " (+https://github.com/obscuredetour/magnitide)");

/// The least time between two requests to a host: a second for search APIs,
/// less for cover images (a results list shows several), and three for
/// Metron, which allows 20 a minute.
fn gap(host: &str) -> Duration {
    if host.starts_with("covers.") || host == "books.google.com" || host == "static.metron.cloud" {
        Duration::from_millis(150)
    } else if host == "metron.cloud" {
        Duration::from_millis(3100)
    } else {
        Duration::from_millis(1000)
    }
}

const MAX_JSON: u64 = 5 << 20;

static AGENT: LazyLock<Agent> = LazyLock::new(|| {
    Agent::config_builder()
        .user_agent(USER_AGENT)
        .timeout_global(Some(Duration::from_secs(15)))
        .https_only(true)
        .build()
        .into()
});

static LAST: LazyLock<Mutex<HashMap<String, Instant>>> = LazyLock::new(Default::default);

/// A failed request, worded for the user.
#[derive(Debug, Clone, PartialEq)]
pub struct Error(String);

impl Error {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<ureq::Error> for Error {
    fn from(e: ureq::Error) -> Self {
        Self(match e {
            ureq::Error::StatusCode(429) => "too many requests; try again in a minute".into(),
            ureq::Error::StatusCode(400 | 401 | 403) => "the request was refused (check the API key or login)".into(),
            ureq::Error::StatusCode(404) => "not found".into(),
            ureq::Error::StatusCode(code) => format!("the server answered with an error ({code})"),
            ureq::Error::Timeout(_) => "no answer in time".into(),
            ureq::Error::BodyExceedsLimit(_) => "the answer was too large".into(),
            ureq::Error::HostNotFound | ureq::Error::Io(_) | ureq::Error::ConnectionFailed => {
                "couldn't connect (are you online?)".into()
            }
            other => other.to_string(),
        })
    }
}

/// Wait until this host may be asked again.
fn throttle(url: &str) {
    let host = url.split('/').nth(2).unwrap_or_default().to_owned();
    let wait = {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let next = last.get(&host).map_or(now, |t| (*t + gap(&host)).max(now));
        last.insert(host, next);
        next - now
    };
    std::thread::sleep(wait);
}

pub(crate) fn get_json(url: &str, query: &[(&str, &str)]) -> Result<serde_json::Value, Error> {
    get_json_with(url, query, None)
}

/// [`get_json`], signed in with a username and password (HTTP Basic).
pub(crate) fn get_json_with(
    url: &str,
    query: &[(&str, &str)],
    login: Option<(&str, &str)>,
) -> Result<serde_json::Value, Error> {
    throttle(url);
    let mut req = AGENT.get(url).header("Accept", "application/json");
    if let Some((user, password)) = login {
        req = req.header("Authorization", format!("Basic {}", base64(format!("{user}:{password}").as_bytes())));
    }
    for (k, v) in query {
        req = req.query(*k, *v);
    }
    let mut resp = req.call()?;
    let body = resp.body_mut().with_config().limit(MAX_JSON).read_to_vec()?;
    serde_json::from_slice(&body).map_err(|_| Error::new("the answer wasn't readable"))
}

pub(crate) fn get_bytes(url: &str, limit: u64) -> Result<Vec<u8>, Error> {
    throttle(url);
    let mut resp = AGENT.get(url).call()?;
    Ok(resp.body_mut().with_config().limit(limit).read_to_vec()?)
}

/// Standard base64, for an HTTP Basic login.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_for_basic_auth() {
        assert_eq!(super::base64(b"Aladdin:open sesame"), "QWxhZGRpbjpvcGVuIHNlc2FtZQ==");
        assert_eq!(super::base64(b"a"), "YQ==");
        assert_eq!(super::base64(b"ab"), "YWI=");
        assert_eq!(super::base64(b"abc"), "YWJj");
        assert_eq!(super::base64(b""), "");
    }
}
