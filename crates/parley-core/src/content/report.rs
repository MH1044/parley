//! The course report (`parley content report`) and the rank check (`parley freq check`) of
//! docs/modules/05-course-content-a1-a2.md: counts of a course's units, skills, lemmas and
//! examples, how much of it a sentence pack attests, and whether every lemma's `rank` is the
//! pack's frequency rank.
//!
//! A pack is read by path, like every content file and build output (module 01, Design notes);
//! SQLite opens it read-only.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

use super::checklist::Checklist;
use super::forms::Forms;
use super::pack::PackError;
use super::review::ReviewFile;
use super::syllabus::{Cefr, Lemma, Syllabus};
use super::tokenise::tokenise;
use super::{Lang, display_path, nfc};

/// The rank an A1 lemma must be within to count towards `a1_rank_le_2000_share`.
const A1_RANK_TARGET: u64 = 2_000;

/// What the report needs from a sentence pack: the `norm` of every sentence and the `freq` table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PackStats {
    pub norms: HashSet<String>,
    /// Lemma to rank.
    pub freq: HashMap<String, u64>,
}

/// Reads the sentence norms and the frequency ranks of the pack at `path`.
pub fn read_pack_stats(path: &Path) -> Result<PackStats, PackError> {
    let shown = display_path(path);
    if !path.is_file() {
        return Err(PackError::NotFound(shown));
    }
    let failed = |e: rusqlite::Error| PackError::Sqlite(format!("{shown}: {e}"));
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| PackError::Sqlite(format!("{shown}: could not open: {e}")))?;
    let mut norms = HashSet::new();
    {
        let mut statement = conn.prepare("SELECT norm FROM sentences").map_err(failed)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(failed)?;
        for row in rows {
            norms.insert(row.map_err(failed)?);
        }
    }
    let mut freq = HashMap::new();
    {
        let mut statement = conn
            .prepare("SELECT lemma, rank FROM freq")
            .map_err(failed)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(failed)?;
        for row in rows {
            let (lemma, rank) = row.map_err(failed)?;
            freq.insert(lemma, rank.max(0) as u64);
        }
    }
    Ok(PackStats { norms, freq })
}

/// A count per level. Serialises as `{"A1": n, "A2": n}`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct LevelCounts {
    #[serde(rename = "A1")]
    pub a1: usize,
    #[serde(rename = "A2")]
    pub a2: usize,
}

/// The smallest and largest skill count of a unit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SkillsPerUnit {
    pub min: usize,
    pub max: usize,
}

/// What `parley content report` prints.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CourseReport {
    pub lang: Lang,
    /// Units per level.
    pub units: LevelCounts,
    /// All skills.
    pub skills: usize,
    pub skills_per_unit: SkillsPerUnit,
    /// `A1`: distinct lemma strings of A1 skills; `A2`: those of A2 skills not taught at A1.
    pub lemmas: LevelCounts,
    /// All examples.
    pub examples: usize,
    /// The share of examples whose normalised text is a pack sentence's `norm`, to 4 places;
    /// null without a pack.
    pub attested_share: Option<f64>,
    /// The share of distinct single-word A1 lemmas the pack ranks within 2,000, to 4 places; null
    /// without a pack.
    pub a1_rank_le_2000_share: Option<f64>,
    /// Checklist entries no skill teaches, in file order.
    pub checklist_uncovered: Vec<String>,
    /// Units `review.yaml` marks unreviewed (every unit without one), in syllabus order.
    pub unreviewed_units: Vec<String>,
}

/// `part / whole` rounded half up to 4 decimal places, in integer arithmetic so the result is the
/// same on every platform; 0 when `whole` is 0.
fn share(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    let (part, whole) = (part as u128, whole as u128);
    let scaled = (part * 20_000 + whole) / (2 * whole);
    scaled as f64 / 10_000.0
}

/// A lemma counts as one word when it has no space.
fn single_word(lemma: &Lemma) -> bool {
    !lemma.lemma.contains(' ')
}

/// The pack's rank of a lemma, as written or in NFC.
fn pack_rank(freq: &HashMap<String, u64>, lemma: &str) -> Option<u64> {
    freq.get(lemma).or_else(|| freq.get(&nfc(lemma))).copied()
}

/// The report of a course that validates. `forms` is the course's forms table (the tokens'
/// normalised forms do not depend on it, so it only has to be the one the course uses).
pub fn course_report(
    s: &Syllabus,
    forms: &Forms,
    checklist: Option<&Checklist>,
    review: Option<&ReviewFile>,
    pack: Option<&PackStats>,
) -> CourseReport {
    let mut units = LevelCounts::default();
    for unit in &s.units {
        match unit.cefr {
            Cefr::A1 => units.a1 += 1,
            Cefr::A2 => units.a2 += 1,
        }
    }
    let counts: Vec<usize> = s.units.iter().map(|unit| unit.skills.len()).collect();
    let skills_per_unit = SkillsPerUnit {
        min: counts.iter().copied().min().unwrap_or(0),
        max: counts.iter().copied().max().unwrap_or(0),
    };

    let mut a1_lemmas: Vec<String> = Vec::new();
    let mut a1_seen: HashSet<String> = HashSet::new();
    let mut a1_single: Vec<String> = Vec::new();
    for (_, skill) in s.skills().filter(|(_, skill)| skill.cefr == Cefr::A1) {
        for lemma in &skill.lemmas {
            let key = nfc(&lemma.lemma);
            if a1_seen.insert(key.clone()) {
                a1_lemmas.push(key.clone());
                if single_word(lemma) {
                    a1_single.push(key);
                }
            }
        }
    }
    let mut a2_seen: HashSet<String> = HashSet::new();
    for (_, skill) in s.skills().filter(|(_, skill)| skill.cefr == Cefr::A2) {
        for lemma in &skill.lemmas {
            let key = nfc(&lemma.lemma);
            if !a1_seen.contains(&key) {
                a2_seen.insert(key);
            }
        }
    }

    let examples: Vec<&str> = s
        .skills()
        .flat_map(|(_, skill)| skill.examples.iter().map(|e| e.target.as_str()))
        .collect();

    let (attested_share, a1_rank_le_2000_share) = match pack {
        Some(pack) => {
            let attested = examples
                .iter()
                .filter(|target| {
                    let tokens = tokenise(s.lang, target, forms);
                    let norm: Vec<&str> = tokens.iter().map(|t| t.norm.as_str()).collect();
                    pack.norms.contains(&norm.join(" "))
                })
                .count();
            let frequent = a1_single
                .iter()
                .filter(|lemma| {
                    pack_rank(&pack.freq, lemma).is_some_and(|rank| rank <= A1_RANK_TARGET)
                })
                .count();
            (
                Some(share(attested, examples.len())),
                Some(share(frequent, a1_single.len())),
            )
        }
        None => (None, None),
    };

    CourseReport {
        lang: s.lang,
        units,
        skills: s.skills().count(),
        skills_per_unit,
        lemmas: LevelCounts {
            a1: a1_lemmas.len(),
            a2: a2_seen.len(),
        },
        examples: examples.len(),
        attested_share,
        a1_rank_le_2000_share,
        checklist_uncovered: checklist.map(Checklist::uncovered).unwrap_or_default(),
        unreviewed_units: match review {
            Some(review) => review.unreviewed_units(s),
            None => s.units.iter().map(|unit| unit.id.clone()).collect(),
        },
    }
}

/// A lemma whose `rank` is not the pack's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RankDiffers {
    pub skill: String,
    pub lemma: String,
    pub rank: Option<u32>,
    pub pack_rank: u64,
}

/// A lemma the pack's frequency table does not have.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RankMissing {
    pub skill: String,
    pub lemma: String,
}

/// What `parley freq check` prints.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FreqCheck {
    pub lang: Lang,
    /// The single-word lemma entries checked.
    pub lemmas: usize,
    pub differs: Vec<RankDiffers>,
    pub missing: Vec<RankMissing>,
}

impl FreqCheck {
    /// Every rank agrees with the pack.
    pub fn ok(&self) -> bool {
        self.differs.is_empty() && self.missing.is_empty()
    }
}

/// Compares the `rank` of every single-word lemma entry, in document order, with the pack's
/// `freq` table: a lemma the table ranks differently (a null `rank` included) differs; one it
/// does not have is missing. Ranks are reported, never filled in.
pub fn freq_check(s: &Syllabus, freq: &HashMap<String, u64>) -> FreqCheck {
    let mut lemmas = 0;
    let mut differs = Vec::new();
    let mut missing = Vec::new();
    for (_, skill) in s.skills() {
        for lemma in skill.lemmas.iter().filter(|lemma| single_word(lemma)) {
            lemmas += 1;
            match pack_rank(freq, &lemma.lemma) {
                Some(rank) if lemma.rank.map(u64::from) == Some(rank) => {}
                Some(rank) => differs.push(RankDiffers {
                    skill: skill.id.clone(),
                    lemma: lemma.lemma.clone(),
                    rank: lemma.rank,
                    pack_rank: rank,
                }),
                None => missing.push(RankMissing {
                    skill: skill.id.clone(),
                    lemma: lemma.lemma.clone(),
                }),
            }
        }
    }
    FreqCheck {
        lang: s.lang,
        lemmas,
        differs,
        missing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::parse_syllabus;
    use crate::content::test_support::{EXAMPLE_SYLLABUS, example_forms};

    fn syllabus() -> Syllabus {
        parse_syllabus("s.yaml", EXAMPLE_SYLLABUS).unwrap()
    }

    #[test]
    fn shares_round_half_up_to_four_places() {
        assert_eq!(share(0, 0), 0.0);
        assert_eq!(share(1, 3), 0.3333);
        assert_eq!(share(2, 3), 0.6667);
        assert_eq!(share(1, 8), 0.125);
        assert_eq!(share(1, 20_000), 0.0001);
        assert_eq!(share(5, 5), 1.0);
    }

    #[test]
    fn the_report_counts_the_course_without_a_pack() {
        let s = syllabus();
        let report = course_report(&s, &example_forms(), None, None, None);
        assert_eq!(report.units, LevelCounts { a1: 1, a2: 0 });
        assert_eq!(report.skills, 1);
        assert_eq!(report.skills_per_unit, SkillsPerUnit { min: 1, max: 1 });
        assert_eq!(report.lemmas, LevelCounts { a1: 7, a2: 0 });
        assert_eq!(report.examples, 4);
        assert_eq!(report.attested_share, None);
        assert_eq!(report.a1_rank_le_2000_share, None);
        assert!(report.checklist_uncovered.is_empty());
        assert_eq!(report.unreviewed_units, ["es.a1.u01"]);
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "lang": "es", "units": {"A1": 1, "A2": 0}, "skills": 1,
                "skills_per_unit": {"min": 1, "max": 1}, "lemmas": {"A1": 7, "A2": 0},
                "examples": 4, "attested_share": null, "a1_rank_le_2000_share": null,
                "checklist_uncovered": [], "unreviewed_units": ["es.a1.u01"]
            })
        );
    }

    #[test]
    fn a2_lemmas_leave_out_those_taught_at_a1_and_units_are_counted_per_level() {
        let mut s = syllabus();
        let mut a2 = s.units[0].clone();
        a2.id = "es.a2.u01".into();
        a2.cefr = Cefr::A2;
        a2.skills[0].cefr = Cefr::A2;
        a2.skills[0].lemmas.truncate(2);
        a2.skills[0].lemmas[1].lemma = "mañana".into();
        let mut extra = a2.skills[0].clone();
        extra.id = "es.a2.u01.s02".into();
        a2.skills.push(extra);
        s.units.push(a2);
        let report = course_report(&s, &Forms::default(), None, None, None);
        assert_eq!(report.units, LevelCounts { a1: 1, a2: 1 });
        assert_eq!(report.skills, 3);
        assert_eq!(report.skills_per_unit, SkillsPerUnit { min: 1, max: 2 });
        assert_eq!(
            report.lemmas,
            LevelCounts { a1: 7, a2: 1 },
            "hola is A1; mañana is new"
        );
        assert_eq!(report.examples, 12);
    }

    #[test]
    fn the_pack_gives_both_shares() {
        let s = syllabus();
        let mut pack = PackStats::default();
        // Examples 1 and 3 normalised as the pack stores them; 2 and 4 are not in the pack.
        pack.norms.insert("hola qué tal".into());
        pack.norms.insert("buenas noches ana".into());
        pack.norms.insert("adiós".into());
        // Seven A1 lemmas, all single words: three within 2,000, one above, three absent.
        for (lemma, rank) in [("hola", 5), ("bueno", 2_000), ("día", 40), ("noche", 2_001)] {
            pack.freq.insert(lemma.into(), rank);
        }
        let report = course_report(&s, &example_forms(), None, None, Some(&pack));
        assert_eq!(report.attested_share, Some(0.5));
        assert_eq!(report.a1_rank_le_2000_share, Some(0.4286));
    }

    #[test]
    fn freq_check_lists_differences_and_missing_lemmas_in_document_order() {
        let text = EXAMPLE_SYLLABUS
            .replace(
                "{lemma: hola, pos: intj, en: [hello, hi], rank: null}",
                "{lemma: hola, pos: intj, en: [hello, hi], rank: 5}",
            )
            .replace(
                "{lemma: bueno, pos: adj, en: [good], rank: null}",
                "{lemma: bueno, pos: adj, en: [good], rank: 9}",
            )
            .replace(
                "{lemma: gracias, pos: intj, en: [thank you, thanks], rank: null}",
                "{lemma: de nada, pos: expr, en: [you're welcome], rank: null}",
            );
        let s = parse_syllabus("s.yaml", &text).unwrap();
        let freq: HashMap<String, u64> = [("hola", 5), ("bueno", 3), ("día", 40), ("tal", 7)]
            .into_iter()
            .map(|(l, r)| (l.to_string(), r))
            .collect();
        let check = freq_check(&s, &freq);
        assert!(!check.ok());
        assert_eq!(
            serde_json::to_value(&check).unwrap(),
            serde_json::json!({
                "lang": "es", "lemmas": 6,
                "differs": [
                    {"skill": "es.a1.u01.s01", "lemma": "bueno", "rank": 9, "pack_rank": 3},
                    {"skill": "es.a1.u01.s01", "lemma": "día", "rank": null, "pack_rank": 40},
                    {"skill": "es.a1.u01.s01", "lemma": "tal", "rank": null, "pack_rank": 7}
                ],
                "missing": [
                    {"skill": "es.a1.u01.s01", "lemma": "adiós"},
                    {"skill": "es.a1.u01.s01", "lemma": "noche"}
                ]
            })
        );
    }

    #[test]
    fn pack_stats_come_from_the_sentences_and_freq_tables() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("es.sqlite3");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE sentences (id INTEGER PRIMARY KEY, text TEXT, norm TEXT, tokens INTEGER,
               max_rank INTEGER, author TEXT);
             CREATE TABLE freq (lemma TEXT PRIMARY KEY, rank INTEGER, count REAL);
             INSERT INTO sentences VALUES (1, 'Hola, ¿qué tal?', 'hola qué tal', 3, 2, 'a');
             INSERT INTO freq VALUES ('hola', 1, 1.0);
             INSERT INTO freq VALUES ('tal', 2, 1.0);",
        )
        .unwrap();
        drop(conn);
        let stats = read_pack_stats(&path).unwrap();
        assert!(stats.norms.contains("hola qué tal"));
        assert_eq!(stats.freq["tal"], 2);
        assert_eq!(
            read_pack_stats(Path::new("no/such/pack.sqlite3")).unwrap_err(),
            PackError::NotFound("no/such/pack.sqlite3".into())
        );
        let not_a_pack = dir.path().join("x.sqlite3");
        std::fs::write(&not_a_pack, b"not a database at all, just some text").unwrap();
        assert!(matches!(
            read_pack_stats(&not_a_pack).unwrap_err(),
            PackError::Sqlite(_)
        ));
    }
}
