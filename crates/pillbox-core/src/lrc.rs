//! Parser for time-synced lyrics in LRC format, as returned by LRCLIB.
//!
//! Handles `[mm:ss]`, `[mm:ss.xx]`, `[mm:ss.xxx]` and `[mm:ss:xx]` stamps, several stamps on
//! one line, the `[offset:±ms]` tag, and ignores other metadata tags such as `[ar:...]`.

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Seconds from the start of the song.
    pub time: f32,
    /// Empty for an instrumental gap.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Lyrics {
    /// Sorted by time.
    pub lines: Vec<Line>,
}

/// Parse the inside of one `[...]` as a timestamp in seconds.
fn parse_stamp(tag: &str) -> Option<f32> {
    let (minutes, rest) = tag.split_once(':')?;
    if minutes.is_empty() || !minutes.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // Seconds may carry a fraction after '.' or, in some files, after a second ':'.
    let (seconds, fraction) = match rest.split_once(['.', ':']) {
        Some((s, f)) => (s, f),
        None => (rest, ""),
    };
    if seconds.is_empty()
        || seconds.len() > 2
        || !seconds.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let minutes: f32 = minutes.parse().ok()?;
    let seconds: f32 = seconds.parse().ok()?;
    if seconds >= 60.0 {
        return None;
    }
    let fraction = if fraction.is_empty() {
        0.0
    } else {
        let digits = &fraction[..fraction.len().min(6)];
        digits.parse::<f32>().ok()? / 10f32.powi(digits.len() as i32)
    };
    Some(minutes * 60.0 + seconds + fraction)
}

/// Remove per-word time tags such as `<00:12.50>` ("enhanced" LRC) and tidy the spacing.
/// Angle brackets that are not time tags are kept.
fn strip_word_stamps(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('>') {
            Some(close) if parse_stamp(after[..close].trim()).is_some() => {
                out.push(' ');
                rest = &after[close + 1..];
            }
            _ => {
                out.push('<');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn parse(text: &str) -> Lyrics {
    let mut lines = Vec::new();
    let mut offset_ms = 0.0f32;
    for raw in text.lines() {
        let mut rest = raw.trim();
        let mut stamps = Vec::new();
        // Peel leading [..] groups.
        while let Some(inner) = rest.strip_prefix('[') {
            let Some((tag, after)) = inner.split_once(']') else {
                break;
            };
            if let Some(time) = parse_stamp(tag.trim()) {
                stamps.push(time);
            } else if let Some(value) = tag.trim().strip_prefix("offset:") {
                offset_ms = value.trim().parse().unwrap_or(0.0);
            } else if !stamps.is_empty() {
                // A bracket that is not a stamp after real stamps is part of the words.
                break;
            }
            rest = after.trim_start();
        }
        if stamps.is_empty() {
            continue;
        }
        let words = strip_word_stamps(rest);
        for time in stamps {
            lines.push(Line {
                time,
                text: words.clone(),
            });
        }
    }
    // A positive offset means the words should appear earlier.
    let shift = offset_ms / 1000.0;
    for line in &mut lines {
        line.time = (line.time - shift).max(0.0);
    }
    lines.sort_by(|a, b| a.time.total_cmp(&b.time));
    Lyrics { lines }
}

/// A song whose first line starts at least this many seconds in gets a gap line (three dots)
/// before it.
pub const INTRO_GAP: f32 = 5.0;
/// How long the last line may be an instrumental gap when the song's end is not known.
const OPEN_GAP: f32 = 8.0;

impl Lyrics {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The same lyrics with an empty line at 0:00 when the song opens with a long
    /// instrumental intro (`INTRO_GAP`), so the wait shows as the three dots too.
    #[must_use]
    pub fn with_intro_gap(mut self) -> Self {
        if let Some(first) = self.lines.first()
            && first.time >= INTRO_GAP
            && !first.text.is_empty()
        {
            self.lines.insert(
                0,
                Line {
                    time: 0.0,
                    text: String::new(),
                },
            );
        }
        self
    }

    /// For an instrumental gap (an empty line): how far through it `position` is, 0..1, from
    /// the gap's start to the next line's start (`OPEN_GAP` seconds for the last line). `None`
    /// for a line with words.
    pub fn gap_progress(&self, index: usize, position: f32) -> Option<f32> {
        let line = self.lines.get(index).filter(|l| l.text.is_empty())?;
        let end = self
            .lines
            .get(index + 1)
            .map_or(line.time + OPEN_GAP, |next| next.time);
        let span = (end - line.time).max(0.001);
        Some(((position - line.time) / span).clamp(0.0, 1.0))
    }

    /// Index of the line being sung at `position` seconds; `None` before the first line.
    pub fn index_at(&self, position: f32) -> Option<usize> {
        let after = self.lines.partition_point(|l| l.time <= position);
        after.checked_sub(1)
    }

    /// Like `index_at`, but glides from one line to the next over the last `glide` seconds
    /// before a line starts, so the view scrolls instead of jumping. Returns -1.0 before the
    /// first line has begun to approach.
    pub fn smooth_index(&self, position: f32, glide: f32) -> f32 {
        let next = self.lines.partition_point(|l| l.time <= position);
        let base = next as f32 - 1.0;
        match self.lines.get(next) {
            Some(line) if glide > 0.0 => {
                let t = ((position - (line.time - glide)) / glide).clamp(0.0, 1.0);
                // Smoothstep.
                base + t * t * (3.0 - 2.0 * t)
            }
            _ => base,
        }
    }
}

/// Where line `index` sits relative to the viewing point `at` (a fractional line index: 2.5 is
/// halfway from line 2 to line 3), in pixels from the top of the line: negative above the
/// point, positive below. `height(i)` is the height of line `i`. Lines have different heights
/// (long ones wrap), so the offset adds up the heights between them.
pub fn line_offset(at: f32, index: usize, height: impl Fn(usize) -> f32) -> f32 {
    let at = if at.is_finite() { at.max(0.0) } else { 0.0 };
    let base = at.floor() as usize;
    let frac = at - base as f32;
    // Distance from the top of line `base` to the viewing point.
    let from_top = frac * height(base);
    if index >= base {
        (base..index).map(&height).sum::<f32>() - from_top
    } else {
        -(index..base).map(&height).sum::<f32>() - from_top
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaps_show_as_dots_with_progress() {
        let l = parse("[00:08.00]first\n[00:20.00]\n[00:30.00]second\n[00:40.00]");
        // A long intro gets a gap line of its own; the song's own gaps are kept.
        let l = l.with_intro_gap();
        assert_eq!(l.lines.len(), 5);
        assert_eq!(l.lines[0].time, 0.0);
        assert!(l.lines[0].text.is_empty());
        assert_eq!(l.gap_progress(0, 0.0), Some(0.0));
        assert_eq!(l.gap_progress(0, 4.0), Some(0.5));
        assert_eq!(l.gap_progress(0, 99.0), Some(1.0));
        // The gap between "first" and "second" (index 2: from 0:20 to 0:30).
        assert_eq!(l.gap_progress(2, 25.0), Some(0.5));
        // A line with words is not a gap; neither is a missing one.
        assert_eq!(l.gap_progress(1, 10.0), None);
        assert_eq!(l.gap_progress(9, 10.0), None);
        // The last line, an open gap: eight seconds are assumed.
        assert_eq!(l.gap_progress(4, 44.0), Some(0.5));
        // A short intro, or one that already is a gap, gets nothing added.
        assert_eq!(parse("[00:03.00]hi").with_intro_gap().lines.len(), 1);
        assert_eq!(
            parse("[00:09.00]\n[00:12.00]hi")
                .with_intro_gap()
                .lines
                .len(),
            2
        );
        assert!(Lyrics::default().with_intro_gap().is_empty());
    }

    #[test]
    fn line_offsets_add_up_the_heights_between() {
        let even = |_: usize| 10.0;
        // Halfway from line 2 to line 3: line 2's top is 5 above the point, line 3's 5 below.
        assert_eq!(line_offset(2.5, 2, even), -5.0);
        assert_eq!(line_offset(2.5, 3, even), 5.0);
        assert_eq!(line_offset(2.5, 5, even), 25.0);
        assert_eq!(line_offset(2.5, 0, even), -25.0);
        // On a line exactly: its own top is at the point.
        assert_eq!(line_offset(4.0, 4, even), 0.0);
        // Different heights (a wrapped line is taller): line 1 is 30 tall.
        let tall = |i: usize| if i == 1 { 30.0 } else { 10.0 };
        assert_eq!(line_offset(1.5, 1, tall), -15.0);
        assert_eq!(line_offset(1.5, 2, tall), 15.0);
        assert_eq!(line_offset(1.5, 0, tall), -25.0);
        // Odd input is held to something sensible.
        assert_eq!(line_offset(-3.0, 0, even), 0.0);
        assert_eq!(line_offset(f32::NAN, 1, even), 10.0);
    }

    fn times(l: &Lyrics) -> Vec<f32> {
        l.lines.iter().map(|x| x.time).collect()
    }

    #[test]
    fn basic_lines() {
        let l = parse("[00:12.50]first words\n[00:15.00]second words\n");
        assert_eq!(l.lines.len(), 2);
        assert_eq!(l.lines[0].time, 12.5);
        assert_eq!(l.lines[0].text, "first words");
        assert_eq!(l.lines[1].time, 15.0);
    }

    #[test]
    fn stamp_formats() {
        let l = parse("[01:02]a\n[01:02.5]b\n[01:02.25]c\n[01:02.125]d\n[01:02:50]e\n[100:00.00]f");
        assert_eq!(times(&l), vec![62.0, 62.125, 62.25, 62.5, 62.5, 6000.0]);
    }

    #[test]
    fn several_stamps_share_one_text() {
        let l = parse("[00:10.00][00:30.00][00:50.00]chorus line");
        assert_eq!(times(&l), vec![10.0, 30.0, 50.0]);
        assert!(l.lines.iter().all(|x| x.text == "chorus line"));
    }

    #[test]
    fn output_is_sorted_even_if_the_file_is_not() {
        let l = parse("[00:30.00]later\n[00:10.00]earlier");
        assert_eq!(l.lines[0].text, "earlier");
        assert_eq!(l.lines[1].text, "later");
    }

    #[test]
    fn metadata_tags_and_plain_lines_are_ignored() {
        let l =
            parse("[ar:Somebody]\n[ti:Something]\n[length:03:20]\nno stamp here\n\n[00:01.00]real");
        assert_eq!(l.lines.len(), 1);
        assert_eq!(l.lines[0].text, "real");
    }

    #[test]
    fn empty_text_is_kept_as_a_gap() {
        let l = parse("[00:05.00]words\n[00:09.00]\n[00:20.00]more");
        assert_eq!(l.lines.len(), 3);
        assert_eq!(l.lines[1].text, "");
    }

    #[test]
    fn offset_shifts_every_line() {
        let l = parse("[offset:+500]\n[00:10.00]a\n[00:00.20]b");
        assert_eq!(times(&l), vec![0.0, 9.5]);
        let l = parse("[offset: -1000]\n[00:10.00]a");
        assert_eq!(times(&l), vec![11.0]);
    }

    #[test]
    fn brackets_inside_the_words_survive() {
        let l = parse("[00:10.00]hello [world] again\n[00:12.00][laughs] and on");
        assert_eq!(l.lines[0].text, "hello [world] again");
        assert_eq!(l.lines[1].text, "[laughs] and on");
    }

    #[test]
    fn per_word_time_tags_are_removed() {
        let l = parse(
            "[00:10.00]<00:10.00>one <00:10.40>two  <00:10.90>three\n[00:12.00] <00:12.00>four<00:12.50>five",
        );
        assert_eq!(l.lines[0].text, "one two three");
        assert_eq!(l.lines[1].text, "four five");
        // Angle brackets that are not time tags are ordinary text.
        let l = parse("[00:01.00]a < b and <3 and <b>bold</b>");
        assert_eq!(l.lines[0].text, "a < b and <3 and <b>bold</b>");
        // A line made only of word tags is an instrumental gap.
        assert_eq!(parse("[00:05.00]<00:05.00>").lines[0].text, "");
    }

    #[test]
    fn malformed_stamps_are_not_lines() {
        for bad in [
            "[1:2:3:4]x",
            "[aa:bb]x",
            "[00:61.00]x",
            "[00:]x",
            "[:10]x",
            "[00:10.xx]x",
            "[00:10",
            "]",
        ] {
            assert!(parse(bad).is_empty(), "{bad}");
        }
    }

    #[test]
    fn windows_line_endings_and_whitespace() {
        let l = parse("  [00:01.00]  spaced out  \r\n[00:02.00]next\r\n");
        assert_eq!(l.lines[0].text, "spaced out");
        assert_eq!(l.lines[1].text, "next");
    }

    #[test]
    fn index_at_positions() {
        let l = parse("[00:10.00]a\n[00:20.00]b\n[00:30.00]c");
        assert_eq!(l.index_at(0.0), None);
        assert_eq!(l.index_at(9.99), None);
        assert_eq!(l.index_at(10.0), Some(0));
        assert_eq!(l.index_at(19.99), Some(0));
        assert_eq!(l.index_at(20.0), Some(1));
        assert_eq!(l.index_at(999.0), Some(2));
        assert_eq!(Lyrics::default().index_at(5.0), None);
    }

    #[test]
    fn smooth_index_glides_into_the_next_line() {
        let l = parse("[00:10.00]a\n[00:20.00]b");
        assert_eq!(l.smooth_index(0.0, 0.4), -1.0);
        assert_eq!(l.smooth_index(10.0, 0.4), 0.0);
        assert_eq!(l.smooth_index(15.0, 0.4), 0.0);
        let mid = l.smooth_index(19.8, 0.4);
        assert!((mid - 0.5).abs() < 1e-3, "{mid}");
        assert_eq!(l.smooth_index(20.0, 0.4), 1.0);
        assert_eq!(l.smooth_index(50.0, 0.4), 1.0);
        // Monotonic through the glide.
        let mut last = -1.0;
        for i in 0..300 {
            let v = l.smooth_index(i as f32 * 0.1, 0.4);
            assert!(v >= last);
            last = v;
        }
    }
}
