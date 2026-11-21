//! Plain calendar arithmetic on local wall-clock time, without time zones: dates, weekdays and
//! "minutes since 1970-01-01 00:00 local" as the one number reminders are stored and compared in.

use crate::face::LocalTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: i32,
    /// 1..=12
    pub month: u32,
    /// 1..=31
    pub day: u32,
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
pub fn days_from_civil(d: Date) -> i64 {
    let y = i64::from(d.year) - i64::from(d.month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(d.month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d.day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn civil_from_days(z: i64) -> Date {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
    Date { year, month, day }
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        _ => 28,
    }
}

impl Date {
    pub fn valid(self) -> bool {
        (1..=12).contains(&self.month)
            && self.day >= 1
            && self.day <= days_in_month(self.year, self.month)
    }

    /// 0 = Sunday .. 6 = Saturday.
    pub fn weekday(self) -> u32 {
        // 1970-01-01 was a Thursday (4).
        (days_from_civil(self) + 4).rem_euclid(7) as u32
    }

    pub fn add_days(self, n: i64) -> Date {
        civil_from_days(days_from_civil(self) + n)
    }

    pub fn day_of_year(self) -> u32 {
        (days_from_civil(self)
            - days_from_civil(Date {
                year: self.year,
                month: 1,
                day: 1,
            })
            + 1) as u32
    }
}

/// `YYYY-MM-DD`.
pub fn parse_date(text: &str) -> Option<Date> {
    let mut parts = text.trim().splitn(3, '-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    let d = Date { year, month, day };
    d.valid().then_some(d)
}

/// Minutes since 1970-01-01 00:00, local wall-clock time.
pub type Minutes = i64;

pub fn minutes(date: Date, hour: u32, minute: u32) -> Minutes {
    days_from_civil(date) * 1440 + i64::from(hour) * 60 + i64::from(minute)
}

/// Split minutes back into a date, hour and minute.
pub fn split(m: Minutes) -> (Date, u32, u32) {
    let day = m.div_euclid(1440);
    let rest = m.rem_euclid(1440);
    (civil_from_days(day), (rest / 60) as u32, (rest % 60) as u32)
}

pub fn date_of(t: LocalTime) -> Date {
    Date {
        year: i32::from(t.year),
        month: u32::from(t.month),
        day: u32::from(t.day),
    }
}

pub fn now_minutes(t: LocalTime) -> Minutes {
    minutes(date_of(t), u32::from(t.hour), u32::from(t.minute))
}

pub const WEEKDAY_NAMES: [&str; 7] = [
    "sunday",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn d(year: i32, month: u32, day: u32) -> Date {
        Date { year, month, day }
    }

    #[test]
    fn epoch_and_round_trips() {
        assert_eq!(days_from_civil(d(1970, 1, 1)), 0);
        assert_eq!(civil_from_days(0), d(1970, 1, 1));
        for days in [-800_000, -1, 0, 1, 365, 19_000, 20_408, 2_932_896] {
            assert_eq!(days_from_civil(civil_from_days(days)), days);
        }
        assert_eq!(
            days_from_civil(d(2000, 3, 1)) - days_from_civil(d(2000, 2, 28)),
            2
        );
    }

    #[test]
    fn weekdays() {
        assert_eq!(d(1970, 1, 1).weekday(), 4);
        assert_eq!(d(2026, 11, 16).weekday(), 1, "16 Nov 2026 is a Monday");
        assert_eq!(d(2024, 2, 29).weekday(), 4);
    }

    #[test]
    fn month_lengths_and_validity() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2100, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert!(!d(2026, 2, 29).valid());
        assert!(!d(2026, 13, 1).valid());
        assert!(d(2026, 12, 31).valid());
    }

    #[test]
    fn parse_dates() {
        assert_eq!(parse_date("2026-12-24"), Some(d(2026, 12, 24)));
        assert_eq!(parse_date(" 2026-1-5 "), Some(d(2026, 1, 5)));
        assert_eq!(parse_date("2026-02-30"), None);
        assert_eq!(parse_date("24.12.2026"), None);
        assert_eq!(parse_date(""), None);
    }

    #[test]
    fn minutes_split_back() {
        let m = minutes(d(2026, 11, 16), 18, 30);
        assert_eq!(split(m), (d(2026, 11, 16), 18, 30));
        assert_eq!(split(m + 6 * 60), (d(2026, 11, 17), 0, 30));
        assert_eq!(d(2026, 12, 31).day_of_year(), 365);
        assert_eq!(d(2026, 11, 30).add_days(1), d(2026, 12, 1));
    }
}
