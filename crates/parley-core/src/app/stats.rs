//! `stats_get` (docs/modules/04-app-shell-and-offline-course.md, Design notes).
//!
//! Streak, freezes, the 30-day calendar and weekly XP come from the `daily` record and cover
//! every course; words known, skills, the CEFR estimate and the recall rate are the language's.

use std::collections::BTreeMap;

use super::{Parley, not_started, open_store};
use crate::api::{SkillProgress, Stats, StatsDay};
use crate::env::{LocalDate, local_date};
use crate::error::AppError;
use crate::game::streak;
use crate::srs::{DAY_MS, retrievability, skill_mastery, skill_state};

/// The days the calendar shows, ending today.
const CALENDAR_DAYS: i64 = 30;
/// A vocabulary item is known when its productive card's retrievability is at least this.
const KNOWN_AT: f64 = 0.9;
/// The recall rate looks back this many days.
const RECALL_DAYS: i64 = 30;

impl Parley {
    pub fn stats_get(&mut self, lang: &str) -> Result<Stats, AppError> {
        let lang = self.lang(lang)?;
        let store = open_store(&mut self.store)?;
        let Some(course) = store.course(lang)? else {
            return Err(not_started());
        };
        let clock = &*self.env.clock;
        let now = clock.now_ms();
        let today = local_date(clock);
        let daily = store.daily()?;
        let state = streak(&daily, today);

        let by_date: BTreeMap<LocalDate, (u32, bool, bool)> = daily
            .iter()
            .map(|row| (row.local_date, (row.xp, row.sessions > 0, row.frozen)))
            .collect();
        let days = ((today.days() - (CALENDAR_DAYS - 1))..=today.days())
            .map(|day| {
                let date = LocalDate::from_days(day);
                let (xp, active, frozen) = by_date.get(&date).copied().unwrap_or_default();
                StatsDay {
                    local_date: date,
                    xp,
                    active,
                    frozen,
                }
            })
            .collect();

        let this_week = today.iso_week();
        let mut weeks: BTreeMap<(i32, u32), u32> = BTreeMap::new();
        for row in &daily {
            *weeks.entry(row.local_date.iso_week()).or_default() += row.xp;
        }
        let week_xp = weeks.get(&this_week).copied().unwrap_or(0);
        let record_week_xp = weeks
            .range(..this_week)
            .map(|(_, xp)| *xp)
            .max()
            .unwrap_or(0);

        let words_known = store
            .vocab_productive_cards(lang)?
            .iter()
            .filter(|card| retrievability(card, now) >= KNOWN_AT)
            .count() as u32;
        let mut skills = Vec::new();
        for skill in store.skills(lang)? {
            let info = store.skill_info(&skill.id)?;
            skills.push(SkillProgress {
                id: skill.id.clone(),
                title: info.title,
                state: skill_state(store, &skill.id, now)?,
                mastery: skill_mastery(store, &skill.id, now)?,
            });
        }
        let reviews = store.review_counts(lang, now - RECALL_DAYS * DAY_MS, now)?;
        let recall_rate =
            (reviews.reviews > 0).then(|| f64::from(reviews.recalled) / f64::from(reviews.reviews));

        Ok(Stats {
            lang: lang.as_str().to_string(),
            empty: !store.has_finished_session(lang)?,
            streak: state.length,
            freezes: state.freezes,
            days,
            week_xp,
            record_week_xp,
            words_known,
            vocab_items: store.vocab_item_count(lang)?,
            skills,
            cefr_estimate: course.cefr_estimate,
            recall_rate,
            reviews_30: reviews.reviews,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::parley;
    use crate::grading::Answer;

    #[test]
    fn stats_start_empty_and_count_a_finished_lesson() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = parley(dir.path());
        assert_eq!(
            app.stats_get("es").unwrap_err().message,
            "This course has not been started."
        );
        app.course_start("es").unwrap();
        let stats = app.stats_get("es").unwrap();
        assert!(stats.empty);
        assert_eq!(stats.days.len(), 30);
        assert_eq!(stats.days[29].local_date.to_string(), "2026-01-05");
        assert_eq!((stats.recall_rate, stats.reviews_30), (None, 0));
        assert_eq!(stats.vocab_items, 7);
        let started = app.lesson_start("es.a1.u01.s01", "learn").unwrap();
        app.exercise_submit(
            &started.session_id,
            &started.exercises[0].id,
            &Answer::Choice(0),
            Some(4_000),
            false,
        )
        .unwrap();
        let summary = app.lesson_finish(&started.session_id).unwrap();
        let stats = app.stats_get("es").unwrap();
        assert!(!stats.empty);
        assert_eq!(stats.streak, 1);
        assert_eq!(stats.week_xp, summary.xp);
        assert_eq!(stats.record_week_xp, 0);
        assert!(stats.days[29].active);
        assert_eq!(stats.days[29].xp, summary.xp);
        assert_eq!(stats.reviews_30, 1);
        assert!(stats.recall_rate.is_some());
        assert_eq!(stats.skills.len(), 1);
    }
}
