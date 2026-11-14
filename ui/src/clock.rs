// Copyright (C) 2026 Julian Decker
// SPDX-License-Identifier: GPL-2.0-or-later

//! Wall-clock time for the screen and for file names: POSIX time zone strings (`CET-1CEST,M3.5.0,M10.5.0/3`, with
//! daylight saving time), the civil calendar, and the formats OOS shows. The unit has no RTC battery: the system clock
//! is only right after an NTP sync (`sntp`) or an entry by hand (System > Date and time).

/// Seconds per day.
const DAY: i64 = 86_400;

/// Days since 1970-01-01 of a civil date (proleptic Gregorian; Howard Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Civil date (year, month 1-12, day 1-31) of a day number since 1970-01-01.
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { yoe + era * 400 + 1 } else { yoe + era * 400 }, m, d)
}

fn leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn month_days(y: i64, m: u32) -> u32 {
    match m {
        2 if leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The day of a transition in a year.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Date {
    /// `Jn`: day 1..365, February 29 never counted
    Julian(u32),
    /// `n`: day 0..365, February 29 counted
    Zero(u32),
    /// `Mm.w.d`: day d (0 = Sunday) of week w (5 = the last) of month m
    Month(u32, u32, u32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rule {
    date: Date,
    /// seconds after local midnight (may be negative or past 24 h)
    time: i32,
}

impl Rule {
    /// Day number (since 1970-01-01) of the rule in year `y`.
    fn day(&self, y: i64) -> i64 {
        let jan1 = days_from_civil(y, 1, 1);
        match self.date {
            Date::Julian(n) => jan1 + n as i64 - 1 + if leap(y) && n >= 60 { 1 } else { 0 },
            Date::Zero(n) => jan1 + n as i64,
            Date::Month(m, w, d) => {
                let first = days_from_civil(y, m, 1);
                // 1970-01-01 was a Thursday
                let wd = (first + 4).rem_euclid(7);
                let mut day = first + (d as i64 - wd).rem_euclid(7) + 7 * (w as i64 - 1);
                while day >= first + month_days(y, m) as i64 {
                    day -= 7;
                }
                day
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Dst {
    name: String,
    /// seconds east of UTC
    off: i32,
    start: Rule,
    end: Rule,
}

/// A POSIX time zone: `std offset [dst [offset] [,start[/time],end[/time]]]`. Offsets count west of Greenwich, as in
/// POSIX (`CET-1` is one hour east), names are three or more letters or `<...>` (`<+0530>-5:30`).
#[derive(Clone, Debug, PartialEq)]
pub struct Tz {
    name: String,
    /// seconds east of UTC
    off: i32,
    dst: Option<Dst>,
}

/// The longest time zone string taken.
pub const TZ_MAX: usize = 60;

/// Common zones for the picker: a name to recognise, its POSIX string.
pub const ZONES: [(&str, &str); 22] = [
    ("UTC", "UTC0"),
    ("London, Lisbon", "GMT0BST,M3.5.0/1,M10.5.0"),
    ("Berlin, Paris, Rome", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Athens, Helsinki, Kyiv", "EET-2EEST,M3.5.0/3,M10.5.0/4"),
    ("Istanbul, Moscow", "<+03>-3"),
    ("Dubai", "<+04>-4"),
    ("India", "IST-5:30"),
    ("Bangkok, Jakarta", "<+07>-7"),
    ("China, Singapore", "CST-8"),
    ("Japan, Korea", "JST-9"),
    ("Brisbane", "AEST-10"),
    ("Sydney, Melbourne", "AEST-10AEDT,M10.1.0,M4.1.0/3"),
    ("New Zealand", "NZST-12NZDT,M9.5.0,M4.1.0/3"),
    ("Honolulu", "HST10"),
    ("Anchorage", "AKST9AKDT,M3.2.0,M11.1.0"),
    ("Los Angeles, Seattle", "PST8PDT,M3.2.0,M11.1.0"),
    ("Phoenix", "MST7"),
    ("Denver, Calgary", "MST7MDT,M3.2.0,M11.1.0"),
    ("Chicago, Winnipeg", "CST6CDT,M3.2.0,M11.1.0"),
    ("Mexico City", "CST6"),
    ("New York, Toronto", "EST5EDT,M3.2.0,M11.1.0"),
    ("Buenos Aires, Brasilia", "<-03>3"),
];

/// The picker name of a zone string, if it is one of `ZONES`.
pub fn zone_name(tz: &str) -> Option<&'static str> {
    ZONES.iter().find(|z| z.1 == tz).map(|z| z.0)
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn num(&mut self, max: u32) -> Result<u32, String> {
        let st = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) && self.i - st < 3 {
            self.i += 1;
        }
        let n: u32 = std::str::from_utf8(&self.s[st..self.i]).ok().and_then(|t| t.parse().ok()).ok_or("a number is missing")?;
        if n > max {
            return Err(format!("{} is too large", n));
        }
        Ok(n)
    }

    fn name(&mut self) -> Result<String, String> {
        if self.eat(b'<') {
            let st = self.i;
            while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == b'+' || c == b'-') {
                self.i += 1;
            }
            let n = String::from_utf8_lossy(&self.s[st..self.i]).into_owned();
            if !self.eat(b'>') || n.len() < 3 {
                return Err("a <name> needs three characters and '>'".into());
            }
            return Ok(n);
        }
        let st = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            self.i += 1;
        }
        if self.i - st < 3 {
            return Err("a zone name needs three letters".into());
        }
        Ok(String::from_utf8_lossy(&self.s[st..self.i]).into_owned())
    }

    /// `[+-]hh[:mm[:ss]]` in seconds, hours up to `hmax`.
    fn hms(&mut self, hmax: u32) -> Result<i32, String> {
        let neg = if self.eat(b'-') {
            true
        } else {
            self.eat(b'+');
            false
        };
        let mut t = self.num(hmax)? as i32 * 3600;
        if self.eat(b':') {
            t += self.num(59)? as i32 * 60;
            if self.eat(b':') {
                t += self.num(59)? as i32;
            }
        }
        Ok(if neg { -t } else { t })
    }

    fn rule(&mut self) -> Result<Rule, String> {
        let date = if self.eat(b'M') {
            let m = self.num(12)?;
            let ok = self.eat(b'.');
            let w = self.num(5)?;
            let ok = ok && self.eat(b'.');
            let d = self.num(6)?;
            if !ok || m == 0 || w == 0 {
                return Err("a rule is Mm.w.d (month 1-12, week 1-5, day 0-6)".into());
            }
            Date::Month(m, w, d)
        } else if self.eat(b'J') {
            let n = self.num(365)?;
            if n == 0 {
                return Err("Jn counts from 1".into());
            }
            Date::Julian(n)
        } else {
            Date::Zero(self.num(365)?)
        };
        let time = if self.eat(b'/') { self.hms(167)? } else { 7200 };
        Ok(Rule { date, time })
    }
}

impl Tz {
    pub fn utc() -> Tz {
        Tz { name: "UTC".into(), off: 0, dst: None }
    }

    pub fn parse(s: &str) -> Result<Tz, String> {
        let s = s.trim();
        if s.is_empty() || s.len() > TZ_MAX || !s.is_ascii() {
            return Err("not a time zone".into());
        }
        let mut p = P { s: s.as_bytes(), i: 0 };
        let name = p.name()?;
        let off = -p.hms(24)?;
        let mut tz = Tz { name, off, dst: None };
        if p.peek().is_some() {
            let name = p.name()?;
            let doff = if p.peek().is_some_and(|c| c != b',') { -p.hms(24)? } else { off + 3600 };
            // without rules: the US rules, as other systems do
            let (start, end) = if p.eat(b',') {
                let a = p.rule()?;
                if !p.eat(b',') {
                    return Err("daylight saving time needs a start and an end".into());
                }
                (a, p.rule()?)
            } else {
                (Rule { date: Date::Month(3, 2, 0), time: 7200 }, Rule { date: Date::Month(11, 1, 0), time: 7200 })
            };
            tz.dst = Some(Dst { name, off: doff, start, end });
        }
        if p.peek().is_some() {
            return Err(format!("unexpected '{}'", p.peek().unwrap() as char));
        }
        Ok(tz)
    }

    /// Seconds east of UTC at `unix`, and whether daylight saving time is in effect.
    pub fn offset_at(&self, unix: i64) -> (i32, bool) {
        let Some(d) = &self.dst else { return (self.off, false) };
        let y = civil_from_days((unix + self.off as i64).div_euclid(DAY)).0;
        // the start is given in standard time, the end in daylight saving time
        let start = d.start.day(y) * DAY + d.start.time as i64 - self.off as i64;
        let end = d.end.day(y) * DAY + d.end.time as i64 - d.off as i64;
        let on = if start < end { unix >= start && unix < end } else { !(unix >= end && unix < start) };
        if on {
            (d.off, true)
        } else {
            (self.off, false)
        }
    }

    /// The zone's abbreviation at `unix` ("CET", "CEST", "+0530").
    pub fn abbr_at(&self, unix: i64) -> &str {
        match (&self.dst, self.offset_at(unix).1) {
            (Some(d), true) => &d.name,
            _ => &self.name,
        }
    }

    /// Local time at `unix`.
    pub fn local(&self, unix: i64) -> Local {
        let (off, _) = self.offset_at(unix);
        let t = unix + off as i64;
        let (y, mo, d) = civil_from_days(t.div_euclid(DAY));
        let s = t.rem_euclid(DAY) as u32;
        Local { y, mo, d, h: s / 3600, mi: s / 60 % 60, s: s % 60 }
    }

    /// The UTC time of a local time (in a gap or a doubled hour of a change: one of the two readings).
    pub fn to_unix(&self, l: &Local) -> i64 {
        let t = days_from_civil(l.y, l.mo, l.d) * DAY + (l.h * 3600 + l.mi * 60 + l.s) as i64;
        let first = t - self.offset_at(t - self.off as i64).0 as i64;
        t - self.offset_at(first).0 as i64
    }
}

/// A local date and time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Local {
    pub y: i64,
    pub mo: u32,
    pub d: u32,
    pub h: u32,
    pub mi: u32,
    pub s: u32,
}

impl Local {
    /// `2026-11-11 14:30:05`
    pub fn text(&self) -> String {
        format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", self.y, self.mo, self.d, self.h, self.mi, self.s)
    }

    /// `2026-11-11 14:30`, what the keyboard of System > Date and time starts with
    pub fn text_min(&self) -> String {
        format!("{:04}-{:02}-{:02} {:02}:{:02}", self.y, self.mo, self.d, self.h, self.mi)
    }

    /// `14:30:05`
    pub fn hms(&self) -> String {
        format!("{:02}:{:02}:{:02}", self.h, self.mi, self.s)
    }

    /// `20261111-143005`, for file names: sorts by time
    pub fn stamp(&self) -> String {
        format!("{:04}{:02}{:02}-{:02}{:02}{:02}", self.y, self.mo, self.d, self.h, self.mi, self.s)
    }

    /// `YYYY-MM-DD HH:MM[:SS]` (also `T` between date and time).
    pub fn parse(s: &str) -> Result<Local, String> {
        let bad = || "Date and time as 2026-11-11 14:30".to_string();
        let s = s.trim();
        let (date, time) = s.split_once([' ', 'T']).ok_or_else(bad)?;
        let n = |t: &str| t.trim().parse::<u32>().map_err(|_| bad());
        let dv: Vec<&str> = date.split('-').collect();
        let tv: Vec<&str> = time.trim().split(':').collect();
        if dv.len() != 3 || !(2..=3).contains(&tv.len()) {
            return Err(bad());
        }
        let (y, mo, d) = (n(dv[0])? as i64, n(dv[1])?, n(dv[2])?);
        let (h, mi) = (n(tv[0])?, n(tv[1])?);
        let s = if tv.len() == 3 { n(tv[2])? } else { 0 };
        if !(2000..=2099).contains(&y) {
            return Err("The year must be 2000 to 2099".into());
        }
        if !(1..=12).contains(&mo) || d == 0 || d > month_days(y, mo) || h > 23 || mi > 59 || s > 59 {
            return Err(bad());
        }
        Ok(Local { y, mo, d, h, mi, s })
    }
}

/// Seconds since 1970 now (system clock). `OOS_FIXED_TIME=<seconds>` stops it there, for host test runs whose pictures
/// must not change with the time of day (tools/layout-check.sh).
pub fn unix_now() -> f64 {
    static FIXED: std::sync::OnceLock<Option<f64>> = std::sync::OnceLock::new();
    if let Some(t) = FIXED.get_or_init(|| std::env::var("OOS_FIXED_TIME").ok().and_then(|v| v.parse().ok())) {
        return *t;
    }
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unix(y: i64, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
        days_from_civil(y, mo, d) * DAY + (h * 3600 + mi * 60) as i64
    }

    #[test]
    fn civil_calendar_both_ways() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        for d in (-800_000..800_000).step_by(997) {
            let (y, m, dd) = civil_from_days(d);
            assert_eq!(days_from_civil(y, m, dd), d);
        }
        assert_eq!(Tz::utc().local(1_762_862_400).text(), "2025-11-11 12:00:00");
        assert_eq!(Tz::utc().local(unix(2026, 11, 11, 14, 30) + 5).text(), "2026-11-11 14:30:05");
    }

    #[test]
    fn central_europe_changes_on_the_last_sundays() {
        let tz = Tz::parse("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
        // 2026: March 29 and October 25, at 01:00 UTC
        assert_eq!(tz.offset_at(unix(2026, 3, 29, 0, 59)), (3600, false));
        assert_eq!(tz.offset_at(unix(2026, 3, 29, 1, 0)), (7200, true));
        assert_eq!(tz.offset_at(unix(2026, 10, 25, 0, 59)), (7200, true));
        assert_eq!(tz.offset_at(unix(2026, 10, 25, 1, 0)), (3600, false));
        assert_eq!(tz.abbr_at(unix(2026, 7, 1, 0, 0)), "CEST");
        assert_eq!(tz.abbr_at(unix(2026, 11, 11, 0, 0)), "CET");
        assert_eq!(tz.local(unix(2026, 7, 1, 10, 0)).text(), "2026-07-01 12:00:00");
        assert_eq!(tz.local(unix(2026, 11, 11, 13, 30)).stamp(), "20261111-143000");
    }

    #[test]
    fn north_america_and_the_default_rules() {
        let a = Tz::parse("EST5EDT,M3.2.0,M11.1.0").unwrap();
        let b = Tz::parse("EST5EDT").unwrap();
        assert_eq!(a, b);
        // 2026: March 8 07:00 UTC, November 1 06:00 UTC
        assert_eq!(a.offset_at(unix(2026, 3, 8, 6, 59)).0, -5 * 3600);
        assert_eq!(a.offset_at(unix(2026, 3, 8, 7, 0)).0, -4 * 3600);
        assert_eq!(a.offset_at(unix(2026, 11, 1, 5, 59)).0, -4 * 3600);
        assert_eq!(a.offset_at(unix(2026, 11, 1, 6, 0)).0, -5 * 3600);
    }

    #[test]
    fn southern_hemisphere_wraps_the_year() {
        let tz = Tz::parse("AEST-10AEDT,M10.1.0,M4.1.0/3").unwrap();
        assert_eq!(tz.offset_at(unix(2026, 1, 15, 0, 0)), (11 * 3600, true));
        assert_eq!(tz.offset_at(unix(2026, 6, 15, 0, 0)), (10 * 3600, false));
        assert_eq!(tz.offset_at(unix(2026, 12, 15, 0, 0)), (11 * 3600, true));
        // 2026-04-05 03:00 AEDT = 04-04 16:00 UTC
        assert!(tz.offset_at(unix(2026, 4, 4, 15, 59)).1);
        assert!(!tz.offset_at(unix(2026, 4, 4, 16, 0)).1);
    }

    #[test]
    fn offsets_names_and_julian_days() {
        let i = Tz::parse("IST-5:30").unwrap();
        assert_eq!(i.offset_at(0), (5 * 3600 + 1800, false));
        let q = Tz::parse("<+0545>-5:45").unwrap();
        assert_eq!((q.offset_at(0).0, q.abbr_at(0)), (5 * 3600 + 2700, "+0545"));
        let s = Tz::parse("<-03>3").unwrap();
        assert_eq!(s.offset_at(0).0, -3 * 3600);
        assert_eq!(Tz::parse("UTC0").unwrap().offset_at(0), (0, false));
        // J60 is March 1 also in a leap year; 59 (zero-based) is February 29 in one
        let j = Rule { date: Date::Julian(60), time: 0 };
        assert_eq!(civil_from_days(j.day(2028)), (2028, 3, 1));
        let z = Rule { date: Date::Zero(59), time: 0 };
        assert_eq!(civil_from_days(z.day(2028)), (2028, 2, 29));
        let t = Tz::parse("XXX-2YYY-3,J100/1:30,J300").unwrap();
        assert_eq!(t.offset_at(unix(2026, 7, 1, 0, 0)), (3 * 3600, true));
        // a rule with a negative time and one past midnight
        assert!(Tz::parse("<-02>2<-01>,M3.5.0/-1,M10.5.0/25").is_ok());
        for bad in ["", "C", "CET", "CET-1CEST,M3.5.0", "CET-1CEST,M13.5.0,M10.5.0", "CET-1x", "<ab>1", "CET-1CEST,M3.0.0,M10.5.0", "Ä-1"] {
            assert!(Tz::parse(bad).is_err(), "{}", bad);
        }
    }

    #[test]
    fn every_listed_zone_parses() {
        for (n, z) in ZONES {
            assert!(Tz::parse(z).is_ok(), "{} {}", n, z);
            assert_eq!(zone_name(z), Some(n));
        }
    }

    #[test]
    fn local_time_back_to_utc() {
        let tz = Tz::parse("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
        for t in [unix(2026, 1, 1, 0, 0), unix(2026, 7, 1, 12, 34), unix(2026, 3, 29, 1, 0), unix(2026, 10, 25, 2, 0)] {
            assert_eq!(tz.to_unix(&tz.local(t)), t);
        }
        let l = Local::parse("2026-11-11 14:30").unwrap();
        assert_eq!(l.text(), "2026-11-11 14:30:00");
        assert_eq!(tz.to_unix(&l), unix(2026, 11, 11, 13, 30));
        assert_eq!(Local::parse(" 2026-02-28T23:59:59 ").unwrap().s, 59);
        for bad in ["2026-11-11", "2026-02-29 10:00", "2026-13-01 10:00", "1999-01-01 00:00", "2026-11-11 24:00", "x-y-z 1:2", "2026-11-11 10"] {
            assert!(Local::parse(bad).is_err(), "{}", bad);
        }
    }
}
