//! `SystemTime` as a JS `Date`: milliseconds since the epoch, and the ISO 8601 strings
//! `Date.prototype.toISOString` writes and `Date.parse` reads.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX: f64 = 8.64e15;
const DAY: i64 = 86_400_000;

/// The milliseconds since the epoch, rounded down.
pub fn millis(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => after.as_millis() as i64,
        Err(before) => {
            let before = before.duration();
            -(before.as_millis() as i64) - i64::from(before.subsec_nanos() % 1_000_000 != 0)
        }
    }
}

/// The time `ms` milliseconds after the epoch, as `new Date(ms)` clips it, or `None` outside the
/// range of a `Date`.
pub fn from_millis(ms: f64) -> Option<SystemTime> {
    if !ms.is_finite() || ms.abs() > MAX {
        return None;
    }
    let ms = ms.trunc();
    let span = Duration::from_millis(ms.abs() as u64);
    if ms < 0.0 { UNIX_EPOCH.checked_sub(span) } else { UNIX_EPOCH.checked_add(span) }
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

/// The time as `toISOString` writes it: `2024-05-01T12:00:00.000Z`.
pub fn iso(time: SystemTime) -> String {
    let ms = millis(time);
    let (year, month, day) = civil_from_days(ms.div_euclid(DAY));
    let rest = ms.rem_euclid(DAY);
    let year = match year {
        0..=9999 => format!("{year:04}"),
        _ if year < 0 => format!("-{:06}", -year),
        _ => format!("+{year:06}"),
    };
    format!(
        "{year}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3_600_000,
        rest / 60_000 % 60,
        rest / 1000 % 60,
        rest % 1000
    )
}

struct Cursor<'s>(&'s [u8]);

impl Cursor<'_> {
    fn eat(&mut self, byte: u8) -> bool {
        let found = self.0.first() == Some(&byte);
        if found {
            self.0 = &self.0[1..];
        }
        found
    }

    fn digits(&mut self, count: usize) -> Option<i64> {
        let digits = self.0.get(..count).filter(|d| d.iter().all(u8::is_ascii_digit))?;
        self.0 = &self.0[count..];
        Some(digits.iter().fold(0, |n, d| n * 10 + i64::from(d - b'0')))
    }
}

/// An ISO 8601 date or date-time string in UTC, or with an offset: `2024-05-01`,
/// `2024-05-01T12:00`, `2024-05-01T12:00:00.000+02:00`.
pub fn parse(text: &str) -> Option<SystemTime> {
    let mut at = Cursor(text.as_bytes());
    let year = if at.eat(b'+') {
        at.digits(6)?
    } else if at.eat(b'-') {
        -at.digits(6).filter(|&year| year != 0)?
    } else {
        at.digits(4)?
    };
    let month = if at.eat(b'-') { at.digits(2)? } else { 1 };
    let day = if at.eat(b'-') { at.digits(2)? } else { 1 };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let mut ms = 0;
    let mut offset = 0;
    if at.eat(b'T') {
        let hour = at.digits(2)?;
        at.eat(b':').then_some(())?;
        let minute = at.digits(2)?;
        let (mut second, mut fraction) = (0, 0);
        if at.eat(b':') {
            second = at.digits(2)?;
            if at.eat(b'.') {
                let count = at.0.iter().take_while(|d| d.is_ascii_digit()).count();
                if count == 0 {
                    return None;
                }
                fraction = at.digits(count.min(3))? * 10_i64.pow(3 - count.min(3) as u32);
                at.digits(count - count.min(3))?;
            }
        }
        if hour > 24 || minute > 59 || second > 59 || hour == 24 && minute + second + fraction > 0 {
            return None;
        }
        ms = ((hour * 60 + minute) * 60 + second) * 1000 + fraction;
        if !at.eat(b'Z')
            && let Some(&sign @ (b'+' | b'-')) = at.0.first()
        {
            at.0 = &at.0[1..];
            let hours = at.digits(2)?;
            at.eat(b':').then_some(())?;
            let minutes = at.digits(2)?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            offset = (hours * 60 + minutes) * 60_000 * if sign == b'+' { 1 } else { -1 };
        }
    }
    if !at.0.is_empty() {
        return None;
    }
    from_millis((days_from_civil(year, month, day) * DAY + ms - offset) as f64)
}
