//! The environment the core is given instead of reading it: clock, randomness, filesystem and
//! network (docs/ARCHITECTURE.md §3 and invariant I3).
//!
//! Core code takes these as trait objects or generics. The Tauri crate passes the real ones; tests
//! pass fakes with fixed values, so a lesson built today is the same lesson built in a year, and no
//! test ever reaches the network.

use std::fmt;
use std::path::{Path, PathBuf};

/// Wall-clock time and the local UTC offset, injected so local dates are reproducible in tests.
pub trait Clock {
    /// Milliseconds since the Unix epoch.
    fn now_ms(&self) -> i64;
    /// The local time zone's offset from UTC in minutes (east positive).
    fn utc_offset_minutes(&self) -> i32;
}

/// A source of randomness. Fixtures and template lessons use a seeded implementation.
pub trait Rng {
    fn next_u64(&mut self) -> u64;
}

/// A filesystem error, kept free of absolute paths so it can be logged (docs/ARCHITECTURE.md §8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsError {
    /// The path does not exist.
    NotFound(PathBuf),
    /// Any other failure, described without the data directory's absolute path.
    Io(String),
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FsError::NotFound(path) => write!(f, "{} was not found", path.display()),
            FsError::Io(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for FsError {}

/// Files inside the app data directory. Every path is relative to that directory; the core never
/// learns where the directory is.
pub trait Fs {
    fn read(&self, path: &Path) -> Result<Vec<u8>, FsError>;
    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), FsError>;
    fn rename(&self, from: &Path, to: &Path) -> Result<(), FsError>;
    fn remove(&self, path: &Path) -> Result<(), FsError>;
}

/// An outbound HTTP request. The transport (module 06) turns it into a real call.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

// Header values can carry a key, so the debug form prints header names only (invariant I1: no
// log line contains a key).
impl fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let header_names: Vec<&str> = self.headers.iter().map(|(name, _)| name.as_str()).collect();
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &header_names)
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

/// An HTTP response as the core sees it: status, headers and the raw body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Why a request produced no response at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError {
    /// The network is switched off for this run (tests, and the offline core).
    Disabled,
    /// No connection could be made.
    Offline,
    /// The request ran out of time.
    Timeout,
    /// Any other transport failure, described without the request body.
    Transport(String),
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetError::Disabled => f.write_str("the network is disabled"),
            NetError::Offline => f.write_str("no connection"),
            NetError::Timeout => f.write_str("the request timed out"),
            NetError::Transport(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for NetError {}

/// The network. The only implementation in the core is [`FailingNet`]; the real one lives in the
/// `parley-publik` crate.
pub trait Net {
    fn send(&self, req: HttpRequest) -> Result<HttpResponse, NetError>;
}

/// A network that fails every call with [`NetError::Disabled`].
///
/// Tests of the offline core pass it to prove that placement, template lessons, reviews, grading,
/// streaks and stats never need a connection (invariant I3).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FailingNet;

impl Net for FailingNet {
    fn send(&self, _req: HttpRequest) -> Result<HttpResponse, NetError> {
        Err(NetError::Disabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failing_net_refuses_every_request() {
        let req = HttpRequest {
            method: "GET".into(),
            url: "https://api.example/x".into(),
            headers: vec![],
            body: vec![],
        };
        assert_eq!(FailingNet.send(req), Err(NetError::Disabled));
    }

    #[test]
    fn request_debug_output_shows_header_names_only() {
        let req = HttpRequest {
            method: "POST".into(),
            url: "https://api.example/chat".into(),
            headers: vec![("authorization".into(), "Bearer pk_live_a".into())],
            body: b"{}".to_vec(),
        };
        let debug = format!("{req:?}");
        assert!(debug.contains("authorization"));
        assert!(!debug.contains("pk_live_a"));
    }
}
