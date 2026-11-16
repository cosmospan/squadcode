//! Schedules beyond "every N minutes": a time of day on chosen weekdays, or a
//! cron expression. A time of day is stored as the cron expression it stands for,
//! so one parser answers when anything runs next.
//!
//! A cron expression has five fields, in local time: minute (0-59), hour (0-23),
//! day of the month (1-31), month (1-12 or jan-dec) and day of the week (0-7 or
//! sun-sat, where 0 and 7 are both Sunday). Each field is `*`, a number, a range
//! (`1-5`), a list (`1,3,5`) or any of those with a step (`*/15`, `9-17/2`). As in
//! cron, when both the day of the month and the day of the week are set, either
//! one matching is enough.

use chrono::{DateTime, Datelike, Duration, TimeZone, Timelike};

/// A parsed cron expression: which minutes, hours, days, months and weekdays match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cron {
    minutes: u64,
    hours: u32,
    days: u32,
    months: u16,
    weekdays: u8,
    /// The day of the month was `*` (so only the weekday counts), and the weekday
    /// was `*` (so only the day of the month counts).
    any_day: bool,
    any_weekday: bool,
}

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];
const WEEKDAYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

/// One field's matching values as bits, from `lo` to `hi`.
fn field(text: &str, lo: u32, hi: u32, names: &[&str], what: &str) -> Result<u64, String> {
    let value = |w: &str| -> Result<u32, String> {
        let low = w.to_ascii_lowercase();
        if let Some(i) = names
            .iter()
            .position(|n| low.starts_with(n) && low.len() >= 3)
        {
            // Names count from the field's lowest value: jan is 1, sun is 0.
            return Ok(i as u32 + lo);
        }
        let n: u32 = w
            .parse()
            .map_err(|_| format!("\u{201c}{w}\u{201d} isn't a {what}."))?;
        if n < lo || n > hi {
            return Err(format!("{what} {n} is outside {lo}-{hi}."));
        }
        Ok(n)
    };
    let mut bits = 0u64;
    for part in text.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => {
                let s: u32 = s
                    .parse()
                    .ok()
                    .filter(|s| *s > 0)
                    .ok_or_else(|| format!("\u{201c}/{s}\u{201d} isn't a step."))?;
                (r, s)
            }
            None => (part, 1),
        };
        let (a, b) = if range == "*" {
            (lo, hi)
        } else if let Some((a, b)) = range.split_once('-') {
            (value(a)?, value(b)?)
        } else {
            let a = value(range)?;
            // "5/15" means from 5 to the end, every 15.
            (a, if step > 1 { hi } else { a })
        };
        if a > b {
            return Err(format!("{what}s {a}-{b} run backwards."));
        }
        let mut n = a;
        while n <= b {
            bits |= 1 << n;
            n += step;
        }
    }
    Ok(bits)
}

impl Cron {
    /// Reads a five-field cron expression, or one of `@hourly`, `@daily`,
    /// `@weekly`, `@monthly` and `@yearly`.
    pub fn parse(expr: &str) -> Result<Cron, String> {
        let expr = match expr.trim() {
            "@hourly" => "0 * * * *",
            "@daily" | "@midnight" => "0 0 * * *",
            "@weekly" => "0 0 * * 0",
            "@monthly" => "0 0 1 * *",
            "@yearly" | "@annually" => "0 0 1 1 *",
            e => e,
        };
        let f: Vec<&str> = expr.split_whitespace().collect();
        if f.len() != 5 {
            return Err(
                "A schedule needs five fields: minute, hour, day, month and weekday.".into(),
            );
        }
        let weekdays = field(f[4], 0, 7, &WEEKDAYS, "weekday")?;
        // 7 is Sunday too.
        let weekdays = (weekdays | (weekdays >> 7)) & 0x7F;
        Ok(Cron {
            minutes: field(f[0], 0, 59, &[], "minute")?,
            hours: field(f[1], 0, 23, &[], "hour")? as u32,
            days: field(f[2], 1, 31, &[], "day")? as u32,
            months: field(f[3], 1, 12, &MONTHS, "month")? as u16,
            weekdays: weekdays as u8,
            any_day: f[2].starts_with('*'),
            any_weekday: f[4].starts_with('*'),
        })
    }

    fn day_matches<Tz: TimeZone>(&self, t: &DateTime<Tz>) -> bool {
        let dom = self.days & (1 << t.day()) != 0;
        let dow = self.weekdays & (1 << t.weekday().num_days_from_sunday()) != 0;
        match (self.any_day, self.any_weekday) {
            (true, true) => true,
            (true, false) => dow,
            (false, true) => dom,
            (false, false) => dom || dow,
        }
    }

    /// The first matching minute after `after` (never `after` itself), within
    /// the next five years; None for a schedule that never comes, like 30 February.
    pub fn next_after<Tz: TimeZone>(&self, after: &DateTime<Tz>) -> Option<DateTime<Tz>> {
        let tz = after.timezone();
        // Start at the next whole minute, in local wall-clock terms.
        let start = after.naive_local().with_second(0)?.with_nanosecond(0)? + Duration::minutes(1);
        let mut day = start.date();
        let end = day + Duration::days(5 * 366);
        let mut first = true;
        while day <= end {
            let probe = tz
                .from_local_datetime(&day.and_hms_opt(12, 0, 0)?)
                .earliest()?;
            if self.months & (1 << day.month()) != 0 && self.day_matches(&probe) {
                let from = if first {
                    (start.hour(), start.minute())
                } else {
                    (0, 0)
                };
                for h in from.0..24 {
                    if self.hours & (1 << h) == 0 {
                        continue;
                    }
                    let m0 = if h == from.0 { from.1 } else { 0 };
                    for m in m0..60 {
                        if self.minutes & (1 << m) == 0 {
                            continue;
                        }
                        // A time skipped by a clock change doesn't exist; the next one does.
                        if let Some(t) = tz
                            .from_local_datetime(&day.and_hms_opt(h, m, 0)?)
                            .earliest()
                        {
                            return Some(t);
                        }
                    }
                }
            }
            day = day.succ_opt()?;
            first = false;
        }
        None
    }
}

/// The cron expression for a time of day on chosen weekdays (0 is Sunday); no
/// weekdays means every day.
pub fn at_time(hour: u32, minute: u32, weekdays: &[u32]) -> String {
    let days = if weekdays.is_empty() || weekdays.len() >= 7 {
        "*".to_string()
    } else if weekdays == [1, 2, 3, 4, 5] {
        "1-5".to_string()
    } else {
        weekdays
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    };
    format!("{minute} {hour} * * {days}")
}

/// "9am", "9:30", "18:00", "6.30pm" as hours and minutes.
fn clock(word: &str) -> Option<(u32, u32)> {
    let w = word.trim().to_ascii_lowercase().replace('.', ":");
    let (t, pm) = if let Some(t) = w.strip_suffix("pm") {
        (t.trim().to_string(), Some(true))
    } else if let Some(t) = w.strip_suffix("am") {
        (t.trim().to_string(), Some(false))
    } else {
        (w, None)
    };
    let (h, m) = match t.split_once(':') {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None => (t.parse::<u32>().ok()?, 0),
    };
    let h = match pm {
        Some(true) if h < 12 => h + 12,
        Some(false) if h == 12 => 0,
        Some(_) if h > 12 => return None,
        _ => h,
    };
    (h < 24 && m < 60).then_some((h, m))
}

/// A schedule as people say it, as a cron expression: "weekdays at 9am",
/// "every day at 18:30", "mondays at 9", "hourly", "every 30 minutes",
/// "every 2 hours", or a cron expression as it is.
pub fn from_words(text: &str) -> Result<String, String> {
    let t = text.trim().to_ascii_lowercase();
    if t.is_empty() {
        return Err("Say when it runs, like \u{201c}weekdays at 9am\u{201d}.".into());
    }
    let cronish = t.starts_with(|c: char| c.is_ascii_digit() || c == '*');
    if t.starts_with('@') || (cronish && t.split_whitespace().count() == 5) {
        Cron::parse(&t)?;
        return Ok(t);
    }
    let t = t
        .trim_start_matches("every ")
        .trim_start_matches("each ")
        .to_string();
    if t == "hour" || t == "hourly" {
        return Ok("0 * * * *".into());
    }
    let words: Vec<&str> = t.split_whitespace().collect();
    if let (Some(n), Some(unit)) = (
        words.first().and_then(|n| n.parse::<u32>().ok()),
        words.get(1),
    ) {
        if words.len() == 2 {
            return match *unit {
                "minutes" | "minute" | "min" | "mins" if (1..60).contains(&n) => {
                    Ok(format!("*/{n} * * * *"))
                }
                "hours" | "hour" if (1..24).contains(&n) => Ok(format!("0 */{n} * * *")),
                _ => Err(format!(
                    "\u{201c}{text}\u{201d} isn't a schedule Squad Code knows."
                )),
            };
        }
    }
    // "<days> at <time>", or "<days>" alone (9 AM).
    let (days, time) = match t.split_once(" at ") {
        Some((d, c)) => (d.trim().to_string(), c.trim().to_string()),
        None if t.starts_with("at ") => ("day".to_string(), t[3..].trim().to_string()),
        None => (t.clone(), "9am".to_string()),
    };
    let (h, m) =
        clock(&time).ok_or_else(|| format!("\u{201c}{time}\u{201d} isn't a time of day."))?;
    let mut weekdays = Vec::new();
    for d in days
        .split([',', ' '])
        .filter(|d| !d.is_empty() && *d != "and")
    {
        match d {
            "day" | "days" | "daily" => weekdays.extend(0..7),
            "weekday" | "weekdays" => weekdays.extend(1..6),
            "weekend" | "weekends" => weekdays.extend([0, 6]),
            d => {
                let i = WEEKDAYS
                    .iter()
                    .position(|w| d.starts_with(w))
                    .ok_or_else(|| format!("\u{201c}{d}\u{201d} isn't a day."))?;
                weekdays.push(i as u32);
            }
        }
    }
    weekdays.sort_unstable();
    weekdays.dedup();
    Ok(at_time(h, m, &weekdays))
}

/// A schedule in words: "Weekdays at 9:00", "Every day at 18:30",
/// "Every 15 minutes", or the expression itself when it says more than that.
pub fn describe(expr: &str) -> String {
    let f: Vec<&str> = expr.split_whitespace().collect();
    if Cron::parse(expr).is_err() {
        return expr.to_string();
    }
    if f.len() != 5 {
        return match expr.trim() {
            "@hourly" => "Every hour".into(),
            "@daily" | "@midnight" => "Every day at 0:00".into(),
            "@weekly" => "Sundays at 0:00".into(),
            "@monthly" => "The 1st of each month at 0:00".into(),
            _ => expr.to_string(),
        };
    }
    let num = |s: &str| s.parse::<u32>().ok();
    if f[2] != "*" || f[3] != "*" {
        return format!("Cron {expr}");
    }
    if let Some(n) = f[0].strip_prefix("*/").and_then(num) {
        if f[1] == "*" && f[4] == "*" {
            return format!("Every {n} minutes");
        }
    }
    let Some(m) = num(f[0]) else {
        return format!("Cron {expr}");
    };
    if f[1] == "*" && f[4] == "*" {
        return if m == 0 {
            "Every hour".into()
        } else {
            format!("Every hour at :{m:02}")
        };
    }
    if let Some(n) = f[1].strip_prefix("*/").and_then(num) {
        if m == 0 && f[4] == "*" {
            return format!("Every {n} hours");
        }
    }
    let Some(h) = num(f[1]) else {
        return format!("Cron {expr}");
    };
    let days = match f[4] {
        "*" => "Every day".to_string(),
        "1-5" => "Weekdays".to_string(),
        "0,6" | "6,0" => "Weekends".to_string(),
        list => {
            let names: Option<Vec<&str>> = list
                .split(',')
                .map(|d| {
                    num(d).and_then(|d| {
                        [
                            "Sundays",
                            "Mondays",
                            "Tuesdays",
                            "Wednesdays",
                            "Thursdays",
                            "Fridays",
                            "Saturdays",
                            "Sundays",
                        ]
                        .get(d as usize)
                        .copied()
                    })
                })
                .collect();
            match names {
                Some(n) if !n.is_empty() => n.join(", "),
                _ => return format!("Cron {expr}"),
            }
        }
    };
    format!("{days} at {h}:{m:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, TimeZone, Utc};

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    #[test]
    fn weekdays_at_nine_skip_the_weekend() {
        let c = Cron::parse("0 9 * * 1-5").unwrap();
        // Friday 2026-11-13 at 10:00: next is Monday the 16th at 9:00.
        assert_eq!(
            c.next_after(&at(2026, 11, 13, 10, 0)),
            Some(at(2026, 11, 16, 9, 0))
        );
        // Monday at 8:59: the same morning.
        assert_eq!(
            c.next_after(&at(2026, 11, 16, 8, 59)),
            Some(at(2026, 11, 16, 9, 0))
        );
        // Exactly 9:00 is now, so the next is tomorrow.
        assert_eq!(
            c.next_after(&at(2026, 11, 16, 9, 0)),
            Some(at(2026, 11, 17, 9, 0))
        );
    }

    #[test]
    fn steps_lists_names_and_sunday_as_seven() {
        let c = Cron::parse("*/15 * * * *").unwrap();
        assert_eq!(
            c.next_after(&at(2026, 1, 1, 10, 7)),
            Some(at(2026, 1, 1, 10, 15))
        );
        let c = Cron::parse("30 18 * * sun").unwrap();
        assert_eq!(c, Cron::parse("30 18 * * 7").unwrap());
        // 2026-11-16 is a Monday; the next Sunday is the 22nd.
        assert_eq!(
            c.next_after(&at(2026, 11, 16, 0, 0)),
            Some(at(2026, 11, 22, 18, 30))
        );
        let c = Cron::parse("0 0 1 jan,jul *").unwrap();
        assert_eq!(
            c.next_after(&at(2026, 2, 1, 0, 0)),
            Some(at(2026, 7, 1, 0, 0))
        );
        // A day of the month and a weekday: either one.
        let c = Cron::parse("0 12 13 * 5").unwrap();
        assert_eq!(
            c.next_after(&at(2026, 11, 1, 0, 0)),
            Some(at(2026, 11, 6, 12, 0))
        );
    }

    #[test]
    fn local_time_is_kept() {
        let ist = FixedOffset::east_opt(5 * 3600 + 1800).unwrap();
        let c = Cron::parse("0 9 * * *").unwrap();
        let now = ist.with_ymd_and_hms(2026, 11, 16, 17, 0, 0).unwrap();
        let next = c.next_after(&now).unwrap();
        assert_eq!(next, ist.with_ymd_and_hms(2026, 11, 17, 9, 0, 0).unwrap());
    }

    #[test]
    fn bad_schedules_say_why_and_impossible_ones_never_come() {
        assert!(Cron::parse("0 9 * *").is_err());
        assert!(Cron::parse("61 9 * * *").is_err());
        assert!(Cron::parse("0 9 * * funday").is_err());
        assert!(Cron::parse("0 9-5 * * *").is_err());
        assert!(Cron::parse("*/0 * * * *").is_err());
        let never = Cron::parse("0 0 30 2 *").unwrap();
        assert_eq!(never.next_after(&at(2026, 1, 1, 0, 0)), None);
    }

    #[test]
    fn schedules_from_words() {
        assert_eq!(from_words("weekdays at 9am").unwrap(), "0 9 * * 1-5");
        assert_eq!(from_words("Every day at 18:30").unwrap(), "30 18 * * *");
        assert_eq!(
            from_words("mondays and fridays at 7pm").unwrap(),
            "0 19 * * 1,5"
        );
        assert_eq!(from_words("every 30 minutes").unwrap(), "*/30 * * * *");
        assert_eq!(from_words("every 2 hours").unwrap(), "0 */2 * * *");
        assert_eq!(from_words("hourly").unwrap(), "0 * * * *");
        assert_eq!(from_words("0 3 * * 0").unwrap(), "0 3 * * 0");
        assert_eq!(from_words("at 6am").unwrap(), "0 6 * * *");
        assert!(from_words("whenever").is_err());
        assert!(from_words("0 25 * * *").is_err());
    }

    #[test]
    fn schedules_in_words() {
        assert_eq!(describe("0 9 * * 1-5"), "Weekdays at 9:00");
        assert_eq!(describe("30 18 * * *"), "Every day at 18:30");
        assert_eq!(describe("*/15 * * * *"), "Every 15 minutes");
        assert_eq!(describe("0 */2 * * *"), "Every 2 hours");
        assert_eq!(describe("0 19 * * 1,5"), "Mondays, Fridays at 19:00");
        assert_eq!(describe("0 0 1 * *"), "Cron 0 0 1 * *");
        assert_eq!(describe("@hourly"), "Every hour");
    }
}
