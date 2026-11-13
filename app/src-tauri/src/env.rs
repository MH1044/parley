//! The real environment the desktop app gives the core: the system clock with the local UTC
//! offset, files under the app data directory, and a time-seeded generator. The network is
//! `parley_publik::http::HttpNet`, the AI transport (module 06).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use parley_core::env::{Clock, Fs, FsError, SplitMix64};

/// The system clock; the UTC offset is the local time zone's at each call, so a learner who
/// travels gets their new local dates.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
    }

    fn utc_offset_minutes(&self) -> i32 {
        chrono::Local::now().offset().local_minus_utc() / 60
    }
}

/// Files under a root directory: a relative path is taken from the root, an absolute one (a
/// file the learner picked) is used as it is. Writing creates missing parent folders.
#[derive(Debug, Clone)]
pub struct DirFs {
    root: PathBuf,
}

impl DirFs {
    pub fn new(root: PathBuf) -> DirFs {
        DirFs { root }
    }

    fn full(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        }
    }
}

/// An I/O error as the core sees it: not found, or the error's text, which names no path.
fn fs_error(path: &Path, e: std::io::Error) -> FsError {
    if e.kind() == std::io::ErrorKind::NotFound {
        FsError::NotFound(path.to_path_buf())
    } else {
        FsError::Io(e.to_string())
    }
}

impl Fs for DirFs {
    fn read(&self, path: &Path) -> Result<Vec<u8>, FsError> {
        std::fs::read(self.full(path)).map_err(|e| fs_error(path, e))
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), FsError> {
        let full = self.full(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).map_err(|e| fs_error(path, e))?;
        }
        std::fs::write(&full, bytes).map_err(|e| fs_error(path, e))
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), FsError> {
        std::fs::rename(self.full(from), self.full(to)).map_err(|e| fs_error(from, e))
    }

    fn remove(&self, path: &Path) -> Result<(), FsError> {
        std::fs::remove_file(self.full(path)).map_err(|e| fs_error(path, e))
    }

    /// The names of the files (not folders) in `dir`, sorted; a missing folder is empty.
    fn list(&self, dir: &Path) -> Result<Vec<String>, FsError> {
        let entries = match std::fs::read_dir(self.full(dir)) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(fs_error(dir, e)),
        };
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        Ok(names)
    }
}

/// A generator seeded from the system time, for lessons and session ids.
pub fn time_seeded_rng() -> SplitMix64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    SplitMix64::new((nanos as u64) ^ ((nanos >> 64) as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_fs_resolves_relative_paths_and_creates_folders() {
        let dir = tempfile::tempdir().unwrap();
        let fs = DirFs::new(dir.path().to_path_buf());
        fs.write(Path::new("packs/es.part"), b"abc").unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("packs/es.part")).unwrap(),
            b"abc"
        );
        fs.rename(Path::new("packs/es.part"), Path::new("packs/es.sqlite3"))
            .unwrap();
        assert_eq!(fs.read(Path::new("packs/es.sqlite3")).unwrap(), b"abc");
        let absolute = dir.path().join("elsewhere.json");
        fs.write(&absolute, b"{}").unwrap();
        assert_eq!(fs.read(&absolute).unwrap(), b"{}");
        fs.remove(Path::new("packs/es.sqlite3")).unwrap();
        assert_eq!(
            fs.remove(Path::new("packs/es.sqlite3")),
            Err(FsError::NotFound(PathBuf::from("packs/es.sqlite3")))
        );
        assert!(matches!(fs.write(dir.path(), b"x"), Err(FsError::Io(_))));
    }

    #[test]
    fn dir_fs_lists_the_files_of_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let fs = DirFs::new(dir.path().to_path_buf());
        assert_eq!(fs.list(Path::new("cache/audio")), Ok(Vec::new()));
        fs.write(Path::new("cache/audio/b.mp3"), b"b").unwrap();
        fs.write(Path::new("cache/audio/a.mp3"), b"a").unwrap();
        fs.write(Path::new("cache/audio/inner/c.mp3"), b"c")
            .unwrap();
        assert_eq!(
            fs.list(Path::new("cache/audio")),
            Ok(vec!["a.mp3".to_string(), "b.mp3".to_string()])
        );
    }

    #[test]
    fn the_system_clock_is_after_2026_and_the_offset_is_in_range() {
        let clock = SystemClock;
        assert!(clock.now_ms() > 1_767_225_600_000);
        assert!((-14 * 60..=14 * 60).contains(&clock.utc_offset_minutes()));
    }
}
