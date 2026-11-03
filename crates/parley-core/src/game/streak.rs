//! Streaks, earned freezes and the weekly record (docs/modules/03-exercise-engine-and-grading.md,
//! Design notes).
//!
//! Everything here is a pure function of the `daily` rows: the streak is recomputed from the
//! days the learner was active, so the same days always give the same streak, the same frozen
//! days and the same freeze events. Freezes are only earned, one per 7 active days with at most
//! 2 banked (invariant I7); nothing here can add one any other way.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::env::LocalDate;

/// One row of the `daily` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DayRow {
    pub local_date: LocalDate,
    pub xp: u32,
    pub sessions: u32,
    pub goal_xp: u32,
    /// The day was missed and a banked freeze kept the streak.
    pub frozen: bool,
}

/// Whether a freeze was earned or used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FreezeKind {
    Earned,
    Used,
}

impl FreezeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FreezeKind::Earned => "earned",
            FreezeKind::Used => "used",
        }
    }
}

/// A freeze earned or used on a day, with the streak length at that moment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FreezeEvent {
    pub local_date: LocalDate,
    pub kind: FreezeKind,
    pub streak_len: u32,
}

/// The streak as of a day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StreakState {
    pub length: u32,
    /// Freezes banked, 0 to 2.
    pub freezes: u32,
    pub active_today: bool,
    /// The missed days a freeze covered, in order.
    pub frozen: Vec<LocalDate>,
    /// Every freeze earned or used, in order.
    pub events: Vec<FreezeEvent>,
}

/// A freeze is earned at every multiple of this many streak days.
const FREEZE_EVERY: u32 = 7;
/// The most freezes a learner can bank.
const MAX_FREEZES: u32 = 2;

/// The message shown once a week, when this week's XP first exceeds the best earlier week.
pub const WEEKLY_RECORD: &str = "New weekly record!";

/// The streak on `today`, processed day by day from the first active day up to yesterday, then
/// today if active (a day is active when its row has sessions > 0): an active day adds 1, and at
/// lengths 7, 14, 21… a freeze is earned if fewer than 2 are banked; a missed day uses a banked
/// freeze (length unchanged, the day frozen) or resets the length to 0. Today, not yet active,
/// keeps yesterday's streak. Rows after today are ignored.
pub fn streak(daily: &[DayRow], today: LocalDate) -> StreakState {
    let active: BTreeSet<i64> = daily
        .iter()
        .filter(|row| row.sessions > 0 && row.local_date <= today)
        .map(|row| row.local_date.days())
        .collect();
    let mut state = StreakState {
        length: 0,
        freezes: 0,
        active_today: active.contains(&today.days()),
        frozen: Vec::new(),
        events: Vec::new(),
    };
    let Some(&first) = active.first() else {
        return state;
    };
    let today_days = today.days();
    for day in first..today_days {
        if active.contains(&day) {
            on_active(&mut state, day);
        } else if state.freezes > 0 {
            state.freezes -= 1;
            let date = LocalDate::from_days(day);
            state.frozen.push(date);
            state.events.push(FreezeEvent {
                local_date: date,
                kind: FreezeKind::Used,
                streak_len: state.length,
            });
        } else {
            state.length = 0;
        }
    }
    if state.active_today {
        on_active(&mut state, today_days);
    }
    state
}

/// An active day: the streak grows by one and, at every multiple of 7, earns a freeze unless 2
/// are banked.
fn on_active(state: &mut StreakState, day: i64) {
    state.length += 1;
    if state.length.is_multiple_of(FREEZE_EVERY) && state.freezes < MAX_FREEZES {
        state.freezes += 1;
        state.events.push(FreezeEvent {
            local_date: LocalDate::from_days(day),
            kind: FreezeKind::Earned,
            streak_len: state.length,
        });
    }
}

/// Whether a session of `session_xp` on `today` sets a weekly record (ISO weeks on local dates):
/// the best earlier week's XP is above 0, this week's XP before the session is at most that, and
/// this week's XP after it is above it. So the message appears once a week, when this week first
/// passes the best earlier week. Rows after this week are ignored.
pub fn weekly_record(daily: &[DayRow], today: LocalDate, session_xp: u32) -> bool {
    let this_week = today.iso_week();
    let mut weeks: BTreeMap<(i32, u32), u64> = BTreeMap::new();
    for row in daily {
        *weeks.entry(row.local_date.iso_week()).or_default() += u64::from(row.xp);
    }
    let best = weeks
        .range(..this_week)
        .map(|(_, xp)| *xp)
        .max()
        .unwrap_or(0);
    let before = weeks.get(&this_week).copied().unwrap_or(0);
    let after = before + u64::from(session_xp);
    best > 0 && before <= best && after > best
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Day 1 is 2026-01-05, a Monday.
    fn day(n: i64) -> LocalDate {
        LocalDate::from_days(LocalDate::parse("2026-01-04").unwrap().days() + n)
    }

    fn active(days: impl IntoIterator<Item = i64>) -> Vec<DayRow> {
        days.into_iter()
            .map(|n| DayRow {
                local_date: day(n),
                xp: 10,
                sessions: 1,
                goal_xp: 20,
                frozen: false,
            })
            .collect()
    }

    #[test]
    fn seven_active_days_earn_a_freeze() {
        let state = streak(&active(1..=7), day(7));
        assert_eq!(
            (state.length, state.freezes, state.active_today),
            (7, 1, true)
        );
        assert_eq!(
            state.events,
            [FreezeEvent {
                local_date: day(7),
                kind: FreezeKind::Earned,
                streak_len: 7
            }]
        );
        let tomorrow = streak(&active(1..=7), day(8));
        assert_eq!((tomorrow.length, tomorrow.active_today), (7, false));
    }

    #[test]
    fn a_missed_day_uses_a_banked_freeze() {
        let mut days: Vec<i64> = (1..=7).collect();
        days.push(9);
        let state = streak(&active(days), day(9));
        assert_eq!((state.length, state.freezes), (8, 0));
        assert_eq!(state.frozen, [day(8)]);
        assert_eq!(state.events[1].kind, FreezeKind::Used);
        assert_eq!(state.events[1].streak_len, 7);
    }

    #[test]
    fn at_most_two_freezes_are_banked() {
        let state = streak(&active(1..=21), day(21));
        assert_eq!((state.length, state.freezes), (21, 2));
        let earned: Vec<u32> = state.events.iter().map(|e| e.streak_len).collect();
        assert_eq!(earned, [7, 14]);
    }

    #[test]
    fn two_missed_days_with_one_freeze_reset_the_streak() {
        let mut days: Vec<i64> = (1..=7).collect();
        days.push(10);
        let state = streak(&active(days), day(10));
        assert_eq!((state.length, state.freezes), (1, 0));
        assert_eq!(state.frozen, [day(8)]);
    }

    #[test]
    fn no_activity_is_no_streak() {
        let state = streak(&[], day(3));
        assert_eq!(state.length, 0);
        let mut rows = active([1]);
        rows[0].sessions = 0;
        assert_eq!(streak(&rows, day(1)).length, 0);
        assert_eq!(
            streak(&active([5]), day(3)).length,
            0,
            "future rows are ignored"
        );
    }

    fn week_rows(xp: &[(i64, u32)]) -> Vec<DayRow> {
        xp.iter()
            .map(|&(n, xp)| DayRow {
                local_date: day(n),
                xp,
                sessions: 1,
                goal_xp: 20,
                frozen: false,
            })
            .collect()
    }

    #[test]
    fn a_weekly_record_is_announced_once() {
        // Week 1 (days 1 to 7) 100 XP, week 2 80 XP; in week 3 sessions of 60, 41 and 30 XP.
        let mut rows = week_rows(&[(1, 100), (8, 80)]);
        assert!(!weekly_record(&rows, day(15), 60));
        rows.extend(week_rows(&[(15, 60)]));
        assert!(weekly_record(&rows, day(16), 41));
        rows.extend(week_rows(&[(16, 41)]));
        assert!(!weekly_record(&rows, day(17), 30));
        assert!(!weekly_record(&[], day(1), 50), "no earlier week");
        assert!(
            !weekly_record(&week_rows(&[(1, 100)]), day(2), 50),
            "same week"
        );
    }
}
