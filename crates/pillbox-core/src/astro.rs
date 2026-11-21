//! Sunrise, sunset and the moon's phase, computed locally (no network). NOAA's simplified
//! solar equations: accurate to a minute or two away from the poles.

use crate::calendar::{Date, days_from_civil};

/// Sunrise and sunset in minutes after local midnight, for a date, place and UTC offset.
/// `None` during polar day or polar night.
pub fn sun_times(date: Date, lat: f64, lon: f64, utc_offset_min: f64) -> Option<(f64, f64)> {
    let n = f64::from(date.day_of_year());
    let g = 2.0 * std::f64::consts::PI / 365.0 * (n - 1.0);
    let eqtime = 229.18
        * (0.000075 + 0.001868 * g.cos()
            - 0.032077 * g.sin()
            - 0.014615 * (2.0 * g).cos()
            - 0.040849 * (2.0 * g).sin());
    let decl = 0.006918 - 0.399912 * g.cos() + 0.070257 * g.sin() - 0.006758 * (2.0 * g).cos()
        + 0.000907 * (2.0 * g).sin()
        - 0.002697 * (3.0 * g).cos()
        + 0.00148 * (3.0 * g).sin();
    let lat_r = lat.to_radians();
    let cos_ha =
        (90.833f64.to_radians().cos() / (lat_r.cos() * decl.cos())) - lat_r.tan() * decl.tan();
    if !(-1.0..=1.0).contains(&cos_ha) {
        return None;
    }
    let ha = cos_ha.acos().to_degrees();
    let rise = 720.0 - 4.0 * (lon + ha) - eqtime + utc_offset_min;
    let set = 720.0 - 4.0 * (lon - ha) - eqtime + utc_offset_min;
    Some((rise.rem_euclid(1440.0), set.rem_euclid(1440.0)))
}

/// Moon phase as a fraction of the cycle: 0 = new, 0.25 = first quarter, 0.5 = full.
/// `minutes_utc` is minutes since 1970-01-01 00:00 UTC.
pub fn moon_phase(minutes_utc: i64) -> f64 {
    const SYNODIC: f64 = 29.530_588_853;
    // A known new moon: 2000-01-06 18:14 UTC.
    let reference = days_from_civil(Date {
        year: 2000,
        month: 1,
        day: 6,
    }) as f64
        + (18.0 * 60.0 + 14.0) / 1440.0;
    let days = minutes_utc as f64 / 1440.0 - reference;
    (days / SYNODIC).rem_euclid(1.0)
}

/// Fraction of the moon's face that is lit, 0..1.
pub fn moon_lit(phase: f64) -> f64 {
    (1.0 - (2.0 * std::f64::consts::PI * phase).cos()) / 2.0
}

pub fn moon_name(phase: f64) -> &'static str {
    match (phase * 8.0).round() as u32 % 8 {
        0 => "New moon",
        1 => "Waxing crescent",
        2 => "First quarter",
        3 => "Waxing gibbous",
        4 => "Full moon",
        5 => "Waning gibbous",
        6 => "Last quarter",
        _ => "Waning crescent",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::minutes;

    fn hm(m: f64) -> (u32, u32) {
        let m = m.round() as u32;
        (m / 60, m % 60)
    }

    #[test]
    fn london_midsummer() {
        let d = Date {
            year: 2026,
            month: 6,
            day: 21,
        };
        // British Summer Time, UTC+1: sunrise about 04:43, sunset about 21:21.
        let (rise, set) = match sun_times(d, 51.5, -0.12, 60.0) {
            Some(t) => t,
            None => panic!("London has a sunrise"),
        };
        let (rh, rm) = hm(rise);
        let (sh, sm) = hm(set);
        assert!((rh, rm) >= (4, 38) && (rh, rm) <= (4, 48), "rise {rh}:{rm}");
        assert!(
            (sh, sm) >= (21, 16) && (sh, sm) <= (21, 26),
            "set {sh}:{sm}"
        );
    }

    #[test]
    fn equator_is_about_twelve_hours() {
        let d = Date {
            year: 2026,
            month: 3,
            day: 20,
        };
        let (rise, set) = match sun_times(d, 0.0, 0.0, 0.0) {
            Some(t) => t,
            None => panic!(),
        };
        assert!((set - rise - 12.0 * 60.0).abs() < 15.0, "{rise} {set}");
    }

    #[test]
    fn polar_night_has_no_sunrise() {
        let d = Date {
            year: 2026,
            month: 12,
            day: 21,
        };
        assert!(
            sun_times(d, 78.2, 15.6, 60.0).is_none(),
            "Svalbard in December"
        );
    }

    #[test]
    fn moon_phases() {
        // New moon on 8 April 2024 at 18:21 UTC (total solar eclipse).
        let new = minutes(
            Date {
                year: 2024,
                month: 4,
                day: 8,
            },
            18,
            21,
        );
        let p = moon_phase(new);
        assert!(!(0.02..=0.98).contains(&p), "{p}");
        assert_eq!(moon_name(p), "New moon");
        // Full moon on 23 April 2024 at 23:49 UTC.
        let full = minutes(
            Date {
                year: 2024,
                month: 4,
                day: 23,
            },
            23,
            49,
        );
        let p = moon_phase(full);
        assert!((p - 0.5).abs() < 0.03, "{p}");
        assert!(moon_lit(p) > 0.98);
        assert_eq!(moon_name(p), "Full moon");
    }
}
