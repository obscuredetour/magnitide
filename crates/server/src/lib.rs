//! Serves a library on the local network, read-only: an OPDS catalogue for
//! reading apps (KOReader, Thorium…) and a plain web page for e-reader
//! browsers (Kobo's own), to browse, search and download books over Wi-Fi.
//!
//! Requests are handled by [`handle`], a plain function from a request to a
//! reply, so the routes are tested without sockets; [`Server`] puts it on a
//! port with a few worker threads. The library comes from a [`Source`], so
//! the desktop app serves whatever library it has open and a headless server
//! can serve one from disk.

mod addresses;
mod html;
mod opds;
mod routes;
mod util;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use base64::Engine;
use magnitide_catalog::Catalog;

pub use addresses::{Addresses, lan_addresses};
pub use routes::handle;

/// A library to serve.
pub trait Served: Send + Sync {
    /// Shown as the catalogue's title.
    fn name(&self) -> &str;
    fn root(&self) -> &Path;
    fn catalog(&self) -> &Catalog;
}

/// Where the library comes from: asked on every request, so the app can
/// switch libraries while serving.
pub trait Source: Send + Sync + 'static {
    fn current(&self) -> Option<Arc<dyn Served>>;
}

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    /// When set, every request needs it (HTTP Basic, any user name).
    pub password: Option<String>,
    /// For cover thumbnails and KEPUBs converted for Kobos.
    pub cache_dir: PathBuf,
}

/// A request, as the routes see it.
#[derive(Debug, Default)]
pub struct Request {
    pub path: String,
    /// Query parameters, decoded.
    pub query: Vec<(String, String)>,
    pub user_agent: String,
}

impl Request {
    /// From a raw request target ("/search?q=dune").
    pub fn new(target: &str, user_agent: &str) -> Self {
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        Self { path: util::percent_decode(path), query: util::parse_query(query), user_agent: user_agent.to_owned() }
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.query.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// Kobo's own browser (its user agent says so), which wants KEPUBs.
    pub fn from_kobo(&self) -> bool {
        self.user_agent.contains("Kobo")
    }
}

#[derive(Debug)]
pub enum Body {
    Bytes(Vec<u8>),
    File(PathBuf),
}

#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub content_type: String,
    pub headers: Vec<(String, String)>,
    pub body: Body,
}

impl Reply {
    fn new(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> Self {
        Self { status, content_type: content_type.to_owned(), headers: Vec::new(), body: Body::Bytes(body.into()) }
    }

    fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_owned(), value.into()));
        self
    }

    /// The reply's body as text (tests).
    pub fn text(&self) -> String {
        match &self.body {
            Body::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
            Body::File(p) => format!("<file {}>", p.display()),
        }
    }
}

/// Whether a request's `Authorization` header carries the password (any user name).
pub fn authorized(password: Option<&str>, authorization: Option<&str>) -> bool {
    let Some(password) = password.filter(|p| !p.is_empty()) else { return true };
    let Some(encoded) = authorization.and_then(|a| a.strip_prefix("Basic ")) else { return false };
    let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded.trim()) else { return false };
    let given = decoded.splitn(2, |&b| b == b':').nth(1).unwrap_or_default();
    // Compare without stopping at the first difference.
    given.len() == password.len() && given.iter().zip(password.as_bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

/// A running server; stops when dropped.
pub struct Server {
    http: Arc<tiny_http::Server>,
    port: u16,
    stopping: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
}

const WORKERS: usize = 4;

impl Server {
    /// Listen on all interfaces at `config.port` (0 picks a free one).
    pub fn start(config: Config, source: Arc<dyn Source>) -> Result<Self, String> {
        let http = tiny_http::Server::http(("0.0.0.0", config.port)).map_err(|e| {
            if e.to_string().contains("in use") {
                format!("port {} is already in use by another app", config.port)
            } else {
                e.to_string()
            }
        })?;
        let port = http.server_addr().to_ip().map_or(config.port, |a| a.port());
        let http = Arc::new(http);
        let stopping = Arc::new(AtomicBool::new(false));
        let config = Arc::new(config);
        let workers = (0..WORKERS)
            .map(|_| {
                let (http, stopping, source, config) = (http.clone(), stopping.clone(), source.clone(), config.clone());
                std::thread::spawn(move || {
                    while !stopping.load(Ordering::Relaxed) {
                        let Ok(request) = http.recv() else { break };
                        respond(request, &config, source.as_ref());
                    }
                })
            })
            .collect();
        Ok(Self { http, port, stopping, workers })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stopping.store(true, Ordering::Relaxed);
        for _ in 0..self.workers.len() {
            self.http.unblock();
        }
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn respond(mut request: tiny_http::Request, config: &Config, source: &dyn Source) {
    let header = |name: &'static str| {
        request.headers().iter().find(|h| h.field.equiv(name)).map(|h| h.value.as_str().to_owned())
    };
    let reply = if !matches!(request.method(), tiny_http::Method::Get | tiny_http::Method::Head) {
        Reply::new(405, "text/plain; charset=utf-8", "This catalogue is read-only.").header("Allow", "GET, HEAD")
    } else if !authorized(config.password.as_deref(), header("Authorization").as_deref()) {
        Reply::new(401, "text/plain; charset=utf-8", "This library needs its password.")
            .header("WWW-Authenticate", "Basic realm=\"Magnitide\", charset=\"UTF-8\"")
    } else {
        let req = Request::new(request.url(), &header("User-Agent").unwrap_or_default());
        handle(&req, source, &config.cache_dir)
    };
    // Drain any body (none expected for GET).
    let _ = std::io::copy(&mut request.as_reader().take(1 << 16), &mut std::io::sink());
    let mut headers: Vec<tiny_http::Header> = reply
        .headers
        .iter()
        .chain(
            [
                ("Content-Type".to_owned(), reply.content_type.clone()),
                ("X-Content-Type-Options".to_owned(), "nosniff".to_owned()),
                ("Referrer-Policy".to_owned(), "no-referrer".to_owned()),
            ]
            .iter(),
        )
        .filter_map(|(k, v)| tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()).ok())
        .collect();
    let status = tiny_http::StatusCode(reply.status);
    let result = match reply.body {
        Body::Bytes(bytes) => request
            .respond(tiny_http::Response::from_data(bytes).with_status_code(status).with_headers_vec(&mut headers)),
        Body::File(path) => match std::fs::File::open(&path) {
            Ok(file) => request
                .respond(tiny_http::Response::from_file(file).with_status_code(status).with_headers_vec(&mut headers)),
            Err(_) => request.respond(
                tiny_http::Response::from_string("That file is missing from the library.").with_status_code(404),
            ),
        },
    };
    let _ = result;
}

/// tiny_http's builder takes headers one at a time.
trait WithHeaders {
    fn with_headers_vec(self, headers: &mut Vec<tiny_http::Header>) -> Self;
}

impl<R: Read> WithHeaders for tiny_http::Response<R> {
    fn with_headers_vec(mut self, headers: &mut Vec<tiny_http::Header>) -> Self {
        for h in headers.drain(..) {
            self.add_header(h);
        }
        self
    }
}

#[cfg(test)]
mod tests;
