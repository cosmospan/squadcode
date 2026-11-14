//! The night shift: jobs that wait for a time you picked ("/later tonight
//! tidy the flaky tests"), jobs that wait for their bot to be free, and jobs
//! that ran out of their engine's plan and carry on by themselves once it
//! resets (or, on the night shift, on another engine that still has plan).
//!
//! Everything here is plain data and pure functions; `Runtime::poll_shift`
//! moves the waiting jobs along.

use chrono::{DateTime, Duration, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};

use super::data::{Member, Mode};
use super::store::{Folder, Item, Job};

/// Most times a job is picked up again after running out of plan.
pub const MAX_RESUMES: u8 = 3;

/// How long a job that ran out waits when its engine didn't say when the
/// plan resets.
pub const UNKNOWN_RESET: u64 = 30 * 60;

/// A job waiting to start.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Later {
    pub id: u64,
    pub bot: Member,
    pub mode: Mode,
    pub text: String,
    pub folder: Folder,
    /// The # thread it goes to; 0 is #general.
    pub thread: u64,
    /// When it may start (seconds since 1970); 0 means as soon as its bot is
    /// free.
    pub at: u64,
    /// When it was put here.
    pub made: u64,
    /// It names an issue that's being read; it starts once that's done.
    pub reading: bool,
    /// The issue it's for, once read: the line its pull request closes it
    /// with, and its address.
    pub issue: String,
    pub issue_url: String,
    /// Sent from another computer: the branch and commit it starts from.
    pub handoff: Option<super::work::Handoff>,
}

impl Default for Later {
    fn default() -> Self {
        Later {
            id: 0,
            bot: Member::Builder,
            mode: Mode::Build,
            text: String::new(),
            folder: Folder::default(),
            thread: 0,
            at: 0,
            made: 0,
            reading: false,
            issue: String::new(),
            issue_url: String::new(),
            handoff: None,
        }
    }
}

impl Later {
    /// It may start now, as far as time goes.
    pub fn due(&self, now: u64) -> bool {
        self.at <= now
    }

    /// The first line of what it's for.
    pub fn label(&self) -> &str {
        self.text.lines().next().unwrap_or("").trim()
    }
}

/// When the person wants a job to start, read from the front of what they
/// typed after `/later`, and the rest: the job itself.
///
/// Understands `tonight` and `overnight` (11 PM, or now when it's already
/// late), `tomorrow` (9 AM) and `tomorrow 7am`, a clock time (`2am`,
/// `at 14:30`, `at 9pm`), and a wait (`in 2h`, `in 30m`, `in 90 min`,
/// `in 1 hour`). None when it doesn't start with a time, or nothing is left
/// to do.
pub fn parse_when<Tz: TimeZone>(text: &str, now: DateTime<Tz>) -> Option<(u64, String)> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let low: Vec<String> = words
        .iter()
        .map(|w| w.to_lowercase().trim_end_matches([',', ':']).to_string())
        .collect();
    let first = low.first()?.as_str();
    let (at, used) = match first {
        "tonight" | "overnight" => {
            let eleven = at_clock(&now, 23, 0, 0)?;
            // Already late: start now; in the small hours "tonight" is now too.
            let late = now.clone().naive_local().time() >= NaiveTime::from_hms_opt(23, 0, 0)?
                || small_hours(&now);
            let at = if late {
                now.timestamp().max(0) as u64
            } else {
                eleven
            };
            (at, 1)
        }
        "tomorrow" => {
            // "tomorrow 7am" or "tomorrow at 7am"; plain "tomorrow" is 9 AM.
            // Still up in the small hours, tomorrow morning is this one.
            let days = if small_hours(&now) { 0 } else { 1 };
            let mut used = 1;
            let mut clock = (9, 0);
            let mut i = 1;
            if low.get(i).map(String::as_str) == Some("at") {
                i += 1;
            }
            if let Some(c) = low.get(i).and_then(|w| clock_of(w)) {
                clock = c;
                used = i + 1;
            }
            (at_clock(&now, clock.0, clock.1, days)?, used)
        }
        "in" => {
            let (secs, used) = wait_of(&low[1..])?;
            (now.timestamp().max(0) as u64 + secs, used + 1)
        }
        "at" => {
            let (h, m) = clock_of(low.get(1)?)?;
            (next_clock(&now, h, m)?, 2)
        }
        w => {
            // A bare clock time needs am/pm or a colon, so "2 bugs" isn't a time.
            if !(w.ends_with("am") || w.ends_with("pm") || w.contains(':')) {
                return None;
            }
            let (h, m) = clock_of(w)?;
            (next_clock(&now, h, m)?, 1)
        }
    };
    let rest = words.get(used..)?.join(" ");
    let rest = rest.trim().to_string();
    (!rest.is_empty()).then_some((at, rest))
}

/// Before 5 AM: still last night, as far as people go.
pub fn small_hours<Tz: TimeZone>(now: &DateTime<Tz>) -> bool {
    use chrono::Timelike;
    now.clone().naive_local().hour() < 5
}

/// "2am", "9pm", "14:30", "1:30am", "9" as hours and minutes.
fn clock_of(word: &str) -> Option<(u32, u32)> {
    let w = word.trim().to_lowercase();
    let w = w.trim_end_matches(['.', ',']);
    let (clock, pm) = match w.strip_suffix("pm") {
        Some(t) => (t.trim(), Some(true)),
        None => match w.strip_suffix("am") {
            Some(t) => (t.trim(), Some(false)),
            None => (w, None),
        },
    };
    let (h, m) = match clock.split_once(':') {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None => (clock.parse::<u32>().ok()?, 0),
    };
    if m > 59 {
        return None;
    }
    let h = match pm {
        Some(true) if h < 12 => h + 12,
        Some(true) if h == 12 => 12,
        Some(false) if h == 12 => 0,
        Some(_) if h > 12 => return None,
        _ => h,
    };
    (h < 24).then_some((h, m))
}

/// "2h", "30m", "90 min", "1 hour", "2 hours" as seconds, and how many words
/// it took.
fn wait_of(words: &[String]) -> Option<(u64, usize)> {
    let first = words.first()?;
    let digits: String = first.chars().take_while(|c| c.is_ascii_digit()).collect();
    let n: u64 = digits.parse().ok()?;
    let unit_here = &first[digits.len()..];
    let (unit, used) = if unit_here.is_empty() {
        (words.get(1)?.as_str(), 2)
    } else {
        (unit_here, 1)
    };
    let per = match unit {
        "m" | "min" | "mins" | "minute" | "minutes" => 60,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3600,
        _ => return None,
    };
    (n > 0 && n <= 72 * 60).then_some((n * per, used))
}

/// The time on the clock `days` days from now's date.
fn at_clock<Tz: TimeZone>(now: &DateTime<Tz>, h: u32, m: u32, days: i64) -> Option<u64> {
    let day = now.clone().naive_local().date() + Duration::days(days);
    let t = now
        .timezone()
        .from_local_datetime(&day.and_time(NaiveTime::from_hms_opt(h, m, 0)?))
        .earliest()?;
    Some(t.timestamp().max(0) as u64)
}

/// The next time the clock reads h:m, today or tomorrow.
fn next_clock<Tz: TimeZone>(now: &DateTime<Tz>, h: u32, m: u32) -> Option<u64> {
    let today = at_clock(now, h, m, 0)?;
    if today > now.timestamp().max(0) as u64 {
        Some(today)
    } else {
        at_clock(now, h, m, 1)
    }
}

/// "at 11:00 PM", "tomorrow at 9:00 AM" or "in 20 minutes", for a note.
pub fn when_words(at: u64, now: u64) -> String {
    if at <= now + 60 {
        return "now".into();
    }
    let secs = at - now;
    if secs < 90 * 60 {
        return format!("in {} minutes", secs.div_ceil(60));
    }
    let local = chrono::Local
        .timestamp_opt(at as i64, 0)
        .single()
        .map(|t| (t.format("%-I:%M %p").to_string(), t.date_naive()));
    let today = chrono::Local::now().date_naive();
    match local {
        Some((clock, day)) if day == today => format!("at {clock}"),
        Some((clock, day)) if Some(day) == today.succ_opt() => format!("tomorrow at {clock}"),
        Some((clock, day)) => format!("on {} at {clock}", day.format("%a %-d %b")),
        None => "later".into(),
    }
}

/// A start time as the Run it later menu shows it: the clock ("11:00 PM")
/// and the day under it ("today", "tomorrow", "Sat 14 Nov"); "now" and
/// nothing when it would start straight away.
pub fn clock_and_day<Tz: TimeZone>(at: u64, now: &DateTime<Tz>) -> (String, String)
where
    Tz::Offset: std::fmt::Display,
{
    let now_s = now.timestamp().max(0) as u64;
    if at <= now_s + 60 {
        return ("now".into(), String::new());
    }
    let Some(t) = now.timezone().timestamp_opt(at as i64, 0).single() else {
        return (String::new(), String::new());
    };
    let today = now.clone().naive_local().date();
    let day = t.naive_local().date();
    let word = if day == today {
        "today".to_string()
    } else if Some(day) == today.succ_opt() {
        "tomorrow".to_string()
    } else {
        day.format("%a %-d %b").to_string()
    };
    (t.format("%-I:%M %p").to_string(), word)
}

/// When a job ran out of its engine's plan: Some with the reset time when
/// the engine said it (seconds since 1970), Some(None) when it ran out but
/// didn't say when, None when it didn't run out.
pub fn ran_out(job: &Job) -> Option<Option<u64>> {
    let mut texts: Vec<&str> = Vec::new();
    for item in job
        .feed
        .get(job.turn_from..)
        .unwrap_or_default()
        .iter()
        .rev()
    {
        match item {
            Item::Error(e) => texts.push(e),
            Item::Text(t) => texts.push(t),
            _ => {}
        }
        if texts.len() >= 3 {
            break;
        }
    }
    for text in &texts {
        if let Some(limit) = crate::planlimit::from_text(text, chrono::Local::now()) {
            return Some(limit.resets_at.map(|t| t.max(0) as u64));
        }
    }
    let lower = texts.join("\n").to_lowercase();
    let worn = lower.contains("rate limit")
        || lower.contains("quota")
        || lower.contains("usage limit")
        || lower.contains("credit balance")
        || lower.contains(" 429");
    worn.then_some(None)
}

/// When a job that ran out should carry on: a minute after its plan resets,
/// or after a while when the engine didn't say.
pub fn resume_time(reset: Option<u64>, now: u64) -> u64 {
    match reset {
        Some(t) if t > now => t + 60,
        _ => now + UNKNOWN_RESET,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn at(h: u32, m: u32) -> DateTime<FixedOffset> {
        FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(2026, 11, 8, h, m, 0)
            .unwrap()
    }

    fn clock(t: u64) -> (u32, u32, u32) {
        let d = FixedOffset::east_opt(0)
            .unwrap()
            .timestamp_opt(t as i64, 0)
            .unwrap();
        use chrono::{Datelike, Timelike};
        (d.day(), d.hour(), d.minute())
    }

    #[test]
    fn tonight_is_eleven_or_now_when_late() {
        let (t, rest) = parse_when("tonight fix the flaky tests", at(14, 0)).unwrap();
        assert_eq!(clock(t), (8, 23, 0));
        assert_eq!(rest, "fix the flaky tests");
        let now = at(23, 30);
        let (t, _) = parse_when("overnight: tidy deps", now).unwrap();
        assert_eq!(t, now.timestamp() as u64);
        let (t, _) = parse_when("tonight tidy", at(2, 0)).unwrap();
        assert_eq!(t, at(2, 0).timestamp() as u64);
    }

    #[test]
    fn clock_times_are_the_next_one() {
        let (t, rest) = parse_when("2am bump the deps", at(14, 0)).unwrap();
        assert_eq!(clock(t), (9, 2, 0));
        assert_eq!(rest, "bump the deps");
        let (t, _) = parse_when("at 16:30 write the docs", at(14, 0)).unwrap();
        assert_eq!(clock(t), (8, 16, 30));
        let (t, _) = parse_when("at 9pm review it", at(22, 0)).unwrap();
        assert_eq!(clock(t), (9, 21, 0));
        let (t, _) = parse_when("12am x", at(14, 0)).unwrap();
        assert_eq!(clock(t), (9, 0, 0));
    }

    #[test]
    fn waits_and_tomorrow() {
        let now = at(10, 0);
        let (t, rest) = parse_when("in 2h run the audit", now).unwrap();
        assert_eq!(t, now.timestamp() as u64 + 7200);
        assert_eq!(rest, "run the audit");
        let (t, rest) = parse_when("in 90 min run it", now).unwrap();
        assert_eq!(t, now.timestamp() as u64 + 5400);
        assert_eq!(rest, "run it");
        let (t, _) = parse_when("tomorrow tidy up", now).unwrap();
        assert_eq!(clock(t), (9, 9, 0));
        let (t, rest) = parse_when("tomorrow at 7am tidy up", now).unwrap();
        assert_eq!(clock(t), (9, 7, 0));
        assert_eq!(rest, "tidy up");
    }

    #[test]
    fn tomorrow_in_the_small_hours_is_this_morning() {
        let (t, _) = parse_when("tomorrow tidy up", at(0, 30)).unwrap();
        assert_eq!(clock(t), (8, 9, 0));
        let (t, _) = parse_when("tomorrow 7am tidy up", at(4, 0)).unwrap();
        assert_eq!(clock(t), (8, 7, 0));
        let (t, _) = parse_when("tomorrow tidy up", at(5, 0)).unwrap();
        assert_eq!(clock(t), (9, 9, 0));
    }

    #[test]
    fn the_menu_shows_a_clock_and_a_day() {
        let now = at(14, 0);
        let (t, _) = parse_when("tonight x", now).unwrap();
        assert_eq!(clock_and_day(t, &now), ("11:00 PM".into(), "today".into()));
        let (t, _) = parse_when("tomorrow x", now).unwrap();
        assert_eq!(
            clock_and_day(t, &now),
            ("9:00 AM".into(), "tomorrow".into())
        );
        let (t, _) = parse_when("in 2h x", now).unwrap();
        assert_eq!(clock_and_day(t, &now), ("4:00 PM".into(), "today".into()));
        let far = now.timestamp() as u64 + 3 * 86400;
        assert_eq!(
            clock_and_day(far, &now),
            ("2:00 PM".into(), "Wed 11 Nov".into())
        );
        let late = at(23, 30);
        let (t, _) = parse_when("tonight x", late).unwrap();
        assert_eq!(clock_and_day(t, &late), ("now".into(), String::new()));
    }

    #[test]
    fn words_that_arent_times_stay_words() {
        assert!(parse_when("fix 2 bugs", at(10, 0)).is_none());
        assert!(parse_when("in the header fix it", at(10, 0)).is_none());
        assert!(parse_when("tonight", at(10, 0)).is_none());
        assert!(parse_when("at 25:00 x", at(10, 0)).is_none());
    }

    fn job_said(text: &str) -> Job {
        Job {
            feed: vec![Item::Text("Working on it".into()), Item::Error(text.into())],
            ..Job::default()
        }
    }

    #[test]
    fn running_out_is_noticed() {
        assert!(ran_out(&job_said("Claude usage limit reached.")).is_some());
        assert_eq!(ran_out(&job_said("429 rate limit exceeded")), Some(None));
        assert_eq!(ran_out(&job_said("tests failed")), None);
        assert_eq!(resume_time(Some(1000), 500), 1060);
        assert_eq!(resume_time(None, 500), 500 + UNKNOWN_RESET);
    }
}
