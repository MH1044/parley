//! `content/<lang>/ids.lock`: every item id ever published, so an id is never renumbered or
//! reused (docs/modules/01-content-model-and-syllabus.md).
//!
//! Format: TSV, sorted by item id in byte order, LF line ends, no header:
//! `item_id<TAB>active|retired<TAB>syllabus version first seen`.

use std::collections::BTreeMap;

use serde::Serialize;

use super::ContentError;
use super::forms::Forms;
use super::items::derive_items;
use super::syllabus::Syllabus;

/// One line of the lock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockLine {
    pub active: bool,
    /// The syllabus `version` the id was first seen in.
    pub version: u32,
}

/// The id lock of one language.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IdsLock {
    lines: BTreeMap<String, LockLine>,
}

impl IdsLock {
    /// Parses the file; a malformed line names its number.
    pub fn parse(text: &str) -> Result<IdsLock, ContentError> {
        let mut lines = BTreeMap::new();
        for (index, line) in text.lines().enumerate() {
            let n = index + 1;
            let line = line.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }
            let columns: Vec<&str> = line.split('\t').collect();
            if columns.len() != 3 {
                return Err(ContentError(format!(
                    "ids.lock: line {n}: expected 3 tab-separated columns"
                )));
            }
            let active = match columns[1] {
                "active" => true,
                "retired" => false,
                _ => {
                    return Err(ContentError(format!(
                        "ids.lock: line {n}: status must be active or retired"
                    )));
                }
            };
            let version: u32 = columns[2].parse().map_err(|_| {
                ContentError(format!("ids.lock: line {n}: version must be an integer"))
            })?;
            if lines
                .insert(columns[0].to_string(), LockLine { active, version })
                .is_some()
            {
                return Err(ContentError(format!(
                    "ids.lock: line {n}: duplicate id {}",
                    columns[0]
                )));
            }
        }
        Ok(IdsLock { lines })
    }

    /// The file's text: sorted by id in byte order, LF line ends.
    pub fn to_text(&self) -> String {
        let mut text = String::new();
        for (id, line) in &self.lines {
            text.push_str(id);
            text.push('\t');
            text.push_str(if line.active { "active" } else { "retired" });
            text.push('\t');
            text.push_str(&line.version.to_string());
            text.push('\n');
        }
        text
    }

    pub fn is_active(&self, id: &str) -> bool {
        self.lines.get(id).is_some_and(|line| line.active)
    }

    pub fn is_retired(&self, id: &str) -> bool {
        self.lines.get(id).is_some_and(|line| !line.active)
    }

    /// Whether the id has a line, active or retired.
    pub fn contains(&self, id: &str) -> bool {
        self.lines.contains_key(id)
    }

    /// Every line, by id in byte order.
    pub fn lines(&self) -> impl Iterator<Item = (&str, &LockLine)> {
        self.lines.iter().map(|(id, line)| (id.as_str(), line))
    }

    /// Every active id, in byte order.
    pub fn active_ids(&self) -> Vec<&str> {
        self.lines
            .iter()
            .filter(|(_, line)| line.active)
            .map(|(id, _)| id.as_str())
            .collect()
    }

    /// Whether the lock has no lines at all.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// What `content lock` changed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct LockReport {
    /// Syllabus ids that were not active: new or reactivated.
    pub added: Vec<String>,
    /// Active ids no longer in the syllabus.
    pub retired: Vec<String>,
}

/// The lock with every id of the syllabus active and every other active id retired. A new line
/// records the syllabus `version` as first seen; a retired or reactivated line keeps its version.
pub fn lock_items(s: &Syllabus, forms: &Forms, lock: &IdsLock) -> (IdsLock, LockReport) {
    let mut next = lock.clone();
    let mut report = LockReport::default();
    let ids: Vec<String> = derive_items(s, forms).into_iter().map(|i| i.id).collect();
    for id in &ids {
        match next.lines.get_mut(id) {
            Some(line) if line.active => {}
            Some(line) => {
                line.active = true;
                report.added.push(id.clone());
            }
            None => {
                next.lines.insert(
                    id.clone(),
                    LockLine {
                        active: true,
                        version: s.version,
                    },
                );
                report.added.push(id.clone());
            }
        }
    }
    for (id, line) in next.lines.iter_mut() {
        if line.active && !ids.contains(id) {
            line.active = false;
            report.retired.push(id.clone());
        }
    }
    report.added.sort_unstable();
    report.retired.sort_unstable();
    (next, report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::parse_syllabus;
    use crate::content::test_support::{EXAMPLE_SYLLABUS, example_forms};

    #[test]
    fn parse_and_write_round_trip_in_byte_order() {
        let text = "es.a1.u01.s01#l:hola\tactive\t1\nes.a1.u01.s01#e:2\tretired\t1\n";
        let lock = IdsLock::parse(text).unwrap();
        assert!(lock.is_active("es.a1.u01.s01#l:hola"));
        assert!(lock.is_retired("es.a1.u01.s01#e:2"));
        assert!(!lock.is_active("es.a1.u01.s01#e:2"));
        assert!(!lock.is_retired("nope"));
        assert_eq!(
            lock.to_text(),
            "es.a1.u01.s01#e:2\tretired\t1\nes.a1.u01.s01#l:hola\tactive\t1\n"
        );
        assert!(IdsLock::parse("").unwrap().is_empty());
        let err = IdsLock::parse("a\tactive\n").unwrap_err();
        assert_eq!(err.0, "ids.lock: line 1: expected 3 tab-separated columns");
        let err = IdsLock::parse("a\tgone\t1\n").unwrap_err();
        assert_eq!(err.0, "ids.lock: line 1: status must be active or retired");
    }

    #[test]
    fn locking_adds_retires_and_reactivates() {
        let syllabus = parse_syllabus("s.yaml", EXAMPLE_SYLLABUS).unwrap();
        let forms = example_forms();
        let (lock, report) = lock_items(&syllabus, &forms, &IdsLock::default());
        assert_eq!(report.added.len(), 11);
        assert!(report.retired.is_empty());
        assert!(lock.is_active("es.a1.u01.s01#e:2"));
        assert_eq!(report.added[0], "es.a1.u01.s01#e:1");

        let without_2 = EXAMPLE_SYLLABUS.replace(
            "          - {id: 2, target: \"Buenos días.\", en: [\"Good morning.\"]}\n",
            "",
        );
        let smaller = parse_syllabus("s.yaml", &without_2).unwrap();
        assert_eq!(smaller.units[0].skills[0].examples.len(), 3);
        let (lock2, report2) = lock_items(&smaller, &forms, &lock);
        assert_eq!(report2.added, Vec::<String>::new());
        assert_eq!(report2.retired, ["es.a1.u01.s01#e:2"]);
        assert!(lock2.is_retired("es.a1.u01.s01#e:2"));
        assert!(lock2.to_text().contains("es.a1.u01.s01#e:2\tretired\t1\n"));

        let (lock3, report3) = lock_items(&syllabus, &forms, &lock2);
        assert_eq!(report3.added, ["es.a1.u01.s01#e:2"]);
        assert!(lock3.is_active("es.a1.u01.s01#e:2"));
        assert_eq!(lock3, lock);
    }

    #[test]
    fn a_version_that_is_not_an_integer_and_a_repeated_id_are_refused() {
        let err = IdsLock::parse("es.a1.u01.s01#l:hola\tactive\tone\n")
            .expect_err("a version that is not an integer is refused");
        assert_eq!(err.0, "ids.lock: line 1: version must be an integer");
        let err =
            IdsLock::parse("es.a1.u01.s01#l:hola\tactive\t1\nes.a1.u01.s01#l:hola\tretired\t1\n")
                .expect_err("a repeated id is refused");
        assert_eq!(err.0, "ids.lock: line 2: duplicate id es.a1.u01.s01#l:hola");
    }
}
