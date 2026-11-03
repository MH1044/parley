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

/// SplitMix64 (Steele, Lea and Flood 2014, "Fast splittable pseudorandom number generators"):
/// the seeded source for simulations. Integer arithmetic only, so a seed gives the same sequence
/// on every platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        SplitMix64 { state: seed }
    }
}

impl Rng for SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// A civil date in the learner's time zone. Displays as `YYYY-MM-DD`; orders chronologically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocalDate {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl fmt::Display for LocalDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

const MS_PER_DAY: i64 = 86_400_000;

/// The civil date of a day count since 1970-01-01 (Howard Hinnant's `civil_from_days`), in
/// integer arithmetic so no date library is needed.
fn civil_from_days(days: i64) -> LocalDate {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if month <= 2 { year + 1 } else { year };
    LocalDate {
        year: year as i32,
        month,
        day,
    }
}

/// Today's date where the learner is: the clock's milliseconds shifted by its UTC offset, floored
/// to whole days, as a civil date. Streaks and daily goals count these dates.
pub fn local_date(clock: &dyn Clock) -> LocalDate {
    let local_ms = clock
        .now_ms()
        .saturating_add(i64::from(clock.utc_offset_minutes()) * 60_000);
    civil_from_days(local_ms.div_euclid(MS_PER_DAY))
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

    struct FixedClock {
        ms: i64,
        offset: i32,
    }

    impl Clock for FixedClock {
        fn now_ms(&self) -> i64 {
            self.ms
        }
        fn utc_offset_minutes(&self) -> i32 {
            self.offset
        }
    }

    #[test]
    fn splitmix64_matches_the_reference_output() {
        let mut rng = SplitMix64::new(0);
        assert_eq!(rng.next_u64(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(rng.next_u64(), 0x6E78_9E6A_A1B9_65F4);
        assert_eq!(rng.next_u64(), 0x06C4_5D18_8009_454F);
        let mut a = SplitMix64::new(42);
        let mut b = a.clone();
        assert_eq!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn local_date_applies_the_offset_before_flooring() {
        // 2026-03-29T00:30Z is 20,541 days and 1,800 seconds after the epoch.
        let ms = 20_541 * MS_PER_DAY + 30 * 60_000;
        let east = FixedClock { ms, offset: 60 };
        let west = FixedClock { ms, offset: -300 };
        assert_eq!(local_date(&east).to_string(), "2026-03-29");
        assert_eq!(local_date(&west).to_string(), "2026-03-28");
        assert_eq!(
            local_date(&FixedClock { ms: 0, offset: 0 }),
            LocalDate {
                year: 1970,
                month: 1,
                day: 1
            }
        );
        assert_eq!(
            local_date(&FixedClock { ms: -1, offset: 0 }).to_string(),
            "1969-12-31"
        );
        assert_eq!(civil_from_days(11_016).to_string(), "2000-02-29");
        assert_eq!(civil_from_days(11_017).to_string(), "2000-03-01");
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
