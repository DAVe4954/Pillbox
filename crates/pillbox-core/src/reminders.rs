//! Reminders: the stored list, the quick-add text parser ("call mum at 18:30", "in 10m tea",
//! "weekdays 9:00 stand-up") and repeat arithmetic.

use serde::{Deserialize, Serialize};

use crate::calendar::{self, Date, Minutes, WEEKDAY_NAMES, minutes, split};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Repeat {
    #[default]
    Once,
    Daily,
    /// Monday to Friday.
    Weekdays,
    Weekly,
}

impl Repeat {
    pub fn label(self) -> &'static str {
        match self {
            Repeat::Once => "",
            Repeat::Daily => "daily",
            Repeat::Weekdays => "weekdays",
            Repeat::Weekly => "weekly",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reminder {
    pub id: u64,
    pub text: String,
    /// When it is (next) due, local minutes.
    pub due: Minutes,
    pub repeat: Repeat,
    /// Set by Snooze: rings again at this time instead of `due`.
    #[serde(default)]
    pub snoozed: Option<Minutes>,
}

impl Reminder {
    /// The time it will ring.
    pub fn rings_at(&self) -> Minutes {
        self.snoozed.unwrap_or(self.due)
    }
}

/// The next time after `after` that a repeating reminder falls on, keeping its time of day.
/// `None` for a one-off.
pub fn next_due(due: Minutes, repeat: Repeat, after: Minutes) -> Option<Minutes> {
    let (date, hour, minute) = split(due);
    let mut day = date;
    let fits = |d: Date| match repeat {
        Repeat::Once => false,
        Repeat::Daily => true,
        Repeat::Weekdays => (1..=5).contains(&d.weekday()),
        Repeat::Weekly => d.weekday() == date.weekday(),
    };
    if repeat == Repeat::Once {
        return None;
    }
    // Jump close to `after` first so a reminder that was off for months does not loop long.
    let (after_date, _, _) = split(after);
    if after_date > day {
        let gap = calendar::days_from_civil(after_date) - calendar::days_from_civil(day);
        let jump = match repeat {
            Repeat::Weekly => (gap / 7) * 7,
            _ => gap,
        };
        day = day.add_days(jump - 1).max(date);
    }
    for _ in 0..=14 {
        day = day.add_days(1);
        let at = minutes(day, hour, minute);
        if fits(day) && at > after {
            return Some(at);
        }
    }
    None
}

/// The list of reminders as stored in `reminders.json`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Reminders {
    pub next_id: u64,
    pub items: Vec<Reminder>,
}

impl Reminders {
    pub fn add(&mut self, text: String, due: Minutes, repeat: Repeat) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.items.push(Reminder {
            id,
            text,
            due,
            repeat,
            snoozed: None,
        });
        self.sort();
        id
    }

    fn sort(&mut self) {
        self.items.sort_by_key(|r| (r.rings_at(), r.id));
    }

    /// Answer "Done": a one-off is removed, a repeating one moves to its next day.
    pub fn done(&mut self, id: u64, now: Minutes) {
        let Some(i) = self.items.iter().position(|r| r.id == id) else {
            return;
        };
        let r = &mut self.items[i];
        match next_due(r.due, r.repeat, now.max(r.due)) {
            Some(next) => {
                r.due = next;
                r.snoozed = None;
            }
            None => {
                self.items.remove(i);
            }
        }
        self.sort();
    }

    pub fn snooze(&mut self, id: u64, until: Minutes) {
        if let Some(r) = self.items.iter_mut().find(|r| r.id == id) {
            r.snoozed = Some(until);
        }
        self.sort();
    }

    pub fn remove(&mut self, id: u64) {
        self.items.retain(|r| r.id != id);
    }

    /// Reminders that are ringing at `now`, oldest first.
    pub fn due(&self, now: Minutes) -> impl Iterator<Item = &Reminder> {
        self.items.iter().filter(move |r| r.rings_at() <= now)
    }

    /// The next time anything rings after `now`, to set a timer for.
    pub fn next_ring_after(&self, now: Minutes) -> Option<Minutes> {
        self.items
            .iter()
            .map(Reminder::rings_at)
            .filter(|&m| m > now)
            .min()
    }
}

/// What the quick-add box understood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub text: String,
    pub due: Minutes,
    pub repeat: Repeat,
}

/// A time of day: "18:30", "6pm", "6:15pm", "9am", "9.30". Returns (hour, minute).
fn parse_clock(word: &str) -> Option<(u32, u32)> {
    let w = word.to_ascii_lowercase();
    let (body, pm) = if let Some(b) = w.strip_suffix("pm") {
        (b, Some(true))
    } else if let Some(b) = w.strip_suffix("am") {
        (b, Some(false))
    } else {
        (w.as_str(), None)
    };
    let (h, m) = match body.split_once([':', '.']) {
        Some((h, m)) if m.len() == 2 => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None if pm.is_some() => (body.parse::<u32>().ok()?, 0),
        _ => return None,
    };
    if m > 59 {
        return None;
    }
    let h = match pm {
        Some(pm) if (1..=12).contains(&h) => (h % 12) + if pm { 12 } else { 0 },
        Some(_) => return None,
        None if h <= 23 => h,
        None => return None,
    };
    Some((h, m))
}

/// A duration: "10m", "10min", "2h", "1h30m", "90s" is not supported (too short to matter).
fn parse_span(word: &str) -> Option<i64> {
    let w = word.to_ascii_lowercase();
    let mut total = 0i64;
    let mut num = String::new();
    let mut any = false;
    let mut chars = w.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            num.push(c);
            continue;
        }
        let mut unit = c.to_string();
        while let Some(&n) = chars.peek() {
            if n.is_ascii_alphabetic() {
                unit.push(n);
                chars.next();
            } else {
                break;
            }
        }
        let n: i64 = num.parse().ok()?;
        num.clear();
        total += match unit.as_str() {
            "m" | "min" | "mins" | "minute" | "minutes" => n,
            "h" | "hr" | "hrs" | "hour" | "hours" => n * 60,
            "d" | "day" | "days" => n * 1440,
            _ => return None,
        };
        any = true;
    }
    (num.is_empty() && any && total > 0).then_some(total)
}

fn weekday_index(word: &str) -> Option<u32> {
    let w = word.to_ascii_lowercase();
    if w.len() < 3 {
        return None;
    }
    WEEKDAY_NAMES
        .iter()
        .position(|name| {
            name.starts_with(&w) || (w.ends_with('s') && name.starts_with(&w[..w.len() - 1]))
        })
        .map(|i| i as u32)
}

/// Understand quick-add text, relative to `now` (local minutes).
pub fn parse_quick(input: &str, now: Minutes) -> Result<Parsed, String> {
    let words: Vec<&str> = input.split_whitespace().collect();
    let (today, _, _) = split(now);
    let mut text: Vec<&str> = Vec::new();
    let mut clock: Option<(u32, u32)> = None;
    let mut span: Option<i64> = None;
    let mut day: Option<Date> = None;
    let mut weekday: Option<u32> = None;
    let mut repeat = Repeat::Once;
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        let lower = w.to_ascii_lowercase();
        let lower = lower.trim_end_matches([',', '.', '!']);
        let next = words.get(i + 1).map(|n| n.to_ascii_lowercase());
        match lower {
            "in" if next.as_deref().is_some_and(|n| parse_span(n).is_some()) => {
                span = next.as_deref().and_then(parse_span);
                i += 2;
                continue;
            }
            "in" if next.as_deref().is_some_and(|n| n.parse::<i64>().is_ok())
                && words
                    .get(i + 2)
                    .is_some_and(|u| parse_span(&format!("1{u}")).is_some()) =>
            {
                // "in 10 minutes"
                let n: i64 = next.as_deref().and_then(|n| n.parse().ok()).unwrap_or(0);
                let unit = parse_span(&format!("1{}", words[i + 2])).unwrap_or(1);
                span = Some(n * unit);
                i += 3;
                continue;
            }
            "at" if next.as_deref().and_then(parse_clock).is_some() => {
                clock = next.as_deref().and_then(parse_clock);
                i += 2;
                continue;
            }
            "today" => day = Some(today),
            "tomorrow" => day = Some(today.add_days(1)),
            "daily" | "everyday" => repeat = Repeat::Daily,
            "weekdays" => repeat = Repeat::Weekdays,
            "weekly" => repeat = Repeat::Weekly,
            "every" => match next.as_deref() {
                Some("day") => {
                    repeat = Repeat::Daily;
                    i += 2;
                    continue;
                }
                Some("weekday") | Some("weekdays") => {
                    repeat = Repeat::Weekdays;
                    i += 2;
                    continue;
                }
                Some("week") => {
                    repeat = Repeat::Weekly;
                    i += 2;
                    continue;
                }
                Some(n) if weekday_index(n).is_some() => {
                    repeat = Repeat::Weekly;
                    weekday = weekday_index(n);
                    i += 2;
                    continue;
                }
                _ => text.push(w),
            },
            "on" if next.as_deref().and_then(weekday_index).is_some() => {
                weekday = next.as_deref().and_then(weekday_index);
                i += 2;
                continue;
            }
            _ => {
                if let Some(c) = parse_clock(lower) {
                    clock = Some(c);
                } else if let Some(d) = calendar::parse_date(lower) {
                    day = Some(d);
                } else if let Some(wd) =
                    weekday_index(lower).filter(|_| lower.len() >= 3 && is_day_word(lower))
                {
                    weekday = Some(wd);
                } else {
                    text.push(w);
                }
            }
        }
        i += 1;
    }
    let text = text.join(" ").trim().to_string();
    if text.is_empty() {
        return Err("What should I remind you of?".into());
    }
    if let Some(span) = span {
        if clock.is_some() || day.is_some() || weekday.is_some() {
            return Err("Use either “in …” or a time, not both".into());
        }
        return Ok(Parsed {
            text,
            due: now + span,
            repeat,
        });
    }
    if clock.is_none() && day.is_none() && weekday.is_none() && repeat == Repeat::Once {
        return Err("Say when, e.g. “in 10m”, “at 15:00” or “tomorrow 9am”".into());
    }
    let (hour, minute) = clock.unwrap_or((9, 0));
    let mut date = match (day, weekday) {
        (Some(d), _) => d,
        (None, Some(wd)) => {
            let ahead = (i64::from(wd) - i64::from(today.weekday())).rem_euclid(7);
            today.add_days(ahead)
        }
        (None, None) => today,
    };
    let mut due = minutes(date, hour, minute);
    // A time that already passed today means the next one.
    if due <= now {
        let step = if weekday.is_some() && day.is_none() {
            7
        } else {
            1
        };
        if day.is_some() && repeat == Repeat::Once {
            return Err("That time has already passed".into());
        }
        date = date.add_days(step);
        due = minutes(date, hour, minute);
    }
    // Repeats start on the first day that fits.
    if repeat == Repeat::Weekdays && !(1..=5).contains(&date.weekday()) {
        due = next_due(due, repeat, due).unwrap_or(due);
    }
    Ok(Parsed { text, due, repeat })
}

/// A bare weekday must be spelled out ("friday", "fridays"); short forms such as "sat" only
/// count after "on" or "every", because they are also ordinary words.
fn is_day_word(w: &str) -> bool {
    WEEKDAY_NAMES
        .iter()
        .any(|n| *n == w || format!("{n}s") == w)
}

/// "Today 18:30", "Tomorrow 9:00", "Fri 9:00" or "24 Dec 9:00", relative to `now`.
pub fn describe(at: Minutes, now: Minutes, clock_24h: bool) -> String {
    let (date, h, m) = split(at);
    let (today, _, _) = split(now);
    let time = if clock_24h {
        format!("{h}:{m:02}")
    } else {
        let h12 = match h % 12 {
            0 => 12,
            x => x,
        };
        format!("{h12}:{m:02} {}", if h < 12 { "AM" } else { "PM" })
    };
    let days = calendar::days_from_civil(date) - calendar::days_from_civil(today);
    let day = match days {
        0 => "Today".to_string(),
        1 => "Tomorrow".to_string(),
        2..=6 => {
            let name = WEEKDAY_NAMES[date.weekday() as usize];
            let mut s = name[..3].to_string();
            if let Some(f) = s.get_mut(0..1) {
                f.make_ascii_uppercase();
            }
            s
        }
        _ => {
            const MONTHS: [&str; 12] = [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ];
            format!(
                "{} {}",
                date.day,
                MONTHS.get(date.month as usize - 1).copied().unwrap_or("")
            )
        }
    };
    format!("{day} {time}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Monday 16 Nov 2026, 14:00.
    fn now() -> Minutes {
        minutes(
            Date {
                year: 2026,
                month: 11,
                day: 16,
            },
            14,
            0,
        )
    }

    fn at(day: u32, h: u32, m: u32) -> Minutes {
        minutes(
            Date {
                year: 2026,
                month: 11,
                day,
            },
            h,
            m,
        )
    }

    fn ok(text: &str) -> Parsed {
        match parse_quick(text, now()) {
            Ok(p) => p,
            Err(e) => panic!("{text}: {e}"),
        }
    }

    #[test]
    fn relative_times() {
        let p = ok("in 10m tea");
        assert_eq!(
            (p.text.as_str(), p.due, p.repeat),
            ("tea", now() + 10, Repeat::Once)
        );
        assert_eq!(ok("stretch in 1h30m").due, now() + 90);
        assert_eq!(ok("in 2 hours call back").due, now() + 120);
        assert_eq!(ok("check oven in 25 minutes").due, now() + 25);
    }

    #[test]
    fn clock_times() {
        let p = ok("call mum at 18:30");
        assert_eq!((p.text.as_str(), p.due), ("call mum", at(16, 18, 30)));
        assert_eq!(ok("dinner 7pm").due, at(16, 19, 0));
        assert_eq!(ok("pills at 6:15pm").due, at(16, 18, 15));
        // Already past today: tomorrow.
        assert_eq!(ok("standup 9:00").due, at(17, 9, 0));
        assert_eq!(ok("lunch 12am").due, at(17, 0, 0));
    }

    #[test]
    fn days() {
        assert_eq!(ok("tomorrow dentist").due, at(17, 9, 0), "no time = 9:00");
        assert_eq!(ok("tomorrow 15:00 dentist").text, "dentist");
        assert_eq!(ok("bins friday 7am").due, at(20, 7, 0));
        assert_eq!(
            ok("on mon review").due,
            at(23, 9, 0),
            "Monday 9:00 has passed: next week"
        );
        assert_eq!(
            ok("party 2026-12-24 20:00").due,
            minutes(
                Date {
                    year: 2026,
                    month: 12,
                    day: 24
                },
                20,
                0
            )
        );
        assert!(parse_quick("old 2026-11-01 10:00", now()).is_err());
    }

    #[test]
    fn repeats() {
        let p = ok("daily 8:00 meds");
        assert_eq!((p.repeat, p.due), (Repeat::Daily, at(17, 8, 0)));
        let p = ok("weekdays 9:30 standup");
        assert_eq!((p.repeat, p.due), (Repeat::Weekdays, at(17, 9, 30)));
        let p = ok("every friday 16:00 timesheet");
        assert_eq!(
            (p.repeat, p.due, p.text.as_str()),
            (Repeat::Weekly, at(20, 16, 0), "timesheet")
        );
        assert_eq!(ok("water plants every day at 19:00").repeat, Repeat::Daily);
    }

    #[test]
    fn refuses_unclear_input() {
        assert!(parse_quick("buy milk", now()).is_err());
        assert!(parse_quick("at 18:00", now()).is_err());
        assert!(parse_quick("", now()).is_err());
        assert!(parse_quick("tea in 10m at 18:00", now()).is_err());
    }

    #[test]
    fn words_that_look_like_days_stay_text() {
        assert_eq!(ok("sat nav update at 18:00").text, "sat nav update");
        assert_eq!(ok("wedding card at 18:00").text, "wedding card");
    }

    #[test]
    fn next_due_follows_the_repeat() {
        let mon_9 = at(16, 9, 0);
        assert_eq!(next_due(mon_9, Repeat::Once, mon_9), None);
        assert_eq!(next_due(mon_9, Repeat::Daily, mon_9), Some(at(17, 9, 0)));
        assert_eq!(next_due(mon_9, Repeat::Weekly, mon_9), Some(at(23, 9, 0)));
        let fri_9 = at(20, 9, 0);
        assert_eq!(next_due(fri_9, Repeat::Weekdays, fri_9), Some(at(23, 9, 0)));
        // Long gaps skip ahead.
        let later = minutes(
            Date {
                year: 2027,
                month: 3,
                day: 3,
            },
            12,
            0,
        );
        let n = next_due(mon_9, Repeat::Weekly, later);
        assert_eq!(
            n,
            Some(minutes(
                Date {
                    year: 2027,
                    month: 3,
                    day: 8
                },
                9,
                0
            ))
        );
    }

    #[test]
    fn list_operations() {
        let mut list = Reminders::default();
        let a = list.add("a".into(), at(16, 15, 0), Repeat::Once);
        let b = list.add("b".into(), at(16, 14, 30), Repeat::Daily);
        assert_eq!(list.items[0].id, b, "sorted by time");
        assert_eq!(list.next_ring_after(now()), Some(at(16, 14, 30)));
        assert_eq!(list.due(at(16, 14, 45)).count(), 1);
        list.snooze(b, at(16, 14, 55));
        assert_eq!(list.due(at(16, 14, 45)).count(), 0);
        list.done(b, at(16, 14, 56));
        assert_eq!(
            list.items.iter().find(|r| r.id == b).map(|r| r.due),
            Some(at(17, 14, 30))
        );
        list.done(a, at(16, 15, 1));
        assert!(list.items.iter().all(|r| r.id != a), "one-off is removed");
    }

    #[test]
    fn describes_times() {
        assert_eq!(describe(at(16, 18, 30), now(), true), "Today 18:30");
        assert_eq!(describe(at(17, 9, 0), now(), false), "Tomorrow 9:00 AM");
        assert_eq!(describe(at(20, 9, 0), now(), true), "Fri 9:00");
        assert_eq!(describe(at(30, 9, 5), now(), true), "30 Nov 9:05");
    }
}
