//! The subcommands of `parley` (docs/modules/01-content-model-and-syllabus.md, Interfaces).
//!
//! Arguments are parsed by hand. Every command prints one JSON object on standard output and
//! exits 0 on success, 1 on content errors (the JSON report is still printed) or 2 on usage
//! errors, which print one line on standard error and nothing on standard output:
//! `error: {path} not found`, `error: {flag} is required. Run parley --help.` or
//! `error: unknown argument {arg}. Run parley --help.`. A path in a message is the path as
//! typed, with `/` as the separator.

pub mod content;
pub mod forms;
pub mod pack;

use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use parley_core::content::{ContentError, Forms, IdsLock, Lang, load_exclude, load_numbers};
use serde::Serialize;

/// A usage error: one line on standard error, exit code 2.
pub fn usage(err: &mut dyn Write, message: &str) -> u8 {
    let _ = writeln!(err, "error: {message}");
    2
}

/// `error: {flag} is required. Run parley --help.`
pub fn missing(err: &mut dyn Write, flag: &str) -> u8 {
    usage(err, &format!("{flag} is required. Run parley --help."))
}

/// `error: unknown argument {arg}. Run parley --help.`
pub fn unknown(err: &mut dyn Write, arg: &str) -> u8 {
    usage(err, &format!("unknown argument {arg}. Run parley --help."))
}

/// `error: {path} not found`
pub fn not_found(err: &mut dyn Write, path: &str) -> u8 {
    usage(err, &format!("{path} not found"))
}

/// A failure that is neither a usage nor a content error: one line on standard error, exit 1.
pub fn failure(err: &mut dyn Write, message: &str) -> u8 {
    let _ = writeln!(err, "error: {message}");
    1
}

/// Prints one JSON object and a newline.
pub fn print_json(out: &mut dyn Write, value: &impl Serialize) {
    let text = serde_json::to_string(value).expect("command output serialises");
    let _ = writeln!(out, "{text}");
}

/// A path as typed, with `/` as the separator.
pub fn shown(path: &str) -> String {
    parley_core::content::display_path(Path::new(path))
}

/// `{dir}/{name}` as a message shows it.
pub fn join(dir: &str, name: &str) -> String {
    format!("{}/{name}", shown(dir))
}

/// The language named by `--lang`, or the usage error.
pub fn parse_lang(err: &mut dyn Write, text: &str) -> Result<Lang, u8> {
    text.parse()
        .map_err(|_| usage(err, "--lang must be es, fr or de. Run parley --help."))
}

/// Whether `text` has the shape `YYYY-MM-DD`.
pub fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(i, b)| {
            if i == 4 || i == 7 {
                *b == b'-'
            } else {
                b.is_ascii_digit()
            }
        })
}

/// The civil date of a day count since 1970-01-01 (the algorithm of Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// Today's date in UTC as `YYYY-MM-DD`: the one place the tool reads the clock, for the
/// defaults of `--date` and `--built-at`; the core only ever receives the string.
pub fn today() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    format!("{year:04}-{month:02}-{day:02}")
}

/// The arguments of one command: `--flag value` pairs (repeatable), switches and positionals.
#[derive(Debug)]
pub struct Parsed {
    values: Vec<(String, String)>,
    switches: Vec<String>,
    pub positional: Vec<String>,
}

impl Parsed {
    /// The last value given for `flag`.
    pub fn value(&self, flag: &str) -> Option<&str> {
        self.values
            .iter()
            .rev()
            .find(|(name, _)| name == flag)
            .map(|(_, value)| value.as_str())
    }

    /// Every value given for `flag`, in order.
    pub fn values_of(&self, flag: &str) -> Vec<&str> {
        self.values
            .iter()
            .filter(|(name, _)| name == flag)
            .map(|(_, value)| value.as_str())
            .collect()
    }

    pub fn has(&self, switch: &str) -> bool {
        self.switches.iter().any(|s| s == switch)
    }
}

/// Parses `args` against the flags a command takes. The error is the usage message.
pub fn parse(
    args: &[String],
    value_flags: &[&str],
    switch_flags: &[&str],
) -> Result<Parsed, String> {
    let mut parsed = Parsed {
        values: Vec::new(),
        switches: Vec::new(),
        positional: Vec::new(),
    };
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if value_flags.contains(&arg) {
            let Some(value) = args.get(i + 1) else {
                return Err(format!("{arg} needs a value. Run parley --help."));
            };
            parsed.values.push((arg.to_string(), value.clone()));
            i += 2;
            continue;
        }
        if switch_flags.contains(&arg) {
            parsed.switches.push(arg.to_string());
        } else if arg.starts_with('-') {
            return Err(format!("unknown argument {arg}. Run parley --help."));
        } else {
            parsed.positional.push(arg.to_string());
        }
        i += 1;
    }
    Ok(parsed)
}

/// The files read from beside a syllabus: `forms.tsv`, `numbers.yaml`, `exclude.txt` and
/// `ids.lock`, each only when present.
pub struct Siblings {
    pub forms: Forms,
    pub numbers: Vec<String>,
    pub exclude: Vec<String>,
    pub lock: Option<IdsLock>,
}

/// Reads a forms table from `path`.
pub fn load_forms(path: &Path) -> Result<Forms, ContentError> {
    let file = std::fs::File::open(path)
        .map_err(|e| ContentError(format!("{}: {e}", parley_core::content::display_path(path))))?;
    Forms::load_tsv(std::io::BufReader::new(file))
}

fn read_if_present(path: &Path) -> Result<Option<String>, ContentError> {
    if !path.is_file() {
        return Ok(None);
    }
    std::fs::read_to_string(path)
        .map(|text| {
            Some(
                text.strip_prefix('\u{feff}')
                    .map_or(text.clone(), str::to_string),
            )
        })
        .map_err(|e| ContentError(format!("{}: {e}", parley_core::content::display_path(path))))
}

/// Reads the files beside the syllabus at `syllabus`; `forms` overrides the sibling table.
pub fn load_siblings(syllabus: &Path, forms: Option<Forms>) -> Result<Siblings, ContentError> {
    let dir = syllabus.parent().unwrap_or(Path::new("."));
    let forms = match forms {
        Some(forms) => forms,
        None => {
            let path = dir.join("forms.tsv");
            if path.is_file() {
                load_forms(&path)?
            } else {
                Forms::default()
            }
        }
    };
    let numbers = match read_if_present(&dir.join("numbers.yaml"))? {
        Some(text) => load_numbers(&text)?,
        None => Vec::new(),
    };
    let exclude = read_if_present(&dir.join("exclude.txt"))?
        .map(|text| load_exclude(&text))
        .unwrap_or_default();
    let lock = match read_if_present(&dir.join("ids.lock"))? {
        Some(text) => Some(IdsLock::parse(&text)?),
        None => None,
    };
    Ok(Siblings {
        forms,
        numbers,
        exclude,
        lock,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates_are_right_at_the_edges() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(20_753), (2026, 10, 27));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(11_017), (2000, 3, 1));
        assert!(is_date(&today()));
        assert!(is_date("2026-10-01"));
        assert!(!is_date("2026-1-01"));
        assert!(!is_date("today"));
    }

    #[test]
    fn flags_switches_and_positionals_are_separated() {
        let args: Vec<String> = [
            "a", "--forms", "f.tsv", "--json", "--lemma", "x", "--lemma", "y",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let parsed = parse(&args, &["--forms", "--lemma"], &["--json"]).unwrap();
        assert_eq!(parsed.positional, ["a"]);
        assert_eq!(parsed.value("--forms"), Some("f.tsv"));
        assert_eq!(parsed.values_of("--lemma"), ["x", "y"]);
        assert!(parsed.has("--json"));
        assert!(!parsed.has("--quiet"));
        let err = parse(&["--bogus".to_string()], &[], &[]).unwrap_err();
        assert_eq!(err, "unknown argument --bogus. Run parley --help.");
        let err = parse(&["--forms".to_string()], &["--forms"], &[]).unwrap_err();
        assert_eq!(err, "--forms needs a value. Run parley --help.");
    }
}
