//! Fullscreen games: which fullscreen apps count as games, and the numbers on the stats strip.

use crate::settings::{GameBehavior, Settings};

/// Apps that often go fullscreen but are not games. With "every fullscreen app" off these
/// leave Pillbox as it is. Users can add their own in `games.ignore_apps`.
pub const NOT_GAMES: &[&str] = &[
    "chrome.exe",
    "msedge.exe",
    "firefox.exe",
    "brave.exe",
    "opera.exe",
    "opera_gx.exe",
    "vivaldi.exe",
    "arc.exe",
    "zen.exe",
    "vlc.exe",
    "mpv.exe",
    "mpc-hc64.exe",
    "mpc-be64.exe",
    "potplayermini64.exe",
    "wmplayer.exe",
    "microsoft.media.player.exe",
    "video.ui.exe",
    "photos.exe",
    "microsoft.photos.exe",
    "powerpnt.exe",
    "winword.exe",
    "excel.exe",
    "acrord32.exe",
    "acrobat.exe",
    "explorer.exe",
    "mstsc.exe",
    "msrdc.exe",
    "vmconnect.exe",
    "applicationframehost.exe",
    // Game stores and launchers: their helper processes (the Steam overlay and store pages
    // are "steamwebhelper") go fullscreen, but nobody plays them.
    "steam.exe",
    "steamwebhelper.exe",
    "epicgameslauncher.exe",
    "epicwebhelper.exe",
    "galaxyclient.exe",
    "battle.net.exe",
    "ubisoftconnect.exe",
    "upc.exe",
    "eadesktop.exe",
    "origin.exe",
    "gog galaxy.exe",
    // Everyday apps that can be maximised or fullscreen.
    "discord.exe",
    "spotify.exe",
    "obs64.exe",
    "teams.exe",
    "ms-teams.exe",
    "zoom.exe",
    "slack.exe",
    "code.exe",
    "claude.exe",
    "notepad.exe",
    "windowsterminal.exe",
    "cmd.exe",
    "powershell.exe",
];

/// A known non-game executable (browser, player, launcher, office app...), by file name in any
/// case. These never count as play time, whatever the "every fullscreen app" setting says.
pub fn is_non_game_exe(exe: &str) -> bool {
    let exe = exe.trim().to_lowercase();
    NOT_GAMES.contains(&exe.as_str())
}

/// What Pillbox should do for a fullscreen app in front. `exe` is the lower-case
/// executable name when known; `exclusive` means Direct3D exclusive fullscreen.
pub fn behavior_for(s: &Settings, exe: Option<&str>, exclusive: bool) -> Option<GameBehavior> {
    let g = &s.games;
    if !g.enabled {
        return None;
    }
    let exe = exe.map(str::to_lowercase);
    // The user's own list always wins.
    if exe
        .as_deref()
        .is_some_and(|e| g.ignore_apps.iter().any(|i| i == e))
    {
        return None;
    }
    let known_non_game = exe.as_deref().is_some_and(|e| NOT_GAMES.contains(&e));
    if !g.every_fullscreen_app && !exclusive && known_non_game {
        return None;
    }
    Some(g.behavior)
}

/// Exponential smoothing for readings sampled ten times a second, so the digits do not
/// flicker. The first sample is taken as is.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Smooth {
    value: Option<f64>,
}

impl Smooth {
    /// Weight of each new sample (0..1).
    const WEIGHT: f64 = 0.3;

    pub fn push(&mut self, sample: f64) -> f64 {
        let v = match self.value {
            Some(v) => v + (sample - v) * Self::WEIGHT,
            None => sample,
        };
        self.value = Some(v);
        v
    }

    pub fn get(&self) -> Option<f64> {
        self.value
    }

    pub fn reset(&mut self) {
        self.value = None;
    }
}

/// A network speed for the strip: "12 Mb/s", "4.2 Mb/s", "860 kb/s" (bits), or the same in
/// bytes ("1.4 MB/s", "860 KB/s").
pub fn format_rate(bytes_per_sec: f64, bits: bool) -> String {
    let v = bytes_per_sec.max(0.0) * if bits { 8.0 } else { 1.0 };
    let (kilo, mega) = if bits {
        ("kb/s", "Mb/s")
    } else {
        ("KB/s", "MB/s")
    };
    if v >= 10_000_000.0 {
        format!("{:.0} {mega}", v / 1_000_000.0)
    } else if v >= 1_000_000.0 {
        format!("{:.1} {mega}", v / 1_000_000.0)
    } else {
        format!("{:.0} {kilo}", v / 1_000.0)
    }
}

/// The adapter LUID (high part, low part) in a "GPU Engine" counter instance name such as
/// `pid_1234_luid_0x00000000_0x0000C6F9_phys_0_eng_0_engtype_3D`.
pub fn parse_engine_luid(instance: &str) -> Option<(i32, u32)> {
    let rest = &instance[instance.find("luid_0x")? + "luid_0x".len()..];
    let (high, rest) = rest.split_once('_')?;
    let low = rest.strip_prefix("0x")?;
    let low = &low[..low.find('_').unwrap_or(low.len())];
    let high = u32::from_str_radix(high, 16).ok()? as i32;
    let low = u32::from_str_radix(low, 16).ok()?;
    Some((high, low))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_instance_names_give_the_adapter() {
        assert_eq!(
            parse_engine_luid("pid_1234_luid_0x00000000_0x0000C6F9_phys_0_eng_0_engtype_3D"),
            Some((0, 0xC6F9))
        );
        assert_eq!(
            parse_engine_luid("pid_8_luid_0x00000001_0x0001A2B3_phys_0_eng_12_engtype_3D"),
            Some((1, 0x1A2B3))
        );
        assert_eq!(parse_engine_luid("_Total"), None);
        assert_eq!(parse_engine_luid("pid_1_luid_0xZZ_0x1_phys_0"), None);
    }
    #[test]
    fn browsers_are_not_games_unless_every_app_is_on() {
        let mut s = Settings::default();
        assert_eq!(behavior_for(&s, Some("chrome.exe"), false), None);
        assert_eq!(
            behavior_for(&s, Some("Game.exe"), false),
            Some(GameBehavior::Strip)
        );
        s.games.every_fullscreen_app = true;
        assert_eq!(
            behavior_for(&s, Some("chrome.exe"), false),
            Some(GameBehavior::Strip)
        );
    }

    #[test]
    fn exclusive_fullscreen_counts_but_ignore_list_wins() {
        let mut s = Settings::default();
        s.games.behavior = GameBehavior::Hide;
        assert_eq!(
            behavior_for(&s, Some("vlc.exe"), true),
            Some(GameBehavior::Hide)
        );
        assert_eq!(behavior_for(&s, None, true), Some(GameBehavior::Hide));
        s.games.ignore_apps = vec!["game.exe".into()];
        assert_eq!(behavior_for(&s, Some("GAME.EXE"), true), None);
        s.games.enabled = false;
        assert_eq!(behavior_for(&s, Some("other.exe"), true), None);
    }

    #[test]
    fn smoothing_starts_at_first_sample_and_moves_partway() {
        let mut m = Smooth::default();
        assert_eq!(m.push(50.0), 50.0);
        let v = m.push(100.0);
        assert!(v > 50.0 && v < 100.0, "{v}");
        m.reset();
        assert_eq!(m.get(), None);
    }

    #[test]
    fn rates_pick_sensible_units() {
        assert_eq!(format_rate(0.0, true), "0 kb/s");
        assert_eq!(format_rate(107_500.0, true), "860 kb/s");
        assert_eq!(format_rate(525_000.0, true), "4.2 Mb/s");
        assert_eq!(format_rate(1_500_000.0, true), "12 Mb/s");
        assert_eq!(format_rate(1_400_000.0, false), "1.4 MB/s");
        assert_eq!(format_rate(-5.0, false), "0 KB/s");
    }
}
