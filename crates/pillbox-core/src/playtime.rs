//! Time spent in fullscreen games, per day and per game, for the play-time widget.
//! Stored in `data\playtime.json`; nothing leaves the PC.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::calendar::{self, Date};

/// Days kept in the log.
pub const KEEP_DAYS: i64 = 60;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayLog {
    /// `YYYY-MM-DD` → game name → seconds.
    pub days: BTreeMap<String, BTreeMap<String, u64>>,
}

pub fn date_key(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year, d.month, d.day)
}

/// The key of the day `n` days before `today`.
fn days_back(today: Date, n: i64) -> String {
    date_key(calendar::civil_from_days(
        calendar::days_from_civil(today) - n,
    ))
}

/// Whether a name in the log (made by `game_name` from an executable) is a real game and not a
/// browser, launcher helper or other app that happened to be fullscreen.
pub fn is_game_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && !crate::games::is_non_game_exe(&format!("{name}.exe"))
        && !crate::games::is_non_game_exe(name)
}

impl PlayLog {
    /// Drop everything that is not a game: entries written before the filter existed.
    pub fn remove_non_games(&mut self) {
        for games in self.days.values_mut() {
            games.retain(|name, _| is_game_name(name));
        }
        self.days.retain(|_, games| !games.is_empty());
    }

    pub fn add(&mut self, today: Date, game: &str, seconds: u64) {
        if seconds == 0 || !is_game_name(game) {
            return;
        }
        *self
            .days
            .entry(date_key(today))
            .or_default()
            .entry(game.to_string())
            .or_default() += seconds;
        // Forget days older than the keep window.
        let oldest = days_back(today, KEEP_DAYS - 1);
        self.days.retain(|day, _| *day >= oldest);
    }

    pub fn day_total(&self, today: Date) -> u64 {
        self.days
            .get(&date_key(today))
            .map_or(0, |g| g.values().sum())
    }

    /// Per-game totals over the last 7 days, today included, largest first.
    pub fn week(&self, today: Date) -> Vec<(String, u64)> {
        let first = days_back(today, 6);
        let last = date_key(today);
        let mut totals: BTreeMap<&str, u64> = BTreeMap::new();
        for (day, games) in &self.days {
            if *day >= first && *day <= last {
                for (game, s) in games {
                    *totals.entry(game).or_default() += s;
                }
            }
        }
        let mut list: Vec<(String, u64)> = totals
            .into_iter()
            .map(|(g, s)| (g.to_string(), s))
            .collect();
        list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        list
    }
}

/// One day of play: what was played and for how long.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayPlay {
    pub date: Date,
    /// Seconds over all games.
    pub total: u64,
    /// Longest played first.
    pub games: Vec<(String, u64)>,
}

impl DayPlay {
    /// "Today" for `today`, else "Thu 19".
    pub fn chip(&self, today: Date) -> String {
        if self.date == today {
            "Today".to_string()
        } else {
            self.short()
        }
    }

    /// "Today", "Yesterday", else "Thursday 19 Nov".
    pub fn title(&self, today: Date) -> String {
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        const DAYS: [&str; 7] = [
            "Sunday",
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
        ];
        if self.date == today {
            "Today".to_string()
        } else if self.date == today.add_days(-1) {
            "Yesterday".to_string()
        } else {
            format!(
                "{} {} {}",
                DAYS[self.date.weekday() as usize % 7],
                self.date.day,
                MONTHS[(self.date.month as usize).clamp(1, 12) - 1]
            )
        }
    }

    fn short(&self) -> String {
        const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        format!(
            "{} {}",
            DAYS[self.date.weekday() as usize % 7],
            self.date.day
        )
    }
}

impl PlayLog {
    /// The last `count` days, today first (days with no play included). `live` is the game
    /// being played now and the seconds of it not written to the log yet; they count for today.
    pub fn recent_days(
        &self,
        today: Date,
        count: usize,
        live: Option<(&str, u64)>,
    ) -> Vec<DayPlay> {
        (0..count)
            .map(|back| {
                let date = today.add_days(-(back as i64));
                let mut games: BTreeMap<String, u64> =
                    self.days.get(&date_key(date)).cloned().unwrap_or_default();
                if back == 0
                    && let Some((game, seconds)) = live
                    && seconds > 0
                {
                    *games.entry(game.to_string()).or_default() += seconds;
                }
                let mut games: Vec<(String, u64)> =
                    games.into_iter().filter(|(_, s)| *s > 0).collect();
                games.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
                DayPlay {
                    date,
                    total: games.iter().map(|(_, s)| s).sum(),
                    games,
                }
            })
            .collect()
    }
}

/// "1h 20m", "35m", "0m".
pub fn format_duration(seconds: u64) -> String {
    let minutes = seconds / 60;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// A readable game name from its executable: `FortniteClient-Win64-Shipping.exe` → "Fortnite",
/// `eldenring.exe` → "Eldenring", `RocketLeague.exe` → "RocketLeague".
pub fn game_name(exe: &str) -> String {
    let mut name = exe.trim();
    if name.to_ascii_lowercase().ends_with(".exe") {
        name = &name[..name.len() - 4];
    }
    // Unreal and similar engines add build suffixes.
    let lower = name.to_ascii_lowercase();
    for cut in [
        "-win64",
        "_win64",
        "-shipping",
        "_shipping",
        "-win32",
        "_x64",
        "-dx12",
        "_dx12",
    ] {
        if let Some(i) = lower.find(cut) {
            name = &name[..i.min(name.len())];
            break;
        }
    }
    let lower = name.to_ascii_lowercase();
    if lower.len() > "client".len() && lower.ends_with("client") {
        name = &name[..name.len() - "client".len()];
    }
    let name = name.trim_matches(['-', '_', ' ']);
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => exe.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(d: u32) -> Date {
        Date {
            year: 2026,
            month: 11,
            day: d,
        }
    }

    #[test]
    fn totals_per_day_and_week() {
        let mut log = PlayLog::default();
        log.add(day(10), "Fortnite", 3600);
        log.add(day(16), "Fortnite", 1800);
        log.add(day(17), "Fortnite", 600);
        log.add(day(17), "Rocket League", 1200);
        log.add(day(17), "Rocket League", 0);
        assert_eq!(log.day_total(day(17)), 1800);
        assert_eq!(log.day_total(day(15)), 0);
        // The 10th is outside the last 7 days (11th..17th).
        assert_eq!(
            log.week(day(17)),
            [
                ("Fortnite".to_string(), 2400),
                ("Rocket League".to_string(), 1200)
            ]
        );
    }

    #[test]
    fn recent_days_list_each_day_with_its_games() {
        let mut log = PlayLog::default();
        log.add(day(15), "Fortnite", 3600);
        log.add(day(17), "Fortnite", 600);
        log.add(day(17), "Rocket League", 1200);
        let days = log.recent_days(day(17), 3, None);
        assert_eq!(days.len(), 3);
        assert_eq!(days[0].date, day(17));
        assert_eq!(days[0].total, 1800);
        assert_eq!(
            days[0].games,
            [
                ("Rocket League".to_string(), 1200),
                ("Fortnite".to_string(), 600)
            ]
        );
        assert!(days[1].games.is_empty(), "the 16th has no play");
        assert_eq!(days[2].total, 3600);
        // The game being played now counts for today only.
        let live = log.recent_days(day(17), 2, Some(("Fortnite", 120)));
        assert_eq!(live[0].total, 1920);
        assert_eq!(live[0].games[1], ("Fortnite".to_string(), 720));
        assert_eq!(live[1].total, 0);
    }

    #[test]
    fn day_labels() {
        let log = PlayLog::default();
        let days = log.recent_days(day(20), 3, None);
        assert_eq!(days[0].chip(day(20)), "Today");
        assert_eq!(days[1].chip(day(20)), "Thu 19");
        assert_eq!(days[0].title(day(20)), "Today");
        assert_eq!(days[1].title(day(20)), "Yesterday");
        assert_eq!(days[2].title(day(20)), "Wednesday 18 Nov");
    }

    #[test]
    fn only_games_are_counted() {
        assert!(is_game_name("Fortnite"));
        assert!(is_game_name("Elden Ring"));
        for not in [
            "Brave",
            "Steamwebhelper",
            "Msedge",
            "Opera_gx",
            "Discord",
            "Vlc",
        ] {
            assert!(!is_game_name(not), "{not}");
        }
        let mut log = PlayLog::default();
        log.add(day(17), "Brave", 600);
        log.add(day(17), "Steamwebhelper", 600);
        log.add(day(17), "Fortnite", 60);
        assert_eq!(log.day_total(day(17)), 60);
        // A log written before the filter is cleaned.
        let mut old = PlayLog::default();
        old.days
            .entry("2026-11-17".into())
            .or_default()
            .insert("Brave".into(), 99);
        old.days
            .entry("2026-11-17".into())
            .or_default()
            .insert("Fortnite".into(), 5);
        old.days
            .entry("2026-11-16".into())
            .or_default()
            .insert("Steamwebhelper".into(), 7);
        old.remove_non_games();
        assert_eq!(old.days.len(), 1);
        assert_eq!(old.day_total(day(17)), 5);
    }

    #[test]
    fn old_days_are_forgotten() {
        let mut log = PlayLog::default();
        log.add(
            Date {
                year: 2026,
                month: 1,
                day: 1,
            },
            "Old",
            60,
        );
        log.add(day(17), "New", 60);
        assert_eq!(log.days.len(), 1);
        assert!(log.days.contains_key("2026-11-17"));
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(format_duration(0), "0m");
        assert_eq!(format_duration(59), "0m");
        assert_eq!(format_duration(35 * 60), "35m");
        assert_eq!(format_duration(3600), "1h");
        assert_eq!(format_duration(80 * 60 + 30), "1h 20m");
    }

    #[test]
    fn game_names_from_executables() {
        assert_eq!(game_name("FortniteClient-Win64-Shipping.exe"), "Fortnite");
        assert_eq!(game_name("fortniteclient-win64-shipping.exe"), "Fortnite");
        assert_eq!(game_name("eldenring.exe"), "Eldenring");
        assert_eq!(game_name("RocketLeague.exe"), "RocketLeague");
        assert_eq!(game_name("Client.exe"), "Client");
        assert_eq!(game_name(".exe"), ".exe");
    }
}
