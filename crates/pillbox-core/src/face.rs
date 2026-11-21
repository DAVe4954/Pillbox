//! Text and data for the resting face and the placeholder card: clock, date, weather, pet.

use serde::Deserialize;

use crate::settings::PillClock;

/// Local wall-clock time, as read from the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LocalTime {
    pub year: u16,
    /// 1..=12
    pub month: u8,
    pub day: u8,
    /// 0 = Sunday
    pub weekday: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// "14:05" or "2:05 PM".
pub fn format_time(t: LocalTime, clock_24h: bool) -> String {
    if clock_24h {
        format!("{}:{:02}", t.hour, t.minute)
    } else {
        let h = match t.hour % 12 {
            0 => 12,
            h => h,
        };
        let suffix = if t.hour < 12 { "AM" } else { "PM" };
        format!("{h}:{:02} {suffix}", t.minute)
    }
}

/// "Mon 16 Nov".
pub fn format_date(t: LocalTime) -> String {
    let weekday = WEEKDAYS.get(usize::from(t.weekday)).copied().unwrap_or("");
    let month = MONTHS
        .get(usize::from(t.month).wrapping_sub(1))
        .copied()
        .unwrap_or("");
    format!("{weekday} {} {month}", t.day)
}

/// The time with the pill clock's options: "14:05", "09:05", "14:05:09" or "2:05:09 PM".
pub fn format_time_with(
    t: LocalTime,
    clock_24h: bool,
    seconds: bool,
    leading_zero: bool,
) -> String {
    let (hour, suffix) = if clock_24h {
        (u32::from(t.hour), "")
    } else {
        let h = match t.hour % 12 {
            0 => 12,
            h => u32::from(h),
        };
        (h, if t.hour < 12 { " AM" } else { " PM" })
    };
    let hour = if leading_zero {
        format!("{hour:02}")
    } else {
        hour.to_string()
    };
    if seconds {
        format!("{hour}:{:02}:{:02}{suffix}", t.minute, t.second)
    } else {
        format!("{hour}:{:02}{suffix}", t.minute)
    }
}

/// The clock face on the resting pill: "14:05", "Fri 14:05" or "20 Nov 14:05", with the
/// time written as `format_time_with` does.
pub fn format_pill_clock(
    t: LocalTime,
    clock_24h: bool,
    shows: PillClock,
    seconds: bool,
    leading_zero: bool,
) -> String {
    let time = format_time_with(t, clock_24h, seconds, leading_zero);
    match shows {
        PillClock::Time => time,
        PillClock::DayTime => {
            let weekday = WEEKDAYS.get(usize::from(t.weekday)).copied().unwrap_or("");
            format!("{weekday} {time}").trim_start().to_string()
        }
        PillClock::DateTime => {
            let month = MONTHS
                .get(usize::from(t.month).wrapping_sub(1))
                .copied()
                .unwrap_or("");
            format!("{} {month} {time}", t.day)
        }
    }
}

/// Fraction of the day that has passed, 0..1.
pub fn day_progress(t: LocalTime) -> f32 {
    let seconds = u32::from(t.hour) * 3600 + u32::from(t.minute) * 60 + u32::from(t.second);
    (seconds as f32 / 86_400.0).clamp(0.0, 1.0)
}

/// Milliseconds until the next minute starts (plus a little, so the clock has ticked over).
pub fn ms_until_next_minute(t: LocalTime) -> u32 {
    (60 - u32::from(t.second.min(59))) * 1000 + 50
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WeatherNow {
    pub temp_c: f32,
    /// WMO weather code.
    pub code: u32,
    pub is_day: bool,
    /// Extra readings for the detailed weather look; any of them may be missing.
    pub feels_c: Option<f32>,
    /// Relative humidity, percent.
    pub humidity: Option<f32>,
    pub wind_kmh: Option<f32>,
    /// Today's high and low.
    pub high_c: Option<f32>,
    pub low_c: Option<f32>,
}

/// The Open-Meteo request for current conditions and today's range. No API key, no account.
pub fn open_meteo_url(latitude: f32, longitude: f32) -> String {
    format!(
        "https://api.open-meteo.com/v1/forecast?latitude={latitude:.3}&longitude={longitude:.3}\
         &current=temperature_2m,weather_code,is_day,apparent_temperature,relative_humidity_2m,\
         wind_speed_10m&daily=temperature_2m_max,temperature_2m_min&forecast_days=1&timezone=auto"
    )
}

/// A place found by the city search in Settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    /// "Prague, Prague, Czechia": name, region and country, without repeats or blanks.
    pub label: String,
    pub latitude: f32,
    pub longitude: f32,
}

/// Open-Meteo's place search (the same provider as the weather; no key, no account). Only the
/// typed name is sent, percent-encoded.
pub fn geocode_url(name: &str) -> String {
    let mut encoded = String::new();
    for b in name.trim().bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(b));
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={encoded}&count=6&language=en&format=json"
    )
}

/// The places in a geocoding reply; none for anything unexpected.
pub fn parse_geocode(json: &str) -> Vec<Place> {
    #[derive(Deserialize)]
    struct Reply {
        #[serde(default)]
        results: Vec<Found>,
    }
    #[derive(Deserialize)]
    struct Found {
        name: String,
        latitude: f32,
        longitude: f32,
        #[serde(default)]
        admin1: Option<String>,
        #[serde(default)]
        country: Option<String>,
    }
    let Ok(reply) = serde_json::from_str::<Reply>(json) else {
        return Vec::new();
    };
    reply
        .results
        .into_iter()
        .filter(|f| {
            f.latitude.is_finite()
                && f.longitude.is_finite()
                && (-90.0..=90.0).contains(&f.latitude)
                && (-180.0..=180.0).contains(&f.longitude)
        })
        .map(|f| {
            let mut parts: Vec<String> = vec![f.name.trim().to_string()];
            for extra in [f.admin1, f.country].into_iter().flatten() {
                let extra = extra.trim().to_string();
                if !extra.is_empty() && !parts.contains(&extra) {
                    parts.push(extra);
                }
            }
            Place {
                label: parts.join(", "),
                latitude: f.latitude,
                longitude: f.longitude,
            }
        })
        .collect()
}

pub fn parse_open_meteo(json: &str) -> Option<WeatherNow> {
    #[derive(Deserialize)]
    struct Current {
        temperature_2m: f32,
        weather_code: u32,
        is_day: u8,
        apparent_temperature: Option<f32>,
        relative_humidity_2m: Option<f32>,
        wind_speed_10m: Option<f32>,
    }
    #[derive(Deserialize, Default)]
    struct Daily {
        #[serde(default)]
        temperature_2m_max: Vec<Option<f32>>,
        #[serde(default)]
        temperature_2m_min: Vec<Option<f32>>,
    }
    #[derive(Deserialize)]
    struct Response {
        current: Current,
        #[serde(default)]
        daily: Daily,
    }
    let r: Response = serde_json::from_str(json).ok()?;
    let finite = |v: Option<f32>| v.filter(|x| x.is_finite());
    let first = |v: &[Option<f32>]| finite(v.first().copied().flatten());
    r.current.temperature_2m.is_finite().then_some(WeatherNow {
        temp_c: r.current.temperature_2m,
        code: r.current.weather_code,
        is_day: r.current.is_day != 0,
        feels_c: finite(r.current.apparent_temperature),
        humidity: finite(r.current.relative_humidity_2m),
        wind_kmh: finite(r.current.wind_speed_10m),
        high_c: first(&r.daily.temperature_2m_max),
        low_c: first(&r.daily.temperature_2m_min),
    })
}

/// A short name for a WMO weather code.
pub fn weather_name(code: u32) -> &'static str {
    match code {
        0 => "Clear",
        1 => "Mostly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51..=57 => "Drizzle",
        61..=67 => "Rain",
        71..=77 => "Snow",
        80..=82 => "Showers",
        85 | 86 => "Snow showers",
        95..=99 => "Thunderstorm",
        _ => "Cloudy",
    }
}

/// The lines of the detailed weather look: condition, today's range and "feels like", wind
/// and humidity. Lines whose readings are missing are left out.
pub fn weather_details(w: WeatherNow, fahrenheit: bool) -> Vec<String> {
    let deg = |c: f32| {
        let v = if fahrenheit { c * 9.0 / 5.0 + 32.0 } else { c };
        format!("{}°", v.round() as i32)
    };
    let mut lines = vec![weather_name(w.code).to_string()];
    let mut range = Vec::new();
    if let (Some(h), Some(l)) = (w.high_c, w.low_c) {
        range.push(format!("H {} · L {}", deg(h), deg(l)));
    }
    if let Some(f) = w.feels_c {
        range.push(format!("feels {}", deg(f)));
    }
    if !range.is_empty() {
        lines.push(range.join(" · "));
    }
    let mut air = Vec::new();
    if let Some(kmh) = w.wind_kmh {
        air.push(if fahrenheit {
            format!("Wind {:.0} mph", kmh / 1.609)
        } else {
            format!("Wind {kmh:.0} km/h")
        });
    }
    if let Some(h) = w.humidity {
        air.push(format!("Humidity {h:.0}%"));
    }
    if !air.is_empty() {
        lines.push(air.join(" · "));
    }
    lines
}
/// A single-glyph icon for a WMO weather code. Clear nights show the moon.
pub fn weather_glyph(code: u32, is_day: bool) -> &'static str {
    match code {
        0 | 1 => {
            if is_day {
                "☀"
            } else {
                "🌙"
            }
        }
        2 => "⛅",
        3 => "☁",
        45 | 48 => "🌫",
        51..=67 | 80..=82 => "🌧",
        71..=77 | 85 | 86 => "❄",
        95..=99 => "⛈",
        _ => "☁",
    }
}

/// "☀ 21°"
pub fn format_weather(w: WeatherNow, fahrenheit: bool) -> String {
    let temp = if fahrenheit {
        w.temp_c * 9.0 / 5.0 + 32.0
    } else {
        w.temp_c
    };
    // Avoid "-0°".
    let rounded = temp.round() as i32;
    format!("{} {rounded}°", weather_glyph(w.code, w.is_day))
}

/// What a pixel of the pet sprite is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PetPixel {
    Body,
    Eye,
    Blush,
}

/// A small cat face. `#` body, `o` eye, `*` cheek.
const PET: [&str; 9] = [
    "#.........#",
    "##.......##",
    "###########",
    "###########",
    "##o#####o##",
    "###########",
    "#*#######*#",
    "###########",
    ".#########.",
];

pub const PET_WIDTH: usize = 11;
pub const PET_HEIGHT: usize = PET.len();

/// The sprite's pixels as (column, row, kind). While blinking the eyes are closed (body colour).
pub fn pet_pixels(blink: bool) -> impl Iterator<Item = (usize, usize, PetPixel)> {
    PET.iter().enumerate().flat_map(move |(y, row)| {
        row.bytes().enumerate().filter_map(move |(x, c)| {
            let kind = match c {
                b'#' => PetPixel::Body,
                b'o' if blink => PetPixel::Body,
                b'o' => PetPixel::Eye,
                b'*' => PetPixel::Blush,
                _ => return None,
            };
            Some((x, y, kind))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(hour: u8, minute: u8, second: u8) -> LocalTime {
        LocalTime {
            year: 2026,
            month: 11,
            day: 16,
            weekday: 1,
            hour,
            minute,
            second,
        }
    }

    #[test]
    fn time_24h() {
        assert_eq!(format_time(at(0, 5, 0), true), "0:05");
        assert_eq!(format_time(at(14, 30, 0), true), "14:30");
    }

    #[test]
    fn time_12h_handles_midnight_and_noon() {
        assert_eq!(format_time(at(0, 5, 0), false), "12:05 AM");
        assert_eq!(format_time(at(12, 0, 0), false), "12:00 PM");
        assert_eq!(format_time(at(13, 9, 0), false), "1:09 PM");
        assert_eq!(format_time(at(23, 59, 0), false), "11:59 PM");
    }

    #[test]
    fn the_pill_clock_adds_the_day_or_date() {
        let t = at(14, 5, 0);
        let plain = |t, h24, shows| format_pill_clock(t, h24, shows, false, false);
        assert_eq!(plain(t, true, PillClock::Time), "14:05");
        assert_eq!(plain(t, true, PillClock::DayTime), "Mon 14:05");
        assert_eq!(plain(t, false, PillClock::DayTime), "Mon 2:05 PM");
        assert_eq!(plain(t, true, PillClock::DateTime), "16 Nov 14:05");
        // A weekday the OS should never give leaves just the time, not a stray space.
        let odd = LocalTime { weekday: 9, ..t };
        assert_eq!(plain(odd, true, PillClock::DayTime), "14:05");
    }

    #[test]
    fn the_pill_clock_can_show_seconds_and_a_leading_zero() {
        let t = at(9, 5, 7);
        assert_eq!(format_time_with(t, true, false, false), "9:05");
        assert_eq!(format_time_with(t, true, false, true), "09:05");
        assert_eq!(format_time_with(t, true, true, true), "09:05:07");
        assert_eq!(format_time_with(t, false, true, false), "9:05:07 AM");
        assert_eq!(
            format_time_with(at(0, 0, 0), false, true, true),
            "12:00:00 AM"
        );
        assert_eq!(
            format_time_with(at(13, 9, 59), false, false, true),
            "01:09 PM"
        );
        // Without the options it is the same text as `format_time`.
        for (h, m) in [(0, 0), (9, 5), (12, 30), (23, 59)] {
            for h24 in [true, false] {
                assert_eq!(
                    format_time_with(at(h, m, 3), h24, false, false),
                    format_time(at(h, m, 3), h24)
                );
            }
        }
        assert_eq!(
            format_pill_clock(t, true, PillClock::DayTime, true, true),
            "Mon 09:05:07"
        );
    }

    #[test]
    fn date_is_short_and_survives_bad_input() {
        assert_eq!(format_date(at(1, 1, 1)), "Mon 16 Nov");
        let odd = LocalTime {
            month: 0,
            weekday: 9,
            ..at(1, 1, 1)
        };
        assert_eq!(format_date(odd), " 16 ");
    }

    #[test]
    fn day_progress_endpoints() {
        assert_eq!(day_progress(at(0, 0, 0)), 0.0);
        assert!((day_progress(at(12, 0, 0)) - 0.5).abs() < 1e-6);
        assert!(day_progress(at(23, 59, 59)) > 0.999);
    }

    #[test]
    fn next_minute_delay() {
        assert_eq!(ms_until_next_minute(at(1, 1, 0)), 60_050);
        assert_eq!(ms_until_next_minute(at(1, 1, 59)), 1_050);
        // A leap second (60) must not underflow.
        assert_eq!(ms_until_next_minute(at(1, 1, 60)), 1_050);
    }

    #[test]
    fn open_meteo_response_parses() {
        let json = r#"{"latitude":51.5,"longitude":-0.1,"current_units":{"temperature_2m":"°C"},
            "current":{"time":"2026-11-16T10:00","interval":900,"temperature_2m":8.4,"weather_code":61,"is_day":1}}"#;
        let w = parse_open_meteo(json);
        assert_eq!(
            w,
            Some(WeatherNow {
                temp_c: 8.4,
                code: 61,
                is_day: true,
                ..Default::default()
            })
        );
        let full = r#"{"current":{"temperature_2m":8.4,"weather_code":2,"is_day":0,
            "apparent_temperature":5.6,"relative_humidity_2m":81,"wind_speed_10m":14.2},
            "daily":{"time":["2026-11-16"],"temperature_2m_max":[11.0],"temperature_2m_min":[3.6]}}"#;
        let w = parse_open_meteo(full);
        assert_eq!(
            w,
            Some(WeatherNow {
                temp_c: 8.4,
                code: 2,
                is_day: false,
                feels_c: Some(5.6),
                humidity: Some(81.0),
                wind_kmh: Some(14.2),
                high_c: Some(11.0),
                low_c: Some(3.6),
            })
        );
        assert_eq!(parse_open_meteo("{}"), None);
        assert_eq!(parse_open_meteo("not json"), None);
        assert_eq!(parse_open_meteo(r#"{"error":true,"reason":"bad"}"#), None);
    }

    #[test]
    fn weather_text() {
        let clear_night = WeatherNow {
            temp_c: -0.2,
            code: 0,
            is_day: false,
            ..Default::default()
        };
        assert_eq!(format_weather(clear_night, false), "🌙 0°");
        assert_eq!(format_weather(clear_night, true), "🌙 32°");
        let rain = WeatherNow {
            temp_c: 21.4,
            code: 63,
            is_day: true,
            ..Default::default()
        };
        assert_eq!(format_weather(rain, false), "🌧 21°");
        assert_eq!(weather_glyph(0, true), "☀");
        assert_eq!(weather_glyph(999, true), "☁");
    }

    #[test]
    fn detailed_weather_lines() {
        let w = WeatherNow {
            temp_c: 8.0,
            code: 61,
            is_day: true,
            feels_c: Some(5.6),
            humidity: Some(81.0),
            wind_kmh: Some(16.1),
            high_c: Some(11.0),
            low_c: Some(3.6),
        };
        assert_eq!(
            weather_details(w, false),
            [
                "Rain",
                "H 11° · L 4° · feels 6°",
                "Wind 16 km/h · Humidity 81%"
            ]
        );
        assert_eq!(weather_details(w, true)[2], "Wind 10 mph · Humidity 81%");
        // Missing readings drop their line instead of showing blanks.
        let bare = WeatherNow {
            code: 3,
            ..Default::default()
        };
        assert_eq!(weather_details(bare, false), ["Overcast"]);
    }

    #[test]
    fn city_search_sends_only_the_name_and_reads_places() {
        assert_eq!(
            geocode_url("  São Paulo & co "),
            "https://geocoding-api.open-meteo.com/v1/search?name=S%C3%A3o%20Paulo%20%26%20co&count=6&language=en&format=json"
        );
        let json = r#"{"results":[
            {"name":"Prague","latitude":50.088,"longitude":14.42,"admin1":"Prague","country":"Czechia"},
            {"name":"Prague","latitude":35.48,"longitude":-96.68,"admin1":"Oklahoma","country":"United States"},
            {"name":"Nowhere","latitude":999,"longitude":0},
            {"name":"Bare","latitude":1,"longitude":2}
        ],"generationtime_ms":0.5}"#;
        let places = parse_geocode(json);
        assert_eq!(places.len(), 3, "the impossible latitude is dropped");
        assert_eq!(places[0].label, "Prague, Czechia");
        assert_eq!(places[1].label, "Prague, Oklahoma, United States");
        assert_eq!(places[2].label, "Bare");
        assert!((places[0].latitude - 50.088).abs() < 1e-4);
        // No match, or not JSON at all: nothing.
        assert!(parse_geocode(r#"{"generationtime_ms":0.2}"#).is_empty());
        assert!(parse_geocode("<html>").is_empty());
    }

    #[test]
    fn url_has_no_key_and_rounds_coordinates() {
        let url = open_meteo_url(51.50735, -0.12776);
        assert!(url.starts_with(
            "https://api.open-meteo.com/v1/forecast?latitude=51.507&longitude=-0.128"
        ));
        assert!(!url.contains("key"));
    }

    #[test]
    fn pet_sprite_is_rectangular_and_blinks() {
        assert!(PET.iter().all(|row| row.len() == PET_WIDTH));
        let eyes = |blink| pet_pixels(blink).filter(|p| p.2 == PetPixel::Eye).count();
        assert_eq!(eyes(false), 2);
        assert_eq!(eyes(true), 0);
        assert_eq!(pet_pixels(false).count(), pet_pixels(true).count());
        assert!(pet_pixels(false).all(|(x, y, _)| x < PET_WIDTH && y < PET_HEIGHT));
    }
}
