//! Music helpers that need no OS: time text, source filtering, LRCLIB requests, the audio
//! spectrum for the visualizer, and colours taken from album art.

use serde::Deserialize;

use crate::{settings::SourceMode, theme::Rgba};

/// "3:07", or "1:02:07" from an hour up.
pub fn format_clock(seconds: f32) -> String {
    let total = if seconds.is_finite() {
        seconds.max(0.0) as u32
    } else {
        0
    };
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Should the music card show this app? `app_id` is the media session's app identifier, for
/// example `Spotify.exe` or `SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify`. List entries match
/// anywhere in it, without regard to case.
pub fn source_allowed(mode: SourceMode, apps: &[String], app_id: &str) -> bool {
    let id = app_id.to_lowercase();
    let listed = apps
        .iter()
        .any(|a| !a.trim().is_empty() && id.contains(&a.trim().to_lowercase()));
    match mode {
        SourceMode::All => true,
        SourceMode::Only => listed,
        SourceMode::Except => !listed,
    }
}

/// Music videos (mostly from browsers) run on a different clock from the album track, so
/// synced lyrics would be off. Recognised by the usual title markers.
pub fn looks_like_music_video(title: &str) -> bool {
    let t = title.to_lowercase();
    [
        "official video",
        "official music video",
        "music video",
        "official mv",
        "lyric video",
        "lyrics video",
        "(video)",
        "[video]",
        "official visualizer",
        "(live",
        "[live",
    ]
    .iter()
    .any(|marker| t.contains(marker))
}

/// Identifies a song for caching: the same title, artist and length is the same song.
pub fn track_key(title: &str, artist: &str, duration: f32) -> String {
    format!(
        "{}|{}|{}",
        title.trim().to_lowercase(),
        artist.trim().to_lowercase(),
        duration.round() as i64
    )
}

/// Stable 64-bit hash (FNV-1a) used for cache file names.
pub fn stable_hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

pub fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Exact-match LRCLIB request. Album and duration make the match stricter when known.
pub fn lrclib_get_url(title: &str, artist: &str, album: &str, duration: f32) -> String {
    let mut url = format!(
        "https://lrclib.net/api/get?track_name={}&artist_name={}",
        percent_encode(title.trim()),
        percent_encode(artist.trim())
    );
    if !album.trim().is_empty() {
        url.push_str(&format!("&album_name={}", percent_encode(album.trim())));
    }
    if duration > 0.0 {
        url.push_str(&format!("&duration={}", duration.round() as i64));
    }
    url
}

/// Looser LRCLIB request, used when the exact one finds nothing.
pub fn lrclib_search_url(title: &str, artist: &str) -> String {
    format!(
        "https://lrclib.net/api/search?track_name={}&artist_name={}",
        percent_encode(title.trim()),
        percent_encode(artist.trim())
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LrclibRecord {
    #[serde(default)]
    duration: f32,
    #[serde(default)]
    synced_lyrics: Option<String>,
}

fn usable(record: &LrclibRecord) -> Option<String> {
    record
        .synced_lyrics
        .as_ref()
        .filter(|s| !s.trim().is_empty())
        .cloned()
}

/// Synced lyrics from an `/api/get` response, if it has any.
pub fn parse_lrclib_get(json: &str) -> Option<String> {
    usable(&serde_json::from_str::<LrclibRecord>(json).ok()?)
}

/// The best synced lyrics from an `/api/search` response: the first result whose length is
/// within three seconds of the song being played (any length if ours is unknown).
pub fn parse_lrclib_search(json: &str, duration: f32) -> Option<String> {
    let records: Vec<LrclibRecord> = serde_json::from_str(json).ok()?;
    records
        .iter()
        .filter(|r| duration <= 0.0 || (r.duration - duration).abs() <= 3.0)
        .find_map(usable)
}

// ---- visualizer -----------------------------------------------------------------------------

/// Samples per analysis window. Must be a power of two.
pub const FFT_SIZE: usize = 1024;
/// Number of bars the visualizer draws.
pub const BANDS: usize = 6;
/// Band edges in hertz, low to high.
const EDGES: [f32; BANDS + 1] = [60.0, 150.0, 400.0, 1000.0, 2400.0, 6000.0, 14000.0];

/// In-place radix-2 FFT. `re.len()` must equal `im.len()` and be a power of two.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    if n < 2 || !n.is_power_of_two() || im.len() != n {
        return;
    }
    // Bit-reversal permutation.
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -2.0 * std::f32::consts::PI / len as f32;
        let (step_re, step_im) = (angle.cos(), angle.sin());
        for start in (0..n).step_by(len) {
            let (mut w_re, mut w_im) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let t_re = re[b] * w_re - im[b] * w_im;
                let t_im = re[b] * w_im + im[b] * w_re;
                re[b] = re[a] - t_re;
                im[b] = im[a] - t_im;
                re[a] += t_re;
                im[a] += t_im;
                let next_re = w_re * step_re - w_im * step_im;
                w_im = w_re * step_im + w_im * step_re;
                w_re = next_re;
            }
        }
        len <<= 1;
    }
}

/// Loudness of each visualizer band (linear amplitude, roughly 0..1 for full-scale input) for
/// one window of mono samples. Fewer than `FFT_SIZE` samples yields silence.
pub fn band_levels(samples: &[f32], sample_rate: f32) -> [f32; BANDS] {
    let mut out = [0.0; BANDS];
    if samples.len() < FFT_SIZE || sample_rate <= 0.0 {
        return out;
    }
    let mut re = [0.0f32; FFT_SIZE];
    let mut im = [0.0f32; FFT_SIZE];
    for (i, (dst, src)) in re
        .iter_mut()
        .zip(&samples[samples.len() - FFT_SIZE..])
        .enumerate()
    {
        // Hann window, so a tone between two bins does not smear across the spectrum.
        let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos();
        *dst = src * w;
    }
    fft(&mut re, &mut im);
    let bin_hz = sample_rate / FFT_SIZE as f32;
    for (band, level) in out.iter_mut().enumerate() {
        let lo = ((EDGES[band] / bin_hz).floor() as usize).clamp(1, FFT_SIZE / 2 - 1);
        let hi = ((EDGES[band + 1] / bin_hz).ceil() as usize).clamp(lo + 1, FFT_SIZE / 2);
        let peak = (lo..hi)
            .map(|i| (re[i] * re[i] + im[i] * im[i]).sqrt())
            .fold(0.0f32, f32::max);
        // Undo the FFT gain (N/2) and the Hann window's amplitude loss (0.5).
        *level = peak / (FFT_SIZE as f32 / 4.0);
    }
    out
}

/// Turn raw band levels into bar heights 0..1 with automatic gain, so quiet and loud music
/// both fill the bars. `peak` is running state owned by the caller (start it at 0).
pub fn normalize_bands(levels: [f32; BANDS], peak: &mut f32) -> [f32; BANDS] {
    const SILENCE: f32 = 1e-5;
    let loudest = levels.iter().copied().fold(0.0f32, f32::max);
    if loudest < SILENCE {
        return [0.0; BANDS];
    }
    // The reference level falls slowly and jumps up at once.
    *peak = (*peak * 0.995).max(loudest).max(SILENCE);
    levels.map(|l| (l / *peak).clamp(0.0, 1.0).sqrt())
}

// ---- album art ------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// The cover's main colour.
    pub dominant: Rgba,
    /// A light tint of it that reads well on a dark background (lyrics, bars).
    pub accent: Rgba,
    /// Up to three distinct colours of the cover, strongest first (filled up with neighbours of
    /// the main colour): the resting pill's gradient and the visualizer's colours.
    pub colors: [Rgba; 3],
}

impl Default for Palette {
    fn default() -> Self {
        let grey = Rgba::rgb(0x40_40_46);
        Self {
            dominant: grey,
            accent: Rgba::rgb(0xFF_FF_FF),
            colors: [grey; 3],
        }
    }
}

/// The longest song title the resting pill shows, in characters.
pub const PILL_TITLE_MAX: usize = 30;

/// The title as the resting pill shows it: at most `PILL_TITLE_MAX` characters, a longer one
/// cut with "…" (which counts as one of them).
pub fn pill_title(title: &str) -> String {
    let title = title.trim();
    if title.chars().count() <= PILL_TITLE_MAX {
        return title.to_string();
    }
    let mut cut: String = title.chars().take(PILL_TITLE_MAX - 1).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    cut
}

/// Brightest relative luminance of the resting pill's song colours: white text keeps a
/// contrast of about 4:1 or more on it.
pub const PILL_TINT_LUMINANCE: f32 = 0.2;

/// One of the resting pill's song colours: the cover colour at full value with its saturation
/// raised (`richer`, so a pale cover still gives a clear colour), darkened to
/// `PILL_TINT_LUMINANCE` (a dark colour is not brightened past 0.85 of full value), hue kept;
/// a grey stays grey.
pub fn pill_tint(c: Rgba) -> Rgba {
    let luminance = |c: Rgba| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
    let max = c.r.max(c.g).max(c.b).max(1e-3);
    let vivid = Rgba::new(c.r / max, c.g / max, c.b / max, 1.0);
    let sat = 1.0 - vivid.r.min(vivid.g).min(vivid.b);
    let k = if sat > 1e-4 { richer(sat) / sat } else { 1.0 };
    let push = |x: f32| 1.0 - (1.0 - x) * k;
    let vivid = Rgba::new(push(vivid.r), push(vivid.g), push(vivid.b), 1.0);
    let k = (PILL_TINT_LUMINANCE / luminance(vivid).max(1e-3)).min(0.85);
    Rgba::new(vivid.r * k, vivid.g * k, vivid.b * k, 1.0)
}

/// The pill's saturation for a cover colour's saturation `s` (0..1): pale colours are raised a
/// lot (0.15 → 0.46), strong ones a little (0.6 → 0.8), and a near-grey one (below about 0.03)
/// barely, so a grey cover stays grey.
fn richer(s: f32) -> f32 {
    let s = s.clamp(0.0, 1.0);
    let target = (0.35 + 0.75 * s).min(1.0);
    let ramp = ((s - 0.02) / 0.06).clamp(0.0, 1.0);
    let ramp = ramp * ramp * (3.0 - 2.0 * ramp);
    s + (target - s) * ramp
}

/// Colour stops across the Music card's background glow.
pub const GLOW_STOPS: usize = 5;

/// Move the background glow towards the music: each stop follows the band curve at its place
/// (bass on the left, treble on the right), rising fast and falling slowly so it breathes
/// instead of flickering. `dt` is the time since the last step, in seconds.
pub fn glow_step(glow: &mut [f32; GLOW_STOPS], bands: &[f32; BANDS], dt: f32) {
    let dt = dt.clamp(0.0, 0.5);
    for (i, g) in glow.iter_mut().enumerate() {
        let target = smooth_level(bands, i as f32 / (GLOW_STOPS - 1) as f32);
        let rate = if target > *g { 14.0 } else { 2.2 };
        *g = (*g + (target - *g) * (1.0 - (-rate * dt).exp())).clamp(0.0, 1.0);
    }
}

/// How long the flash a new lyric line gives the glow lasts, in seconds.
pub const PULSE_SECONDS: f32 = 0.7;

/// The line pulse after `dt` seconds: 1 right when a line starts, 0 after `PULSE_SECONDS`.
pub fn pulse_step(pulse: f32, dt: f32) -> f32 {
    (pulse - dt.max(0.0) / PULSE_SECONDS).max(0.0)
}

/// The level at `x` (0 = the first band, 1 = the last) on a smooth curve through the bands:
/// eased (cosine) between neighbours, so a wave drawn from it has no corners.
pub fn smooth_level(bands: &[f32; BANDS], x: f32) -> f32 {
    let pos = x.clamp(0.0, 1.0) * (BANDS - 1) as f32;
    let i = (pos as usize).min(BANDS - 2);
    let f = pos - i as f32;
    let f = (1.0 - (f * std::f32::consts::PI).cos()) / 2.0;
    let (a, b) = (bands[i].clamp(0.0, 1.0), bands[i + 1].clamp(0.0, 1.0));
    a + (b - a) * f
}

/// Which band each of the six bar positions shows for the "burst" style: the lowest band in
/// the middle, higher ones outwards on alternating sides, so the bars swell from the centre.
pub const BURST_ORDER: [usize; BANDS] = [4, 2, 0, 1, 3, 5];

/// A point on the loop through three colours: 0 = the first, 1/3 the second, 2/3 the third,
/// and back to the first at 1. Blends smoothly (eased) between neighbours.
pub fn cycle_color(colors: &[Rgba; 3], t: f32) -> Rgba {
    let t = t.rem_euclid(1.0) * 3.0;
    let i = (t as usize).min(2);
    let f = t - i as f32;
    let f = f * f * (3.0 - 2.0 * f);
    let (a, b) = (colors[i], colors[(i + 1) % 3]);
    let mix = |x: f32, y: f32| x + (y - x) * f;
    Rgba::new(mix(a.r, b.r), mix(a.g, b.g), mix(a.b, b.b), mix(a.a, b.a))
}

/// The resting pill's gradient stops (position along the pill, colour) at drift `phase`: the
/// pill shows a little over half of the colour loop at once, and the loop slides along as the
/// phase grows, so the colours flow slowly from one end to the other.
pub fn pill_stops(colors: &[Rgba; 3], phase: f32) -> [(f32, Rgba); 5] {
    let mut out = [(0.0, colors[0]); 5];
    for (i, stop) in out.iter_mut().enumerate() {
        let x = i as f32 / 4.0;
        *stop = (x, cycle_color(colors, phase + x * 0.55));
    }
    out
}

/// How present (colour weight) a second or third colour must be, next to the main one.
const ACCENT_SHARE: f32 = 0.025;
/// The least colour weight per sampled pixel a second or third colour needs.
const ACCENT_MIN: f32 = 0.0015;

/// Pick colours from BGRA pixels: the most present vivid hue, or the average for a grey cover.
pub fn palette(bgra: &[u8]) -> Palette {
    const HUES: usize = 12;
    let mut buckets = [(0.0f32, 0.0f32, 0.0f32, 0.0f32); HUES]; // weight, r, g, b
    let (mut sum, mut count) = ((0.0f32, 0.0f32, 0.0f32), 0.0f32);
    // Every fourth pixel is plenty.
    for px in bgra.as_chunks::<4>().0.iter().step_by(4) {
        let (b, g, r) = (
            f32::from(px[0]) / 255.0,
            f32::from(px[1]) / 255.0,
            f32::from(px[2]) / 255.0,
        );
        sum = (sum.0 + r, sum.1 + g, sum.2 + b);
        count += 1.0;
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let chroma = max - min;
        if max < 0.15 || chroma < 0.08 {
            continue; // near black or near grey: no usable hue
        }
        let hue = if max == r {
            ((g - b) / chroma).rem_euclid(6.0)
        } else if max == g {
            (b - r) / chroma + 2.0
        } else {
            (r - g) / chroma + 4.0
        };
        let slot = ((hue / 6.0 * HUES as f32) as usize).min(HUES - 1);
        let weight = chroma * max;
        let bucket = &mut buckets[slot];
        *bucket = (
            bucket.0 + weight,
            bucket.1 + r * weight,
            bucket.2 + g * weight,
            bucket.3 + b * weight,
        );
    }
    if count == 0.0 {
        return Palette::default();
    }
    let average = Rgba::new(sum.0 / count, sum.1 / count, sum.2 / count, 1.0);
    let best = buckets
        .iter()
        .copied()
        .fold((0.0, 0.0, 0.0, 0.0), |a, b| if b.0 > a.0 { b } else { a });
    // Too little colour overall: treat the cover as grey.
    if best.0 / count < 0.004 {
        return Palette {
            dominant: average,
            accent: Rgba::rgb(0xFF_FF_FF),
            colors: companions(average, &[]),
        };
    }
    let dominant = Rgba::new(best.1 / best.0, best.2 / best.0, best.3 / best.0, 1.0);
    // Other colours the cover really has: hue groups with at least `ACCENT_SHARE` of the main
    // one's weight (so a small accent on a cover that is 90% one colour still counts) and a
    // little colour of their own overall (not stray noise), at least two groups (60°) from
    // every colour already taken, strongest first.
    let mut order: Vec<usize> = (0..HUES).filter(|i| buckets[*i].0 > 0.0).collect();
    order.sort_by(|a, b| buckets[*b].0.total_cmp(&buckets[*a].0));
    let mut taken: Vec<usize> = Vec::new();
    let mut found: Vec<Rgba> = Vec::new();
    for slot in order {
        let bucket = buckets[slot];
        let far = taken.iter().all(|t| {
            let d = slot.abs_diff(*t);
            d.min(HUES - d) >= 2
        });
        if bucket.0 >= best.0 * ACCENT_SHARE && bucket.0 / count >= ACCENT_MIN && far {
            taken.push(slot);
            found.push(Rgba::new(
                bucket.1 / bucket.0,
                bucket.2 / bucket.0,
                bucket.3 / bucket.0,
                1.0,
            ));
            if found.len() == 3 {
                break;
            }
        }
    }
    // Brighten to full value, then pull a third of the way to white.
    let max = dominant.r.max(dominant.g).max(dominant.b).max(1e-3);
    let lift = |c: f32| (c / max) * 0.65 + 0.35;
    Palette {
        dominant,
        accent: Rgba::new(lift(dominant.r), lift(dominant.g), lift(dominant.b), 1.0),
        colors: companions(dominant, &found),
    }
}

/// Three colours for the music pill's gradient: the cover's own distinct colours (`found`,
/// strongest first). A cover with fewer is topped up with softer shades of its main colour,
/// never with a hue the cover does not have (turning red's hue made pink on covers with no
/// pink), so a one-colour cover still gives a gradient that moves.
fn companions(main: Rgba, found: &[Rgba]) -> [Rgba; 3] {
    let mut out = [main; 3];
    for (slot, c) in out.iter_mut().zip(found.iter()) {
        *slot = *c;
    }
    match found.len() {
        0 | 1 => {
            out[1] = mute(out[0], 0.4);
            out[2] = mute(out[0], 0.7);
        }
        2 => out[2] = mute(out[0], 0.5),
        _ => {}
    }
    out
}

/// The same colour with its saturation scaled by `keep` (0 = grey, 1 = unchanged); the hue and
/// the brightest channel stay.
pub fn mute(c: Rgba, keep: f32) -> Rgba {
    let max = c.r.max(c.g).max(c.b);
    let keep = keep.clamp(0.0, 1.0);
    let pull = |x: f32| max - (max - x) * keep;
    Rgba::new(pull(c.r), pull(c.g), pull(c.b), c.a)
}

/// A blurred, darkened copy of BGRA pixels, used behind the music card and lyrics.
/// `darken` is how much light to remove, 0..1.
pub fn soften(bgra: &[u8], width: usize, height: usize, radius: usize, darken: f32) -> Vec<u8> {
    if width == 0 || height == 0 || bgra.len() != width * height * 4 {
        return bgra.to_vec();
    }
    let mut a = bgra.to_vec();
    let mut b = vec![0u8; a.len()];
    let r = radius.max(1);
    // Three box blurs approximate a Gaussian.
    for _ in 0..3 {
        box_blur(&a, &mut b, width, height, r, true);
        box_blur(&b, &mut a, width, height, r, false);
    }
    let keep = (1.0 - darken.clamp(0.0, 1.0)) * 256.0;
    let keep = keep as u32;
    for px in a.as_chunks_mut::<4>().0 {
        for c in &mut px[..3] {
            *c = ((u32::from(*c) * keep) >> 8) as u8;
        }
        px[3] = 255;
    }
    a
}

/// One box-blur pass along rows (`horizontal`) or columns, clamping at the edges.
fn box_blur(
    src: &[u8],
    dst: &mut [u8],
    width: usize,
    height: usize,
    radius: usize,
    horizontal: bool,
) {
    let (lines, len) = if horizontal {
        (height, width)
    } else {
        (width, height)
    };
    let index = |line: usize, i: usize| {
        if horizontal {
            (line * width + i) * 4
        } else {
            (i * width + line) * 4
        }
    };
    let window = (2 * radius + 1) as u32;
    for line in 0..lines {
        for channel in 0..3 {
            let at = |i: isize| {
                u32::from(src[index(line, i.clamp(0, len as isize - 1) as usize) + channel])
            };
            let mut sum: u32 = (-(radius as isize)..=radius as isize).map(at).sum();
            for i in 0..len {
                dst[index(line, i) + channel] = (sum / window) as u8;
                sum += at(i as isize + radius as isize + 1);
                sum -= at(i as isize - radius as isize);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pill_titles_are_capped_at_thirty_characters() {
        assert_eq!(pill_title("  Breaking the Habit "), "Breaking the Habit");
        let exact = "a".repeat(30);
        assert_eq!(pill_title(&exact), exact);
        let long = "The Quick Brown Fox Jumps Over The Lazy Dog";
        let cut = pill_title(long);
        assert_eq!(cut.chars().count(), 30);
        assert_eq!(cut, "The Quick Brown Fox Jumps Ove…");
        // A cut that lands after a space does not leave the space before the "…".
        assert_eq!(
            pill_title(&format!("{} tail end", "b".repeat(28))),
            format!("{}…", "b".repeat(28))
        );
        assert_eq!(pill_title("日本語".repeat(20).as_str()).chars().count(), 30);
    }

    #[test]
    fn pill_tint_keeps_the_hue_and_stays_dark() {
        let lum = |c: Rgba| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
        for c in [
            Rgba::new(1.0, 0.85, 0.2, 1.0), // bright yellow: must be darkened a lot
            Rgba::new(0.1, 0.2, 0.7, 1.0),  // blue
            Rgba::new(0.05, 0.02, 0.02, 1.0),
            Rgba::new(0.5, 0.5, 0.5, 1.0), // grey stays grey
        ] {
            let top = pill_tint(c);
            assert!(lum(top) <= PILL_TINT_LUMINANCE + 0.001, "{c:?} -> {top:?}");
            // The strongest channel of the cover is still the strongest.
            let strongest = |x: Rgba| {
                if x.r >= x.g && x.r >= x.b {
                    0
                } else if x.g >= x.b {
                    1
                } else {
                    2
                }
            };
            assert_eq!(strongest(top), strongest(c));
        }
        let grey = pill_tint(Rgba::new(0.5, 0.5, 0.5, 1.0));
        assert!((grey.r - grey.g).abs() < 1e-4 && (grey.g - grey.b).abs() < 1e-4);
    }

    #[test]
    fn pale_covers_give_a_richer_pill_colour() {
        let sat = |c: Rgba| {
            let max = c.r.max(c.g).max(c.b);
            (max - c.r.min(c.g).min(c.b)) / max
        };
        // A pale blue (saturation 0.15) comes out clearly blue, about three times as rich.
        let pale = Rgba::new(0.68, 0.72, 0.8, 1.0);
        let tint = pill_tint(pale);
        assert!(sat(tint) > 2.5 * sat(pale), "{tint:?}");
        assert!(tint.b > tint.g && tint.g > tint.r, "hue kept: {tint:?}");
        // Strong colours get only a little stronger; a near-grey stays near grey.
        assert!((richer(0.6) - 0.8).abs() < 1e-4);
        assert!(richer(0.95) <= 1.0);
        assert!(richer(0.01) - 0.01 < 1e-6);
        let near_grey = pill_tint(Rgba::new(0.656, 0.652, 0.649, 1.0));
        assert!(sat(near_grey) < 0.02, "{near_grey:?}");
    }

    #[test]
    fn a_small_accent_on_a_one_colour_cover_still_shows() {
        // 90% red with a 6% blue and a 4% green corner: all three colours make the gradient.
        let px = stripes(&[(220, 40, 40)], 900)
            .into_iter()
            .chain(stripes(&[(40, 60, 220)], 60))
            .chain(stripes(&[(40, 200, 60)], 40))
            .collect::<Vec<u8>>();
        let [a, b, c] = palette(&px).colors;
        assert!(a.r > a.g && a.r > a.b, "red first: {a:?}");
        assert!(b.b > b.r, "then blue: {b:?}");
        assert!(c.g > c.r && c.g > c.b, "then green: {c:?}");
        // A speck (well under 1%) is not taken for a colour of the cover.
        let px = stripes(&[(220, 40, 40)], 2000)
            .into_iter()
            .chain(stripes(&[(40, 60, 220)], 4))
            .collect::<Vec<u8>>();
        let [_, b, c] = palette(&px).colors;
        assert!(b.r > b.b && c.r > c.b, "only shades of red: {b:?} {c:?}");
    }

    /// BGRA pixels: `n` of each colour, one after the other.
    fn stripes(colors: &[(u8, u8, u8)], n: usize) -> Vec<u8> {
        colors
            .iter()
            .flat_map(|(r, g, b)| std::iter::repeat_n([*b, *g, *r, 255], n))
            .flatten()
            .collect()
    }

    #[test]
    fn a_cover_with_several_colours_gives_them_all() {
        // Half red, a third blue, the rest green: three distinct colours, strongest first.
        let px = stripes(&[(230, 30, 30), (30, 40, 220), (40, 200, 60)], 0)
            .into_iter()
            .chain(stripes(&[(230, 30, 30)], 600))
            .chain(stripes(&[(30, 40, 220)], 400))
            .chain(stripes(&[(40, 200, 60)], 200))
            .collect::<Vec<u8>>();
        let p = palette(&px);
        let [a, b, c] = p.colors;
        assert!(a.r > a.g && a.r > a.b, "red first: {a:?}");
        assert!(b.b > b.r && b.b > b.g, "then blue: {b:?}");
        assert!(c.g > c.r && c.g > c.b, "then green: {c:?}");
    }

    #[test]
    fn a_one_colour_cover_still_gets_a_moving_gradient() {
        let p = palette(&stripes(&[(220, 40, 40)], 800));
        let [a, b, c] = p.colors;
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(b, c);
        // Softer shades of the same red, never another hue: the reddest channel stays the
        // brightest, green and blue rise together (no pink, no orange).
        for shade in [b, c] {
            assert!(shade.r > shade.g, "{shade:?}");
            assert!((shade.g - shade.b).abs() < 1e-4, "{shade:?}");
            assert!(shade.g > a.g, "{shade:?}");
            let (ta, ts) = (pill_tint(a), pill_tint(shade));
            assert!(ts.b - ts.g < 1e-4 && ta.r > ta.b, "{ts:?}");
        }
        // A grey cover stays grey in all three.
        let g = palette(&stripes(&[(120, 120, 120)], 400));
        assert!(g.colors.iter().all(|c| (c.r - c.g).abs() < 1e-3));
    }

    #[test]
    fn the_wave_passes_through_every_band() {
        let bands = [0.0, 1.0, 0.5, 0.25, 0.75, 0.0];
        for (i, b) in bands.iter().enumerate() {
            let x = i as f32 / (BANDS - 1) as f32;
            assert!((smooth_level(&bands, x) - b).abs() < 1e-5, "band {i}");
        }
        // Halfway between 0 and 1 the eased curve is halfway too, and stays in range.
        assert!((smooth_level(&bands, 0.1) - 0.5).abs() < 1e-4);
        assert_eq!(smooth_level(&bands, -3.0), 0.0);
        assert_eq!(smooth_level(&[2.0; BANDS], 0.5), 1.0);
        // The burst order uses every band once, lowest in the middle.
        let mut seen = BURST_ORDER;
        seen.sort_unstable();
        assert_eq!(seen, [0, 1, 2, 3, 4, 5]);
        assert_eq!(BURST_ORDER[2], 0);
    }

    #[test]
    fn the_colour_loop_passes_every_colour_and_wraps() {
        let c = [
            Rgba::new(1.0, 0.0, 0.0, 1.0),
            Rgba::new(0.0, 1.0, 0.0, 1.0),
            Rgba::new(0.0, 0.0, 1.0, 1.0),
        ];
        assert_eq!(cycle_color(&c, 0.0), c[0]);
        assert_eq!(cycle_color(&c, 1.0 / 3.0), c[1]);
        assert!(cycle_color(&c, 2.0 / 3.0 + 1e-6).b > 0.99);
        assert_eq!(cycle_color(&c, 1.0), c[0]);
        assert_eq!(cycle_color(&c, -1.0 / 3.0 * 2.0), c[1]);
        let half = cycle_color(&c, 1.0 / 6.0);
        assert!((half.r - 0.5).abs() < 1e-4 && (half.g - 0.5).abs() < 1e-4);
        // Stops run from the pill's left (0) to its right (1) and drift with the phase.
        let s = pill_stops(&c, 0.0);
        assert_eq!((s[0].0, s[4].0), (0.0, 1.0));
        assert_eq!(s[0].1, c[0]);
        assert_ne!(pill_stops(&c, 0.1)[0].1, s[0].1);
    }

    #[test]
    fn the_glow_follows_the_music_up_fast_and_down_slowly() {
        let mut glow = [0.0; GLOW_STOPS];
        let loud = [1.0; BANDS];
        glow_step(&mut glow, &loud, 0.1);
        assert!(glow.iter().all(|g| *g > 0.7), "rises fast: {glow:?}");
        let up = glow;
        glow_step(&mut glow, &[0.0; BANDS], 0.1);
        assert!(
            glow.iter()
                .zip(up)
                .all(|(now, was)| *now < was && *now > was * 0.7),
            "falls slowly: {glow:?}"
        );
        // Bass on the left only lights the left stops most.
        let mut glow = [0.0; GLOW_STOPS];
        let bass = [1.0, 0.6, 0.3, 0.1, 0.0, 0.0];
        for _ in 0..20 {
            glow_step(&mut glow, &bass, 0.05);
        }
        assert!(glow.windows(2).all(|w| w[0] > w[1]), "{glow:?}");
        // Whatever comes in, the values stay in range; a stalled clock does not jump.
        glow_step(&mut glow, &[9.0; BANDS], 99.0);
        assert!(glow.iter().all(|g| (0.0..=1.0).contains(g)));
        let before = glow;
        glow_step(&mut glow, &[0.0; BANDS], 0.0);
        assert_eq!(glow, before);
    }

    #[test]
    fn the_line_pulse_fades_out() {
        assert_eq!(pulse_step(1.0, 0.0), 1.0);
        assert!((pulse_step(1.0, PULSE_SECONDS / 2.0) - 0.5).abs() < 1e-5);
        assert_eq!(pulse_step(1.0, 5.0), 0.0);
        assert_eq!(pulse_step(0.0, 1.0), 0.0);
    }

    #[test]
    fn muting_keeps_the_hue_and_the_brightest_channel() {
        let red = Rgba::new(0.9, 0.1, 0.2, 1.0);
        let same = mute(red, 1.0);
        assert!((same.g - red.g).abs() < 1e-5 && (same.b - red.b).abs() < 1e-5);
        let grey = mute(red, 0.0);
        assert!((grey.r - 0.9).abs() < 1e-5 && (grey.g - 0.9).abs() < 1e-5);
        let half = mute(red, 0.5);
        assert!((half.r - 0.9).abs() < 1e-5);
        assert!((half.g - 0.5).abs() < 1e-5 && (half.b - 0.55).abs() < 1e-5);
    }

    #[test]
    fn clock_text() {
        assert_eq!(format_clock(0.0), "0:00");
        assert_eq!(format_clock(7.9), "0:07");
        assert_eq!(format_clock(187.0), "3:07");
        assert_eq!(format_clock(3727.0), "1:02:07");
        assert_eq!(format_clock(-5.0), "0:00");
        assert_eq!(format_clock(f32::NAN), "0:00");
    }

    #[test]
    fn source_filter_modes() {
        let apps = vec!["Spotify".to_string(), " ".to_string()];
        let spotify = "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify";
        assert!(source_allowed(SourceMode::All, &apps, "chrome.exe"));
        assert!(source_allowed(SourceMode::Only, &apps, spotify));
        assert!(source_allowed(SourceMode::Only, &apps, "spotify.exe"));
        assert!(!source_allowed(SourceMode::Only, &apps, "chrome.exe"));
        assert!(!source_allowed(SourceMode::Except, &apps, spotify));
        assert!(source_allowed(SourceMode::Except, &apps, "chrome.exe"));
        // A blank entry must not match everything.
        assert!(!source_allowed(
            SourceMode::Only,
            &[" ".to_string()],
            "chrome.exe"
        ));
        assert!(!source_allowed(SourceMode::Only, &[], "chrome.exe"));
    }

    #[test]
    fn music_video_titles() {
        assert!(looks_like_music_video("Artist - Song (Official Video)"));
        assert!(looks_like_music_video("Song [Official Music Video] 4K"));
        assert!(looks_like_music_video("Song (Lyric Video)"));
        assert!(looks_like_music_video("Song (Live at Wembley)"));
        assert!(!looks_like_music_video("Video Games"));
        assert!(!looks_like_music_video("Alive"));
        assert!(!looks_like_music_video("Song (Remastered 2011)"));
    }

    #[test]
    fn keys_and_hashes_are_stable() {
        assert_eq!(track_key(" Song ", "ARTIST", 213.4), "song|artist|213");
        assert_eq!(
            track_key("Song", "Artist", 213.4),
            track_key("song", "artist", 212.6)
        );
        assert_ne!(
            track_key("Song", "Artist", 213.0),
            track_key("Song", "Artist", 300.0)
        );
        assert_eq!(stable_hash("abc"), stable_hash("abc"));
        assert_ne!(stable_hash("abc"), stable_hash("abd"));
    }

    #[test]
    fn percent_encoding() {
        assert_eq!(percent_encode("AZaz09-_.~"), "AZaz09-_.~");
        assert_eq!(percent_encode("a b&c=d/e?"), "a%20b%26c%3Dd%2Fe%3F");
        assert_eq!(percent_encode("é"), "%C3%A9");
    }

    #[test]
    fn lrclib_urls() {
        assert_eq!(
            lrclib_get_url("My Song", "The Band", "An Album", 213.4),
            "https://lrclib.net/api/get?track_name=My%20Song&artist_name=The%20Band&album_name=An%20Album&duration=213"
        );
        assert_eq!(
            lrclib_get_url("S", "A", " ", 0.0),
            "https://lrclib.net/api/get?track_name=S&artist_name=A"
        );
        assert_eq!(
            lrclib_search_url("My Song", "A&B"),
            "https://lrclib.net/api/search?track_name=My%20Song&artist_name=A%26B"
        );
    }

    #[test]
    fn lrclib_get_response() {
        let found = r#"{"id":1,"trackName":"S","duration":213.0,"instrumental":false,
            "plainLyrics":"words","syncedLyrics":"[00:01.00]words"}"#;
        assert_eq!(parse_lrclib_get(found).as_deref(), Some("[00:01.00]words"));
        let plain_only = r#"{"id":1,"duration":213.0,"plainLyrics":"words","syncedLyrics":null}"#;
        assert_eq!(parse_lrclib_get(plain_only), None);
        assert_eq!(
            parse_lrclib_get(r#"{"code":404,"name":"TrackNotFound"}"#),
            None
        );
        assert_eq!(parse_lrclib_get("not json"), None);
    }

    #[test]
    fn lrclib_search_prefers_matching_length() {
        let json = r#"[
            {"duration": 500.0, "syncedLyrics": "[00:01.00]live version"},
            {"duration": 214.0, "syncedLyrics": null},
            {"duration": 212.0, "syncedLyrics": "[00:01.00]album version"}
        ]"#;
        assert_eq!(
            parse_lrclib_search(json, 213.0).as_deref(),
            Some("[00:01.00]album version")
        );
        assert_eq!(
            parse_lrclib_search(json, 0.0).as_deref(),
            Some("[00:01.00]live version")
        );
        assert_eq!(parse_lrclib_search(json, 90.0), None);
        assert_eq!(parse_lrclib_search("[]", 213.0), None);
    }

    fn tone(hz: f32, amplitude: f32) -> Vec<f32> {
        (0..FFT_SIZE)
            .map(|i| amplitude * (2.0 * std::f32::consts::PI * hz * i as f32 / 48_000.0).sin())
            .collect()
    }

    #[test]
    fn a_tone_lands_in_its_band() {
        for (hz, expected) in [
            (100.0, 0),
            (250.0, 1),
            (700.0, 2),
            (1500.0, 3),
            (4000.0, 4),
            (9000.0, 5),
        ] {
            let levels = band_levels(&tone(hz, 0.5), 48_000.0);
            let loudest = levels
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map(|(i, _)| i);
            assert_eq!(loudest, Some(expected), "{hz} Hz gave {levels:?}");
        }
    }

    #[test]
    fn tone_amplitude_is_recovered() {
        let levels = band_levels(&tone(1500.0, 0.5), 48_000.0);
        assert!((levels[3] - 0.5).abs() < 0.1, "{levels:?}");
    }

    #[test]
    fn silence_and_short_input_give_zero() {
        assert_eq!(band_levels(&[0.0; FFT_SIZE], 48_000.0), [0.0; BANDS]);
        assert_eq!(band_levels(&[0.5; 100], 48_000.0), [0.0; BANDS]);
        let mut peak = 0.0;
        assert_eq!(normalize_bands([0.0; BANDS], &mut peak), [0.0; BANDS]);
    }

    #[test]
    fn auto_gain_fills_the_bars_for_quiet_music() {
        let mut peak = 0.0;
        let quiet = normalize_bands(band_levels(&tone(1500.0, 0.01), 48_000.0), &mut peak);
        assert!(quiet[3] > 0.95, "{quiet:?}");
        assert!(quiet.iter().all(|v| (0.0..=1.0).contains(v)));
        // A sudden loud passage raises the reference at once and never exceeds 1.
        let loud = normalize_bands(band_levels(&tone(1500.0, 0.9), 48_000.0), &mut peak);
        assert!(loud.iter().all(|v| *v <= 1.0));
    }

    fn solid(b: u8, g: u8, r: u8, n: usize) -> Vec<u8> {
        [b, g, r, 255].repeat(n)
    }

    #[test]
    fn palette_follows_the_cover() {
        let red = palette(&solid(20, 30, 220, 400));
        assert!(red.dominant.r > 0.8 && red.dominant.g < 0.2);
        assert!(red.accent.r > red.accent.g && red.accent.r > red.accent.b);
        assert!(red.accent.g >= 0.35, "accent must be light enough to read");
        // Mostly grey with a blue patch: blue still wins as the hue.
        let mut mixed = solid(120, 120, 120, 300);
        mixed.extend(solid(230, 60, 30, 100));
        let p = palette(&mixed);
        assert!(p.dominant.b > p.dominant.r);
    }

    #[test]
    fn grey_or_empty_covers_fall_back() {
        assert_eq!(
            palette(&solid(128, 128, 128, 400)).accent,
            Rgba::rgb(0xFF_FF_FF)
        );
        assert_eq!(palette(&[]), Palette::default());
    }

    #[test]
    fn soften_blurs_and_darkens() {
        // Left half black, right half white.
        let (w, h) = (16usize, 4usize);
        let mut img = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                let v = if x < w / 2 { 0 } else { 255 };
                img.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let out = soften(&img, w, h, 2, 0.0);
        assert_eq!(out.len(), img.len());
        let at = |x: usize| out[x * 4];
        assert!(at(0) < 20 && at(w - 1) > 235, "ends stay near their colour");
        assert!(at(w / 2 - 1) > 40 && at(w / 2) < 215, "the edge is smeared");
        let dark = soften(&img, w, h, 2, 0.5);
        assert!(dark[(w - 1) * 4] < 140);
        assert!(dark.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
        // Wrong buffer size: returned unchanged rather than panicking.
        assert_eq!(soften(&[1, 2, 3], 4, 4, 2, 0.5), vec![1, 2, 3]);
    }
}
