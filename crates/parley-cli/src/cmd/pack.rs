//! `parley pack build`, `parley pack info` and `parley pack install`.

use std::io::Write;
use std::path::{Path, PathBuf};

use parley_core::app::{LockedPack, PackInstallError, STORE_FILE};
use parley_core::content::{PackError, PackInput, Report, build_pack, pack_info, parse_packs_lock};
use parley_core::env::{Fs, FsError};
use parley_core::store::Store;

use super::{
    failure, is_date, missing, not_found, parse, parse_lang, print_json, shown, today, unknown,
    usage,
};

pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match args.first().map(String::as_str) {
        Some("build") => build(&args[1..], out, err),
        Some("info") => info(&args[1..], out, err),
        Some("install") => install(&args[1..], out, err),
        Some(other) => unknown(err, other),
        None => missing(err, "pack build, pack info or pack install"),
    }
}

/// Maps a pack error to the exit code and output of the command: a missing input is a usage
/// error, content errors print the validator's report, anything else is one line on stderr.
fn report_error(error: PackError, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match error {
        PackError::NotFound(path) => not_found(err, &path),
        PackError::Content(errors) => {
            let report = Report {
                errors: errors.into_iter().map(|e| e.0).collect(),
                warnings: Vec::new(),
            };
            print_json(out, &report);
            1
        }
        other => failure(err, &other.to_string()),
    }
}

fn build(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let parsed = match parse(
        args,
        &[
            "--lang",
            "--tatoeba-dir",
            "--wiktextract",
            "--syllabus",
            "--exclude",
            "--built-at",
            "--out",
        ],
        &[],
    ) {
        Ok(parsed) => parsed,
        Err(message) => return usage(err, &message),
    };
    if let Some(extra) = parsed.positional.first() {
        return unknown(err, extra);
    }
    let Some(lang) = parsed.value("--lang") else {
        return missing(err, "--lang");
    };
    let lang = match parse_lang(err, lang) {
        Ok(lang) => lang,
        Err(code) => return code,
    };
    let Some(tatoeba_dir) = parsed.value("--tatoeba-dir") else {
        return missing(err, "--tatoeba-dir");
    };
    let Some(wiktextract) = parsed.value("--wiktextract") else {
        return missing(err, "--wiktextract");
    };
    let Some(syllabus) = parsed.value("--syllabus") else {
        return missing(err, "--syllabus");
    };
    let Some(out_path) = parsed.value("--out") else {
        return missing(err, "--out");
    };
    let built_at = parsed
        .value("--built-at")
        .map_or_else(today, str::to_string);
    if !is_date(&built_at) {
        return usage(err, "--built-at must be YYYY-MM-DD. Run parley --help.");
    }
    let input = PackInput {
        lang,
        tatoeba_dir: PathBuf::from(tatoeba_dir),
        wiktextract: PathBuf::from(wiktextract),
        syllabus: PathBuf::from(syllabus),
        exclude: parsed.value("--exclude").map(PathBuf::from),
        built_at,
    };
    match build_pack(&input, Path::new(out_path)) {
        Ok(report) => {
            print_json(out, &report);
            0
        }
        Err(error) => report_error(error, out, err),
    }
}

fn info(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let parsed = match parse(args, &[], &[]) {
        Ok(parsed) => parsed,
        Err(message) => return usage(err, &message),
    };
    if let Some(extra) = parsed.positional.get(1) {
        return unknown(err, extra);
    }
    let Some(file) = parsed.positional.first() else {
        return missing(err, "<file.sqlite3>");
    };
    match pack_info(Path::new(file)) {
        Ok(info) => {
            print_json(out, &info);
            0
        }
        Err(error) => report_error(error, out, err),
    }
}

/// The lock `pack install` checks against unless `--lock` names another.
const DEFAULT_LOCK: &str = "content/packs.lock.toml";

/// Files under the data directory given with `--data-dir`, reached the way the app reaches its
/// own: every path relative to the directory, missing folders created on write.
struct DataDirFs(PathBuf);

fn fs_error(path: &Path, e: std::io::Error) -> FsError {
    if e.kind() == std::io::ErrorKind::NotFound {
        FsError::NotFound(path.to_path_buf())
    } else {
        FsError::Io(e.to_string())
    }
}

impl Fs for DataDirFs {
    fn read(&self, path: &Path) -> Result<Vec<u8>, FsError> {
        std::fs::read(self.0.join(path)).map_err(|e| fs_error(path, e))
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), FsError> {
        let full = self.0.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).map_err(|e| fs_error(path, e))?;
        }
        std::fs::write(full, bytes).map_err(|e| fs_error(path, e))
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), FsError> {
        std::fs::rename(self.0.join(from), self.0.join(to)).map_err(|e| fs_error(from, e))
    }

    fn remove(&self, path: &Path) -> Result<(), FsError> {
        std::fs::remove_file(self.0.join(path)).map_err(|e| fs_error(path, e))
    }
}

/// `parley pack install <file> --data-dir <dir> [--lock <file>]`: for development, installs a
/// locally built pack into a started course. The file must be the pack
/// the lock pins for its language (SHA-256 and size), checked before anything is written; it
/// then takes the path a download takes.
fn install(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let parsed = match parse(args, &["--data-dir", "--lock"], &[]) {
        Ok(parsed) => parsed,
        Err(message) => return usage(err, &message),
    };
    if let Some(extra) = parsed.positional.get(1) {
        return unknown(err, extra);
    }
    let Some(file) = parsed.positional.first() else {
        return missing(err, "<file>");
    };
    let Some(data_dir) = parsed.value("--data-dir") else {
        return missing(err, "--data-dir");
    };
    let lock = parsed.value("--lock").unwrap_or(DEFAULT_LOCK);
    if !Path::new(file).is_file() {
        return not_found(err, &shown(file));
    }
    if !Path::new(data_dir).is_dir() {
        return not_found(err, &shown(data_dir));
    }
    if !Path::new(lock).is_file() {
        return not_found(err, &shown(lock));
    }

    // The pack's language comes from its meta; a file that is not a pack fails as `pack info`.
    let info = match pack_info(Path::new(file)) {
        Ok(info) => info,
        Err(error) => return report_error(error, out, err),
    };
    let lock_text = match std::fs::read_to_string(lock) {
        Ok(text) => text,
        Err(e) => return failure(err, &format!("{}: {e}", shown(lock))),
    };
    if let Err(e) = parse_packs_lock(&lock_text) {
        return failure(err, &e.0);
    }
    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(e) => return failure(err, &format!("{}: {e}", shown(file))),
    };
    let locked = match LockedPack::verify(&lock_text, &info.lang, bytes) {
        Ok(locked) => locked,
        Err(e) => return failure(err, &e.to_string()),
    };

    // A course is started in the app's data file; without one there is nothing to install into,
    // and no data file is created.
    let store_path = Path::new(data_dir).join(STORE_FILE);
    if !store_path.is_file() {
        return failure(
            err,
            &PackInstallError::NotStarted(locked.lang()).to_string(),
        );
    }
    let mut store = match Store::open(&store_path) {
        Ok(store) => store,
        Err(e) => return failure(err, &e.to_string()),
    };
    let fs = DataDirFs(PathBuf::from(data_dir));
    match locked.install(&mut store, &fs) {
        Ok(installed) => {
            print_json(out, &installed);
            0
        }
        Err(e) => failure(err, &e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_capture(args: &[&str]) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(&args, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn install_usage_errors() {
        let (code, out, err) = run_capture(&["install"]);
        assert_eq!((code, out.as_str()), (2, ""));
        assert_eq!(err, "error: <file> is required. Run parley --help.\n");
        let (code, _, err) = run_capture(&["install", "es.sqlite3"]);
        assert_eq!(code, 2);
        assert_eq!(err, "error: --data-dir is required. Run parley --help.\n");
        let (code, _, err) = run_capture(&["install", "no/such/es.sqlite3", "--data-dir", "."]);
        assert_eq!(code, 2);
        assert_eq!(err, "error: no/such/es.sqlite3 not found\n");
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("es.sqlite3");
        std::fs::write(&file, b"x").unwrap();
        let file = parley_core::content::display_path(&file);
        let (code, _, err) = run_capture(&["install", &file, "--data-dir", "no/such/dir"]);
        assert_eq!(code, 2);
        assert_eq!(err, "error: no/such/dir not found\n");
        let (code, _, err) = run_capture(&[
            "install",
            &file,
            "--data-dir",
            ".",
            "--lock",
            "no/such/packs.lock.toml",
        ]);
        assert_eq!(code, 2);
        assert_eq!(err, "error: no/such/packs.lock.toml not found\n");
    }

    #[test]
    fn a_file_that_is_not_a_pack_fails_as_pack_info_does() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("es.sqlite3");
        std::fs::write(
            &file,
            b"this is not a database, only some text that is long enough",
        )
        .unwrap();
        let lock = dir.path().join("packs.lock.toml");
        std::fs::write(&lock, "schema = \"parley.packs/1\"\n").unwrap();
        let file = parley_core::content::display_path(&file);
        let data = parley_core::content::display_path(dir.path());
        let lock = parley_core::content::display_path(&lock);
        let (code, out, err) =
            run_capture(&["install", &file, "--data-dir", &data, "--lock", &lock]);
        assert_eq!((code, out.as_str()), (1, ""));
        let (_, _, info_err) = run_capture(&["info", &file]);
        assert_eq!(err, info_err);
        assert!(err.starts_with("error: "), "{err}");
        assert!(!dir.path().join("packs").exists());
    }
}
