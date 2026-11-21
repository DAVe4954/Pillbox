//! Clipboard history: the list of copied items, de-duplication, pinning, limits and filters.
//! Pixel data of pictures is kept by the app; this holds what is needed to list and search.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipKind {
    Text,
    Link,
    Picture,
    Files,
}

/// The sub-tabs of the clipboard tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClipFilter {
    #[default]
    All,
    Text,
    Links,
    Pictures,
    Files,
    Pinned,
    Snippets,
}

impl ClipFilter {
    pub const ALL: [ClipFilter; 7] = [
        ClipFilter::All,
        ClipFilter::Text,
        ClipFilter::Links,
        ClipFilter::Pictures,
        ClipFilter::Files,
        ClipFilter::Pinned,
        ClipFilter::Snippets,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ClipFilter::All => "All",
            ClipFilter::Text => "Text",
            ClipFilter::Links => "Links",
            ClipFilter::Pictures => "Pictures",
            ClipFilter::Files => "Files",
            ClipFilter::Pinned => "Pinned",
            ClipFilter::Snippets => "Snippets",
        }
    }

    fn accepts(self, item: &ClipItem) -> bool {
        match self {
            ClipFilter::All => true,
            ClipFilter::Text => item.kind == ClipKind::Text,
            ClipFilter::Links => item.kind == ClipKind::Link,
            ClipFilter::Pictures => item.kind == ClipKind::Picture,
            ClipFilter::Files => item.kind == ClipKind::Files,
            ClipFilter::Pinned => item.pinned,
            ClipFilter::Snippets => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipItem {
    pub id: u64,
    pub kind: ClipKind,
    /// The text, the link, or the file paths one per line. Empty for pictures.
    pub text: String,
    /// Picture size in pixels.
    #[serde(default)]
    pub size: (u32, u32),
    pub pinned: bool,
    /// When it was (last) copied, seconds since 1970.
    pub at: i64,
    /// Content fingerprint, to spot the same thing copied again.
    pub hash: u64,
    /// The app it came from, if known.
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ClipHistory {
    pub next_id: u64,
    /// Newest first.
    pub items: Vec<ClipItem>,
}

/// What `record` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recorded {
    New(u64),
    /// The same content was already there; it moved to the top.
    Moved(u64),
}

/// FNV-1a, enough to recognise identical copies.
pub fn fingerprint(kind: ClipKind, bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ kind as u64;
    for b in bytes {
        h = (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Text that is a single web or mail link counts as a link.
pub fn classify_text(text: &str) -> ClipKind {
    let t = text.trim();
    let one_token = !t.is_empty() && !t.contains(char::is_whitespace);
    let lower = t.to_ascii_lowercase();
    let looks_like_link = lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("www.")
        || lower.starts_with("mailto:")
        || lower.starts_with("ftp://");
    if one_token && looks_like_link {
        ClipKind::Link
    } else {
        ClipKind::Text
    }
}

/// One line for the list: whitespace runs collapsed, cut to `max` characters with "…".
pub fn preview(text: &str, max: usize) -> String {
    let mut out = String::new();
    let mut count = 0;
    for word in text.split_whitespace() {
        if count > 0 {
            out.push(' ');
            count += 1;
        }
        for c in word.chars() {
            if count >= max {
                out.push('…');
                return out;
            }
            out.push(c);
            count += 1;
        }
    }
    out
}

/// "now", "4 min", "3 h", "2 d".
pub fn ago(seconds: i64) -> String {
    match seconds.max(0) {
        0..60 => "now".into(),
        s @ 60..3600 => format!("{} min", s / 60),
        s @ 3600..86_400 => format!("{} h", s / 3600),
        s => format!("{} d", s / 86_400),
    }
}

impl ClipHistory {
    /// Add a copy at the top, or move an identical earlier copy there.
    pub fn record(&mut self, mut item: ClipItem) -> Recorded {
        if let Some(i) = self
            .items
            .iter()
            .position(|e| e.hash == item.hash && e.kind == item.kind)
        {
            let mut existing = self.items.remove(i);
            existing.at = item.at;
            let id = existing.id;
            self.items.insert(0, existing);
            return Recorded::Moved(id);
        }
        self.next_id += 1;
        item.id = self.next_id;
        let id = item.id;
        self.items.insert(0, item);
        Recorded::New(id)
    }

    /// Drop the oldest unpinned items beyond `max_items`, and unpinned pictures beyond
    /// `max_pictures`. Returns the removed ids (their picture files can then be deleted).
    pub fn trim(&mut self, max_items: usize, max_pictures: usize) -> Vec<u64> {
        let mut removed = Vec::new();
        let mut kept = 0usize;
        let mut pictures = 0usize;
        self.items.retain(|item| {
            if item.pinned {
                return true;
            }
            let is_picture = item.kind == ClipKind::Picture;
            let keep = kept < max_items && (!is_picture || pictures < max_pictures);
            if keep {
                kept += 1;
                pictures += usize::from(is_picture);
            } else {
                removed.push(item.id);
            }
            keep
        });
        removed
    }

    /// Drop unpinned items copied more than `days` days before `now` (seconds since 1970);
    /// `days` = 0 keeps everything. Returns the removed ids (their picture files can then be
    /// deleted). An item dated in the future (the clock was changed) is kept.
    pub fn expire(&mut self, now: i64, days: u32) -> Vec<u64> {
        if days == 0 {
            return Vec::new();
        }
        let cutoff = now.saturating_sub(i64::from(days) * 86_400);
        let mut removed = Vec::new();
        self.items.retain(|item| {
            let keep = item.pinned || item.at >= cutoff;
            if !keep {
                removed.push(item.id);
            }
            keep
        });
        removed
    }

    pub fn set_pinned(&mut self, id: u64, pinned: bool) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.pinned = pinned;
        }
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|i| i.id != id);
        self.items.len() != before
    }

    /// Remove everything that is not pinned; returns the removed ids.
    pub fn clear_unpinned(&mut self) -> Vec<u64> {
        let removed = self
            .items
            .iter()
            .filter(|i| !i.pinned)
            .map(|i| i.id)
            .collect();
        self.items.retain(|i| i.pinned);
        removed
    }

    pub fn get(&self, id: u64) -> Option<&ClipItem> {
        self.items.iter().find(|i| i.id == id)
    }

    /// Items for a sub-tab and search text (case-insensitive, all words must match), pinned
    /// items first, then newest first.
    pub fn view(&self, filter: ClipFilter, query: &str) -> Vec<&ClipItem> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let mut out: Vec<&ClipItem> = self
            .items
            .iter()
            .filter(|i| filter.accepts(i))
            .filter(|i| {
                if words.is_empty() {
                    return true;
                }
                let hay = format!("{} {}", i.text, i.source).to_lowercase();
                words.iter().all(|w| hay.contains(w.as_str()))
            })
            .collect();
        // Stable sort keeps newest-first order within each group.
        out.sort_by_key(|i| !i.pinned);
        out
    }
}

/// What a right-click "open" does with a clipboard item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenTarget {
    /// Open these files or folders with their own apps (the first `OPEN_MAX_FILES`).
    Paths(Vec<String>),
    /// Open this web or mail link in the browser / mail app.
    Link(String),
    /// Write the text to a file and open it in the text editor.
    TextFile,
    /// Write the picture to a PNG and open it in the picture viewer.
    PictureFile,
}

/// The most files one right-click opens.
pub const OPEN_MAX_FILES: usize = 5;

/// How to open an item. A "link" only opens as one when it is `http(s)://` or `mailto:` (the
/// shell would run anything else it is given, a program path for example); other text opens
/// as a text file.
pub fn open_target(item: &ClipItem) -> OpenTarget {
    match item.kind {
        ClipKind::Files => OpenTarget::Paths(
            item.text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .take(OPEN_MAX_FILES)
                .map(str::to_string)
                .collect(),
        ),
        ClipKind::Picture => OpenTarget::PictureFile,
        ClipKind::Link | ClipKind::Text => {
            let t = item.text.trim();
            let lower = t.to_ascii_lowercase();
            let web = ["http://", "https://", "mailto:"]
                .iter()
                .any(|p| lower.starts_with(p));
            if web && !t.contains(char::is_whitespace) {
                OpenTarget::Link(t.to_string())
            } else {
                OpenTarget::TextFile
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_items_picks_the_right_app() {
        let link = text("https://example.com/a?b=1", 1);
        assert_eq!(
            open_target(&link),
            OpenTarget::Link("https://example.com/a?b=1".into())
        );
        assert_eq!(
            open_target(&text("  mailto:someone@example.com ", 1)),
            OpenTarget::Link("mailto:someone@example.com".into())
        );
        // Plain text, and text that only looks a bit like a link, open as a text file.
        assert_eq!(open_target(&text("hello world", 1)), OpenTarget::TextFile);
        assert_eq!(
            open_target(&text("https://a.com and more words", 1)),
            OpenTarget::TextFile
        );
        let mut exe = text("C:\\Windows\\System32\\cmd.exe", 1);
        exe.kind = ClipKind::Link;
        assert_eq!(
            open_target(&exe),
            OpenTarget::TextFile,
            "never run a path as a link"
        );
        // Files: each path, at most five.
        let mut files = text("", 1);
        files.kind = ClipKind::Files;
        files.text = (0..8).map(|i| format!("C:\\f{i}.txt\n")).collect();
        match open_target(&files) {
            OpenTarget::Paths(p) => {
                assert_eq!(p.len(), OPEN_MAX_FILES);
                assert_eq!(p[0], "C:\\f0.txt");
            }
            other => panic!("{other:?}"),
        }
        let mut pic = text("", 1);
        pic.kind = ClipKind::Picture;
        assert_eq!(open_target(&pic), OpenTarget::PictureFile);
    }

    fn text(t: &str, at: i64) -> ClipItem {
        let kind = classify_text(t);
        ClipItem {
            id: 0,
            kind,
            text: t.into(),
            size: (0, 0),
            pinned: false,
            at,
            hash: fingerprint(kind, t.as_bytes()),
            source: "notepad.exe".into(),
        }
    }

    fn picture(n: u8, at: i64) -> ClipItem {
        ClipItem {
            id: 0,
            kind: ClipKind::Picture,
            text: String::new(),
            size: (10, 10),
            pinned: false,
            at,
            hash: fingerprint(ClipKind::Picture, &[n]),
            source: String::new(),
        }
    }

    #[test]
    fn links_are_recognised() {
        assert_eq!(classify_text("https://example.com/a?b=c"), ClipKind::Link);
        assert_eq!(classify_text("  www.example.com "), ClipKind::Link);
        assert_eq!(classify_text("mailto:me@example.com"), ClipKind::Link);
        assert_eq!(classify_text("see https://example.com"), ClipKind::Text);
        assert_eq!(classify_text("example.com"), ClipKind::Text);
    }

    #[test]
    fn copying_again_moves_to_top_instead_of_duplicating() {
        let mut h = ClipHistory::default();
        let a = h.record(text("alpha", 1));
        let b = h.record(text("beta", 2));
        assert!(matches!((a, b), (Recorded::New(1), Recorded::New(2))));
        assert_eq!(h.record(text("alpha", 3)), Recorded::Moved(1));
        assert_eq!(h.items.len(), 2);
        assert_eq!(h.items[0].text, "alpha");
        assert_eq!(h.items[0].at, 3);
    }

    #[test]
    fn old_unpinned_items_expire() {
        const DAY: i64 = 86_400;
        let now = 100 * DAY;
        let mut h = ClipHistory::default();
        h.record(text("ten days old", now - 10 * DAY));
        h.record(text("two days old", now - 2 * DAY));
        h.record(text("pinned and old", now - 50 * DAY));
        h.set_pinned(3, true);
        h.record(text("just now", now));
        h.record(text("from the future", now + DAY));
        assert!(h.expire(now, 0).is_empty(), "0 = never");
        assert_eq!(h.items.len(), 5);
        assert_eq!(h.expire(now, 7), vec![1]);
        assert_eq!(h.expire(now, 1), vec![2]);
        let left: Vec<&str> = h.items.iter().map(|i| i.text.as_str()).collect();
        assert_eq!(left, ["from the future", "just now", "pinned and old"]);
        // Exactly at the limit stays.
        let mut h = ClipHistory::default();
        h.record(text("edge", now - DAY));
        assert!(h.expire(now, 1).is_empty());
    }

    #[test]
    fn trimming_keeps_pinned_and_limits_pictures() {
        let mut h = ClipHistory::default();
        for i in 0..5 {
            h.record(text(&format!("t{i}"), i));
        }
        h.set_pinned(1, true); // "t0", the oldest
        for i in 0..4u8 {
            h.record(picture(i, 10 + i64::from(i)));
        }
        let removed = h.trim(5, 2);
        // 8 unpinned items: the two oldest pictures go for the picture limit, then the oldest
        // text for the item limit; the pinned one never counts.
        assert_eq!(
            h.items
                .iter()
                .filter(|i| i.kind == ClipKind::Picture)
                .count(),
            2
        );
        assert!(h.get(1).is_some(), "pinned survives");
        assert_eq!(removed, vec![7, 6, 2], "{removed:?}");
        assert_eq!(h.items.len(), 6);
    }

    #[test]
    fn filters_and_search() {
        let mut h = ClipHistory::default();
        h.record(text("Hello World", 1));
        h.record(text("https://rust-lang.org", 2));
        h.record(picture(1, 3));
        let pinned = match h.record(text("pinned note", 4)) {
            Recorded::New(id) => id,
            Recorded::Moved(id) => id,
        };
        h.set_pinned(pinned, true);
        h.record(text("newest", 5));
        assert_eq!(
            h.view(ClipFilter::All, "")[0].text,
            "pinned note",
            "pinned first"
        );
        assert_eq!(h.view(ClipFilter::All, "")[1].text, "newest");
        assert_eq!(h.view(ClipFilter::Links, "").len(), 1);
        assert_eq!(h.view(ClipFilter::Pictures, "").len(), 1);
        assert_eq!(h.view(ClipFilter::Text, "world hello").len(), 1);
        assert_eq!(
            h.view(ClipFilter::All, "NOTEPAD").len(),
            4,
            "source app is searchable"
        );
        assert!(h.view(ClipFilter::Snippets, "").is_empty());
        assert_eq!(h.clear_unpinned().len(), 4);
        assert_eq!(h.items.len(), 1);
    }

    #[test]
    fn previews_and_ages() {
        assert_eq!(preview("  a\n\n b\tc  ", 10), "a b c");
        assert_eq!(preview("abcdefghij", 4), "abcd…");
        assert_eq!(ago(5), "now");
        assert_eq!(ago(125), "2 min");
        assert_eq!(ago(7300), "2 h");
        assert_eq!(ago(200_000), "2 d");
    }
}
