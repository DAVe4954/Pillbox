//! M8: the notification mirror's logic. The platform layer reads Windows' toast notifications
//! (`UserNotificationListener`); this module keeps the list, tells which ones just arrived (for
//! the stack on the closed pill) and cleans their text. No Windows types in here.

use std::collections::HashMap;

use crate::settings::{NoteMode, NoteRule};

/// One toast notification as Windows shows it in the notification centre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// Windows' id for it; unique among the notifications present right now.
    pub id: u32,
    /// The app's id (AUMID), the same for every notification of one app.
    pub app_id: String,
    /// The app's display name.
    pub app: String,
    /// First text line (usually a title or sender).
    pub title: String,
    /// The remaining text lines, joined with a space.
    pub body: String,
    /// When it was posted, milliseconds since 1970 (UTC).
    pub created_ms: i64,
}

/// Windows `DateTime` ticks (100 ns since 1601-01-01 UTC) as milliseconds since 1970.
pub fn unix_ms_from_ticks(ticks: i64) -> i64 {
    const EPOCH_DIFF_TICKS: i64 = 116_444_736_000_000_000;
    (ticks - EPOCH_DIFF_TICKS).div_euclid(10_000)
}

/// Text from a notification with line breaks and runs of spaces collapsed to single spaces.
pub fn clean(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// At most `max` characters, with "…" in place of the cut-off part.
pub fn short(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut s: String = text.chars().take(max.saturating_sub(1)).collect();
    s.truncate(s.trim_end().len());
    s.push('…');
    s
}

/// The letter on an app's badge: the first letter or digit of its name, upper case ("?" if none).
pub fn badge_letter(app: &str) -> char {
    app.chars()
        .find(|c| c.is_alphanumeric())
        .and_then(|c| c.to_uppercase().next())
        .unwrap_or('?')
}

/// FNV-1a over the lower-cased text: stable across runs and platforms (the std hasher is not).
fn fnv(text: &str) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in text.to_lowercase().bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// The key an app's icon is stored under in the renderer's thumbnail slots: the same for every
/// notification of the app. Bit 59 keeps it apart from the other kinds of thumbnail key.
pub fn icon_key(app_id: &str) -> u64 {
    (1 << 59) | u64::from(fnv(app_id))
}

/// A badge colour (0xRRGGBB) for an app, the same every time: picked from a fixed palette by a
/// hash of the app id, so each app keeps its colour.
pub fn badge_color(app_id: &str) -> u32 {
    const PALETTE: [u32; 8] = [
        0x4C8DF6, 0xE5645A, 0x3FB27F, 0xE0A030, 0x9B6BE0, 0x2CB5C0, 0xD86AA8, 0x7A8CA3,
    ];
    let h = fnv(app_id);
    PALETTE[(h % PALETTE.len() as u32) as usize]
}

/// What the rules say about a notification's app, if any rule names it (by app name or app id,
/// ignoring case). The first matching rule wins.
pub fn rule_mode(note: &Note, rules: &[NoteRule]) -> Option<NoteMode> {
    rules
        .iter()
        .find(|r| r.app.eq_ignore_ascii_case(&note.app) || r.app.eq_ignore_ascii_case(&note.app_id))
        .map(|r| r.mode)
}

/// What the closed pill says about a notification: its title, or its text when it has no
/// title, or the app name when it has neither; at most `max` characters.
pub fn pill_text(note: &Note, max: usize) -> String {
    let text = [&note.title, &note.body, &note.app]
        .into_iter()
        .find(|t| !t.is_empty())
        .map_or("", String::as_str);
    short(text, max)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Tracked {
    note: Note,
    /// When this program first saw it; `None` for those already there at the first reading,
    /// which never go on the pill.
    arrived_ms: Option<i64>,
}

/// The notifications present now, newest first, and when each arrived.
#[derive(Debug, Default)]
pub struct NoteStore {
    items: Vec<Tracked>,
    primed: bool,
}

/// What one reading changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Synced {
    /// Anything differs from the previous reading (the list must be redrawn).
    pub changed: bool,
    /// Notifications that were not there before (not counting the first reading).
    pub arrived: usize,
}

/// More than this many are kept; Windows itself keeps about 20 per app.
const MAX_KEPT: usize = 100;

impl NoteStore {
    /// Take a new reading of the notification centre.
    pub fn sync(&mut self, mut incoming: Vec<Note>, now_ms: i64) -> Synced {
        incoming.sort_by(|a, b| {
            b.created_ms
                .cmp(&a.created_ms)
                .then_with(|| b.id.cmp(&a.id))
        });
        incoming.truncate(MAX_KEPT);
        let known: HashMap<u32, &Tracked> = self.items.iter().map(|t| (t.note.id, t)).collect();
        let mut arrived = 0;
        let items: Vec<Tracked> = incoming
            .into_iter()
            .map(|note| match known.get(&note.id) {
                Some(t) if t.note == note => (*t).clone(),
                // The same id with other text is a replaced notification: it arrives again.
                _ if !self.primed => Tracked {
                    note,
                    arrived_ms: None,
                },
                _ => {
                    arrived += 1;
                    Tracked {
                        note,
                        arrived_ms: Some(now_ms),
                    }
                }
            })
            .collect();
        let changed = !self.primed || items != self.items;
        self.primed = true;
        self.items = items;
        Synced { changed, arrived }
    }

    /// Forget everything (the mirror was turned off).
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Every notification, newest first.
    pub fn notes(&self) -> impl ExactSizeIterator<Item = &Note> {
        self.items.iter().map(|t| &t.note)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The notifications that arrived within the last `window_ms`, newest first: what the
    /// closed pill stacks.
    pub fn stack(&self, now_ms: i64, window_ms: i64) -> Vec<&Note> {
        let mut fresh: Vec<&Tracked> = self
            .items
            .iter()
            .filter(|t| t.arrived_ms.is_some_and(|a| now_ms - a < window_ms))
            .collect();
        fresh.sort_by_key(|t| std::cmp::Reverse(t.arrived_ms));
        fresh.into_iter().map(|t| &t.note).collect()
    }

    /// When the latest arrival was first seen (`None` if nothing arrived yet).
    pub fn last_arrival_ms(&self) -> Option<i64> {
        self.items.iter().filter_map(|t| t.arrived_ms).max()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(id: u32, app: &str, title: &str, created_ms: i64) -> Note {
        Note {
            id,
            app_id: format!("{app}!App"),
            app: app.into(),
            title: title.into(),
            body: String::new(),
            created_ms,
        }
    }

    #[test]
    fn windows_ticks_become_unix_milliseconds() {
        // 2026-01-01T00:00:00Z.
        assert_eq!(
            unix_ms_from_ticks(134_116_992_000_000_000),
            1_767_225_600_000
        );
        assert_eq!(unix_ms_from_ticks(116_444_736_000_000_000), 0);
    }

    #[test]
    fn text_is_cleaned_and_shortened() {
        assert_eq!(clean("  a \n b\t\tc  "), "a b c");
        assert_eq!(short("short", 10), "short");
        assert_eq!(short("a long notification title", 10), "a long no…");
        assert_eq!(short("abc ", 3), "ab…");
        assert_eq!(short("", 5), "");
    }

    #[test]
    fn badges_are_stable() {
        assert_eq!(badge_letter("  claude"), 'C');
        assert_eq!(badge_letter("…"), '?');
        assert_eq!(badge_color("Claude!App"), badge_color("claude!app"));
        // Different apps do not all share one colour.
        let colors: std::collections::HashSet<u32> =
            (0..20).map(|i| badge_color(&format!("app{i}"))).collect();
        assert!(colors.len() > 3);
    }

    fn rule(app: &str, mode: NoteMode) -> NoteRule {
        NoteRule {
            app: app.into(),
            mode,
        }
    }

    #[test]
    fn rules_match_the_app_name_or_the_app_id_ignoring_case() {
        let n = note(1, "Claude", "hi", 0);
        assert_eq!(rule_mode(&n, &[]), None);
        assert_eq!(
            rule_mode(&n, &[rule("claude", NoteMode::Hide)]),
            Some(NoteMode::Hide)
        );
        assert_eq!(
            rule_mode(&n, &[rule("CLAUDE!APP", NoteMode::ListOnly)]),
            Some(NoteMode::ListOnly)
        );
        assert_eq!(rule_mode(&n, &[rule("Claud", NoteMode::Hide)]), None);
        // The first rule that names the app wins.
        let both = [
            rule("claude", NoteMode::ListOnly),
            rule("claude", NoteMode::Hide),
        ];
        assert_eq!(rule_mode(&n, &both), Some(NoteMode::ListOnly));
    }

    #[test]
    fn an_apps_icon_key_is_stable_and_marked() {
        assert_eq!(icon_key("Claude!App"), icon_key("claude!app"));
        assert_ne!(icon_key("a"), icon_key("b"));
        // Bit 59 is set and the two top bits (used by the file and picture keys) are not.
        assert_eq!(icon_key("x") >> 59, 1);
    }

    #[test]
    fn the_pill_says_the_title_or_else_the_text_or_else_the_app() {
        let mut n = note(1, "Mail", "New message", 0);
        assert_eq!(pill_text(&n, 30), "New message");
        n.title.clear();
        n.body = "hello there".into();
        assert_eq!(pill_text(&n, 30), "hello there");
        n.body.clear();
        assert_eq!(pill_text(&n, 30), "Mail");
        assert_eq!(pill_text(&n, 3), "Ma…");
    }

    #[test]
    fn the_first_reading_is_not_news_but_later_ones_are() {
        let mut s = NoteStore::default();
        let first = s.sync(
            vec![note(1, "A", "old", 100), note(2, "B", "older", 50)],
            1000,
        );
        assert!(first.changed);
        assert_eq!(first.arrived, 0);
        assert!(s.stack(1001, 5000).is_empty());
        let next = s.sync(
            vec![
                note(1, "A", "old", 100),
                note(2, "B", "older", 50),
                note(3, "C", "new", 200),
            ],
            2000,
        );
        assert_eq!((next.changed, next.arrived), (true, 1));
        let titles: Vec<&str> = s
            .stack(2100, 5000)
            .iter()
            .map(|n| n.title.as_str())
            .collect();
        assert_eq!(titles, ["new"]);
    }

    #[test]
    fn the_list_is_newest_first() {
        let mut s = NoteStore::default();
        s.sync(
            vec![
                note(1, "A", "a", 10),
                note(2, "B", "b", 30),
                note(3, "C", "c", 20),
            ],
            0,
        );
        let ids: Vec<u32> = s.notes().map(|n| n.id).collect();
        assert_eq!(ids, [2, 3, 1]);
    }

    #[test]
    fn an_unchanged_reading_changes_nothing() {
        let mut s = NoteStore::default();
        let list = vec![note(1, "A", "a", 10)];
        s.sync(list.clone(), 0);
        let again = s.sync(list, 1000);
        assert_eq!((again.changed, again.arrived), (false, 0));
    }

    #[test]
    fn the_stack_holds_recent_arrivals_newest_first_and_then_empties() {
        let mut s = NoteStore::default();
        s.sync(vec![note(1, "A", "a", 10)], 0);
        s.sync(vec![note(1, "A", "a", 10), note(2, "B", "b", 20)], 1000);
        s.sync(
            vec![
                note(1, "A", "a", 10),
                note(2, "B", "b", 20),
                note(3, "C", "c", 30),
            ],
            3000,
        );
        let ids: Vec<u32> = s.stack(4000, 5000).iter().map(|n| n.id).collect();
        assert_eq!(ids, [3, 2]);
        // 5 s after #2 arrived it leaves the stack; #3 is still there.
        let ids: Vec<u32> = s.stack(6500, 5000).iter().map(|n| n.id).collect();
        assert_eq!(ids, [3]);
        assert!(s.stack(9000, 5000).is_empty());
        assert_eq!(s.last_arrival_ms(), Some(3000));
    }

    #[test]
    fn a_dismissed_notification_leaves_and_a_replaced_one_arrives_again() {
        let mut s = NoteStore::default();
        s.sync(vec![note(1, "A", "a", 10), note(2, "B", "b", 20)], 0);
        let gone = s.sync(vec![note(2, "B", "b", 20)], 1000);
        assert_eq!((gone.changed, gone.arrived, s.len()), (true, 0, 1));
        let replaced = s.sync(vec![note(2, "B", "b, edited", 20)], 2000);
        assert_eq!((replaced.changed, replaced.arrived), (true, 1));
    }

    #[test]
    fn clearing_starts_over_with_a_quiet_first_reading() {
        let mut s = NoteStore::default();
        s.sync(vec![note(1, "A", "a", 10)], 0);
        s.clear();
        assert!(s.is_empty());
        let first = s.sync(vec![note(1, "A", "a", 10)], 5000);
        assert_eq!(first.arrived, 0);
        assert!(s.stack(5001, 5000).is_empty());
    }
}
