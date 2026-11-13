//! The environment the core is given instead of reading it: clock, randomness, filesystem and
//! network (docs/ARCHITECTURE.md §3 and invariant I3).
//!
//! Core code takes these as trait objects or generics. The Tauri crate passes the real ones; tests
//! pass fakes with fixed values, so a lesson built today is the same lesson built in a year, and no
//! test ever reaches the network.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

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

/// The value `n` below which a random index is drawn: `next_u64() % n`. The small modulo bias
/// (at most n / 2^64) does not matter for choosing exercises, and the plain remainder is the same
/// on every platform. `n` must be at least 1.
pub fn below(rng: &mut dyn Rng, n: usize) -> usize {
    assert!(n >= 1, "below needs n >= 1");
    (rng.next_u64() % n as u64) as usize
}

/// Shuffles `items` in place with Fisher and Yates's algorithm (Durstenfeld's form): for i from
/// len − 1 down to 1, swap item i with item `below(rng, i + 1)`. A seed gives the same order on
/// every platform.
pub fn shuffle<T>(items: &mut [T], rng: &mut dyn Rng) {
    for i in (1..items.len()).rev() {
        let j = below(rng, i + 1);
        items.swap(i, j);
    }
}

/// A civil date in the learner's time zone. Displays and serialises as `YYYY-MM-DD`; orders
/// chronologically.
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

impl Serialize for LocalDate {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for LocalDate {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        LocalDate::parse(&text).ok_or_else(|| {
            serde::de::Error::custom(format!("{text} is not a date of the form YYYY-MM-DD"))
        })
    }
}

impl LocalDate {
    /// The civil date `days` days after 1970-01-01.
    pub fn from_days(days: i64) -> LocalDate {
        civil_from_days(days)
    }

    /// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`), the inverse of
    /// [`LocalDate::from_days`].
    pub fn days(self) -> i64 {
        let year = i64::from(self.year) - i64::from(self.month <= 2);
        let era = year.div_euclid(400);
        let yoe = year.rem_euclid(400);
        let month = i64::from(self.month);
        let mp = if month > 2 { month - 3 } else { month + 9 };
        let doy = (153 * mp + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// Reads exactly `YYYY-MM-DD` (four digits, two, two) naming a real date; anything else,
    /// such as `2026-02-30` or `2026-2-3`, is none.
    pub fn parse(text: &str) -> Option<LocalDate> {
        let bytes = text.as_bytes();
        let shaped = bytes.len() == 10
            && bytes.iter().enumerate().all(|(i, b)| {
                if i == 4 || i == 7 {
                    *b == b'-'
                } else {
                    b.is_ascii_digit()
                }
            });
        if !shaped {
            return None;
        }
        let year: i32 = text[0..4].parse().ok()?;
        let month: u32 = text[5..7].parse().ok()?;
        let day: u32 = text[8..10].parse().ok()?;
        if !(1..=12).contains(&month) || day == 0 {
            return None;
        }
        let date = LocalDate { year, month, day };
        // A day past the month's end rolls over into the next month, so the round trip finds it.
        (LocalDate::from_days(date.days()) == date).then_some(date)
    }

    /// The ISO 8601 week: (week-numbering year, week 1 to 53). Weeks start on Monday, and week 1
    /// is the week that holds the year's first Thursday, so a date belongs to the week-year of
    /// the Thursday of its week.
    pub fn iso_week(self) -> (i32, u32) {
        let days = self.days();
        // 1970-01-01 was a Thursday: weekday 1 (Monday) to 7 (Sunday).
        let weekday = (days + 3).rem_euclid(7) + 1;
        let thursday = LocalDate::from_days(days - weekday + 4);
        let january_first = LocalDate {
            year: thursday.year,
            month: 1,
            day: 1,
        };
        let week = (thursday.days() - january_first.days()) / 7 + 1;
        (thursday.year, week as u32)
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
    local_date_at(clock.now_ms(), clock.utc_offset_minutes())
}

/// The civil date at `ms` milliseconds since the epoch where the UTC offset is
/// `utc_offset_minutes` (east positive): shifted by the offset, then floored to whole days.
pub fn local_date_at(ms: i64, utc_offset_minutes: i32) -> LocalDate {
    let local_ms = ms.saturating_add(i64::from(utc_offset_minutes) * 60_000);
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
    /// The names of the files in `dir` (relative to the data directory), sorted; a missing
    /// directory is an empty list (module 09, clearing the audio cache). The default lists
    /// nothing, so a filesystem that never holds cached audio need not implement it.
    fn list(&self, _dir: &Path) -> Result<Vec<String>, FsError> {
        Ok(Vec::new())
    }
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

    fn date(text: &str) -> LocalDate {
        LocalDate::parse(text).unwrap_or_else(|| panic!("{text} is a date"))
    }

    #[test]
    fn day_counts_round_trip() {
        assert_eq!(date("1970-01-01").days(), 0);
        assert_eq!(date("1969-12-31").days(), -1);
        assert_eq!(date("2000-02-29").days(), 11_016);
        assert_eq!(date("2026-10-27").days(), 20_753);
        for days in (-800_000..800_000).step_by(997) {
            assert_eq!(LocalDate::from_days(days).days(), days);
        }
    }

    #[test]
    fn parse_takes_real_dates_in_one_shape_only() {
        assert_eq!(
            date("2026-02-28"),
            LocalDate {
                year: 2026,
                month: 2,
                day: 28
            }
        );
        assert!(LocalDate::parse("2024-02-29").is_some());
        for bad in [
            "2026-02-30",
            "2025-02-29",
            "2026-13-01",
            "2026-00-10",
            "2026-04-31",
            "2026-1-01",
            "2026/01/01",
            "20260101",
            " 2026-01-01",
            "2026-01-00",
            "",
        ] {
            assert_eq!(LocalDate::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn iso_weeks_follow_the_thursday_rule() {
        assert_eq!(date("2026-01-01").iso_week(), (2026, 1));
        assert_eq!(date("2027-01-01").iso_week(), (2026, 53));
        assert_eq!(date("2024-12-30").iso_week(), (2025, 1));
        assert_eq!(date("2021-01-03").iso_week(), (2020, 53));
        assert_eq!(date("2021-01-04").iso_week(), (2021, 1));
        // 2026-11-02 is a Monday and 2026-11-08 the Sunday of the same week.
        assert_eq!(date("2026-11-02").iso_week(), date("2026-11-08").iso_week());
        assert_ne!(date("2026-11-08").iso_week(), date("2026-11-09").iso_week());
    }

    #[test]
    fn dates_serialise_as_iso_text() {
        let d = date("2026-03-09");
        assert_eq!(serde_json::to_string(&d).unwrap(), "\"2026-03-09\"");
        assert_eq!(
            serde_json::from_str::<LocalDate>("\"2026-03-09\"").unwrap(),
            d
        );
        assert!(serde_json::from_str::<LocalDate>("\"2026-02-30\"").is_err());
    }

    #[test]
    fn local_date_at_counts_late_evening_and_after_midnight_as_two_days() {
        // 2026-03-29T00:30Z is 20,541 days and 1,800 seconds after the epoch.
        let ms = 20_541 * MS_PER_DAY + 30 * 60_000;
        assert_eq!(local_date_at(ms, 0).to_string(), "2026-03-29");
        assert_eq!(local_date_at(ms - 60 * 60_000, 0).to_string(), "2026-03-28");
        assert_eq!(local_date_at(ms, -60).to_string(), "2026-03-28");
    }

    /// A source that returns the given values in turn.
    struct Seq(Vec<u64>, usize);

    impl Rng for Seq {
        fn next_u64(&mut self) -> u64 {
            let value = self.0[self.1 % self.0.len()];
            self.1 += 1;
            value
        }
    }

    #[test]
    fn below_is_the_remainder_and_shuffle_is_fisher_yates() {
        let mut rng = Seq(vec![17], 0);
        assert_eq!(below(&mut rng, 5), 2);
        assert_eq!(below(&mut rng, 1), 0);
        // i = 3: swap(3, 7 % 4 = 3); i = 2: swap(2, 4 % 3 = 1); i = 1: swap(1, 9 % 2 = 1).
        let mut items = ['a', 'b', 'c', 'd'];
        shuffle(&mut items, &mut Seq(vec![7, 4, 9], 0));
        assert_eq!(items, ['a', 'c', 'b', 'd']);
        let mut empty: [u8; 0] = [];
        shuffle(&mut empty, &mut Seq(vec![1], 0));
        let mut a = [1, 2, 3, 4, 5, 6, 7, 8];
        let mut b = a;
        shuffle(&mut a, &mut SplitMix64::new(3));
        shuffle(&mut b, &mut SplitMix64::new(3));
        assert_eq!(a, b);
        let mut sorted = a;
        sorted.sort_unstable();
        assert_eq!(sorted, [1, 2, 3, 4, 5, 6, 7, 8]);
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
