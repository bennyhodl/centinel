//! When a scheduled run is due — the pure half of `docs/SCHEDULING.md`.
//!
//! Everything here is a function of `(expression, zone, last attempt, now)`. There are no
//! timers, no tasks, and no I/O: the loop that sleeps and fires lives in the binary crate
//! beside `http.rs` and `mcp.rs`, because it is the same kind of thing they are — a
//! surface that drives the op registry.
//!
//! That split is what makes any of this testable. "Does a monthly schedule fire on the
//! 1st", "what happens on the day the clocks go forward", and "how many runs does a
//! fortnight of downtime owe you" are all questions about an instant that is not now, and
//! a module that owned a clock could not be asked them.
//!
//! ## Why cron and not an interval
//!
//! `every = "24h"` drifts. An interval restarted after a forty-minute run moves forty
//! minutes later every day and walks into business hours within a fortnight — against a
//! city's web server, which is what the politeness stance exists to protect. It also
//! cannot say "02:00 on the 1st", the natural cadence for a source that publishes monthly.
//!
//! ## Why the parser is here and not a dependency
//!
//! Five fields, five range parsers, and the whole grammar fits on a page. The crates that
//! implement it are built on `chrono`, and the *hard* part is not the grammar — it is
//! stepping through local time across a DST boundary, which is `jiff`'s job either way.
//! Taking a crate would have put a second date library on a direct path to buy the easy
//! half.

use std::fmt;

use jiff::civil::DateTime;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan, Unit, Zoned};

/// How far ahead [`Cron::next_after`] will search before giving up.
///
/// A schedule like `0 0 30 2 *` — the 30th of February — is syntactically valid and never
/// occurs. Without a bound the search for its next fire is an infinite loop inside a
/// server's startup validation. Five years is far past any real cadence and still a few
/// thousand iterations.
const SEARCH_LIMIT_YEARS: i16 = 5;

/// A parsed 5-field cron expression: minute, hour, day-of-month, month, day-of-week.
///
/// Each field is a set of the values it matches, precomputed at parse time. Matching is
/// then a lookup rather than a re-parse, which matters because finding the next fire
/// across a sparse expression steps minute by minute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cron {
    /// The text it was parsed from, so a report can show what the operator wrote rather
    /// than a rendering of what it was understood to mean.
    source: String,
    minutes: Vec<u8>,
    hours: Vec<u8>,
    days_of_month: Vec<u8>,
    months: Vec<u8>,
    days_of_week: Vec<u8>,
    /// Whether day-of-month and day-of-week were *both* restricted. See [`Cron::matches`]
    /// — this is the one place cron's semantics are not intersection.
    both_days_restricted: bool,
}

/// A cron expression that could not be parsed, naming the field rather than the position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidCron {
    pub expression: String,
    pub reason: String,
}

impl fmt::Display for InvalidCron {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is not a cron expression: {}",
            self.expression, self.reason
        )
    }
}

impl std::error::Error for InvalidCron {}

/// The shorthands, because nobody should have to remember whether Sunday is 0 or 7.
const ALIASES: &[(&str, &str)] = &[
    ("@yearly", "0 0 1 1 *"),
    ("@annually", "0 0 1 1 *"),
    ("@monthly", "0 0 1 * *"),
    ("@weekly", "0 0 * * 0"),
    ("@daily", "0 0 * * *"),
    ("@midnight", "0 0 * * *"),
    ("@hourly", "0 * * * *"),
];

const MONTH_NAMES: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];
const DAY_NAMES: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

impl Cron {
    /// Parses a 5-field expression, or one of the `@daily` shorthands.
    pub fn parse(expression: &str) -> Result<Self, InvalidCron> {
        let raw = expression.trim();
        let bad = |reason: &str| InvalidCron {
            expression: expression.to_string(),
            reason: reason.to_string(),
        };

        let expanded = ALIASES
            .iter()
            .find(|(alias, _)| alias.eq_ignore_ascii_case(raw))
            .map(|(_, spec)| *spec)
            .unwrap_or(raw);

        if expanded.starts_with('@') {
            let known = ALIASES
                .iter()
                .map(|(a, _)| *a)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(bad(&format!("unknown shorthand; try one of: {known}")));
        }

        let fields: Vec<&str> = expanded.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(bad(&format!(
                "expected 5 fields (minute hour day-of-month month day-of-week), found {}",
                fields.len()
            )));
        }

        let minutes = parse_field(fields[0], 0, 59, &[], "minute").map_err(|e| bad(&e))?;
        let hours = parse_field(fields[1], 0, 23, &[], "hour").map_err(|e| bad(&e))?;
        let days_of_month =
            parse_field(fields[2], 1, 31, &[], "day-of-month").map_err(|e| bad(&e))?;
        let months = parse_field(fields[3], 1, 12, &MONTH_NAMES, "month").map_err(|e| bad(&e))?;
        let mut days_of_week =
            parse_field(fields[4], 0, 7, &DAY_NAMES, "day-of-week").map_err(|e| bad(&e))?;

        // Both 0 and 7 mean Sunday, and every real crontab relies on it.
        if days_of_week.contains(&7) {
            days_of_week.retain(|d| *d != 7);
            if !days_of_week.contains(&0) {
                days_of_week.push(0);
            }
            days_of_week.sort_unstable();
        }

        Ok(Self {
            source: raw.to_string(),
            minutes,
            hours,
            both_days_restricted: !is_wildcard(fields[2]) && !is_wildcard(fields[4]),
            days_of_month,
            months,
            days_of_week,
        })
    }

    /// The expression as the operator wrote it.
    pub fn as_str(&self) -> &str {
        &self.source
    }

    /// Whether a local wall-clock time is a fire time.
    ///
    /// **Day-of-month and day-of-week are ORed when both are restricted**, which is the
    /// one place cron does not intersect its fields and the one place a hand-rolled
    /// parser gets it wrong. `0 0 1 * 1` is "the 1st, *and also* every Monday" — not "a
    /// Monday that is the 1st", which would fire about once a year and look like a broken
    /// schedule rather than a misread one. Vixie cron has behaved this way since 1987 and
    /// operators write expressions expecting it.
    fn matches(&self, when: DateTime) -> bool {
        if !self.minutes.contains(&(when.minute() as u8))
            || !self.hours.contains(&(when.hour() as u8))
            || !self.months.contains(&(when.month() as u8))
        {
            return false;
        }

        let dom = self.days_of_month.contains(&(when.day() as u8));
        // jiff counts Monday=1..Sunday=7; cron counts Sunday=0..Saturday=6.
        let weekday = when.weekday().to_monday_one_offset() % 7;
        let dow = self.days_of_week.contains(&(weekday as u8));

        if self.both_days_restricted {
            dom || dow
        } else {
            dom && dow
        }
    }

    /// The first fire time strictly after `after`, in `zone`.
    ///
    /// Returns `None` for an expression that never occurs — the 30th of February, or a
    /// February-only Wednesday-the-31st. A server refuses to start on one rather than
    /// waiting five years to notice (SCHEDULING §9.1).
    ///
    /// ## The two DST cases
    ///
    /// Stepping through *civil* time and converting each candidate is what makes these
    /// answerable at all; stepping through instants would skip an hour of local time in
    /// spring and repeat one in autumn.
    ///
    /// - **A fire time that does not exist** — 02:30 on a spring-forward day — is shifted
    ///   forward by the length of the gap, so it fires at 03:30. Not "the first instant
    ///   after the gap": keeping the offset from the hour is what makes a 02:00 schedule
    ///   and a 02:30 schedule stay half an hour apart on the one morning of the year they
    ///   would otherwise collide. This is jiff's `compatible` rule, which is also what
    ///   Temporal and ICU do — a bespoke rule here would be a second answer to a question
    ///   the ecosystem has already settled.
    /// - **A fire time that happens twice** — 01:30 on a fall-back day — fires **once**,
    ///   at the earlier of the two, because `after` is already past by the second.
    ///
    /// Getting these wrong is a missed day and a doubled day, once a year each, in the
    /// direction nobody is watching.
    pub fn next_after(&self, after: Timestamp, zone: &TimeZone) -> Option<Timestamp> {
        let local = after.to_zoned(zone.clone());
        let horizon = local.year() + SEARCH_LIMIT_YEARS;

        // Start at the next whole minute: cron has minute resolution, and a candidate
        // equal to `after` would fire the same schedule twice.
        let mut candidate = local
            .datetime()
            .with()
            .second(0)
            .subsec_nanosecond(0)
            .build()
            .ok()?
            .checked_add(1.minute())
            .ok()?;

        while candidate.year() <= horizon {
            if self.matches(candidate) {
                // `compatible` is the rule this module documents: a gap shifts forward
                // by the gap's length, a fold resolves to the earlier of the two.
                let zoned: Zoned = zone.to_ambiguous_zoned(candidate).compatible().ok()?;
                let at = zoned.timestamp();
                // A gap can push the instant back before `after` only if `after` itself
                // sat inside the gap, which cannot happen — but a fold *can* produce an
                // instant already passed, and firing it would repeat the run.
                if at > after {
                    return Some(at);
                }
            }
            candidate = candidate.checked_add(1.minute()).ok()?;
        }
        None
    }

    /// The next `count` fire times — what the selector previews before writing a block.
    ///
    /// The whole reason the wizard earns its place: `0 3 * * 1` is Mondays and `0 3 1 * *`
    /// is the 1st, and three dates settle which one was meant.
    pub fn next_n(&self, after: Timestamp, zone: &TimeZone, count: usize) -> Vec<Timestamp> {
        let mut out = Vec::with_capacity(count);
        let mut cursor = after;
        for _ in 0..count {
            match self.next_after(cursor, zone) {
                Some(at) => {
                    out.push(at);
                    cursor = at;
                }
                None => break,
            }
        }
        out
    }

    /// The shortest gap between consecutive fires, used to decide what "one interval
    /// late" means for catch-up.
    ///
    /// Sampled over the next few fires rather than derived from the expression, because
    /// the expression's period is not a closed form: `0 3 * * 1,2` is one day and then
    /// six. The *shortest* gap is the conservative choice — it makes catch-up trigger
    /// sooner, and a spurious catch-up run costs one pass of skip predicates over a
    /// current corpus while a missed one costs a day of collection.
    pub fn shortest_interval(&self, from: Timestamp, zone: &TimeZone) -> Option<jiff::Span> {
        let fires = self.next_n(from, zone, 5);
        fires
            .windows(2)
            .map(|w| w[1] - w[0])
            .min_by_key(|s| s.total(Unit::Second).unwrap_or(f64::MAX) as i64)
    }
}

impl fmt::Display for Cron {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.source)
    }
}

/// `*` or `*/n` — the forms that leave a field unrestricted for the OR rule above.
///
/// A step is still a wildcard for this purpose: `*/2` in day-of-month restricts *which*
/// days but is not the operator saying "these specific days", which is what the ORing is
/// meant to capture.
fn is_wildcard(field: &str) -> bool {
    field == "*" || field.starts_with("*/")
}

/// Parses one field into the sorted, deduplicated set of values it matches.
fn parse_field(
    field: &str,
    min: u8,
    max: u8,
    names: &[&str],
    label: &str,
) -> Result<Vec<u8>, String> {
    let mut values = Vec::new();

    for part in field.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err(format!("{label} has an empty entry"));
        }

        let (range_part, step) = match part.split_once('/') {
            Some((r, s)) => {
                let step: u8 = s
                    .parse()
                    .map_err(|_| format!("{label}: `{s}` is not a step"))?;
                if step == 0 {
                    return Err(format!("{label}: a step of zero matches nothing"));
                }
                (r, step)
            }
            None => (part, 1),
        };

        let (lo, hi) = if range_part == "*" {
            (min, max)
        } else if let Some((a, b)) = range_part.split_once('-') {
            (
                parse_value(a, min, max, names, label)?,
                parse_value(b, min, max, names, label)?,
            )
        } else {
            let v = parse_value(range_part, min, max, names, label)?;
            // A bare value with a step means "from here to the end", as in `5/15`.
            if step > 1 { (v, max) } else { (v, v) }
        };

        if lo > hi {
            return Err(format!(
                "{label}: `{range_part}` counts backwards; write it as two entries"
            ));
        }

        let mut v = lo;
        while v <= hi {
            values.push(v);
            // The last step can overflow `u8` on a field whose max is 59 and whose step
            // is large; saturating keeps the loop terminating rather than wrapping to 0.
            v = match v.checked_add(step) {
                Some(next) => next,
                None => break,
            };
        }
    }

    values.sort_unstable();
    values.dedup();
    if values.is_empty() {
        return Err(format!("{label} matches nothing"));
    }
    Ok(values)
}

/// One number, or a three-letter name where the field allows them.
fn parse_value(raw: &str, min: u8, max: u8, names: &[&str], label: &str) -> Result<u8, String> {
    let raw = raw.trim();
    if let Some(i) = names
        .iter()
        .position(|n| n.eq_ignore_ascii_case(raw) || raw.to_ascii_lowercase().starts_with(n))
    {
        // Months are 1-based, weekdays 0-based, and `min` already says which.
        return Ok(i as u8 + min);
    }
    let value: u8 = raw
        .parse()
        .map_err(|_| format!("{label}: `{raw}` is not a number"))?;
    if value < min || value > max {
        return Err(format!("{label}: {value} is outside {min}–{max}"));
    }
    Ok(value)
}

// ── jitter ────────────────────────────────────────────────────────────────────

/// A deterministic offset within `[0, span)` for one schedule on one machine.
///
/// **Why jitter exists at all:** the licence is MIT and forks are the point — other cities
/// run their own instance. Twenty installs sharing a default `0 3 * * *`, against a
/// handful of shared vendor platforms, is a small synchronised flood at 03:00 from a
/// project whose stated stance is politeness.
///
/// **Why it is deterministic:** an operator has to be able to predict their own fire
/// times, and `schedules` has to be able to print the real one. Randomising per fire would
/// make the reported "next" a lie the moment it was printed. Seeded on the machine's
/// identity and the schedule id, so two installs differ and one install does not.
pub fn jitter_offset(node_seed: &str, schedule_id: &str, span_secs: u64) -> u64 {
    if span_secs == 0 {
        return 0;
    }
    // FNV-1a: a hash whose value must be stable across processes and releases, which
    // rules out `DefaultHasher` — its output is explicitly not guaranteed between
    // versions, and a Rust upgrade would silently move every fire time.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in node_seed.bytes().chain([0]).chain(schedule_id.bytes()) {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash % span_secs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tz(name: &str) -> TimeZone {
        TimeZone::get(name).unwrap()
    }

    fn at(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    /// Renders a fire time in its own zone, which is the only way these read.
    fn local(ts: Timestamp, zone: &TimeZone) -> String {
        ts.to_zoned(zone.clone())
            .strftime("%Y-%m-%d %H:%M %Z")
            .to_string()
    }

    #[test]
    fn a_daily_expression_fires_once_a_day_at_the_named_hour() {
        let c = Cron::parse("0 3 * * *").unwrap();
        let z = tz("America/New_York");
        let fires = c.next_n(at("2026-08-06T00:00:00Z"), &z, 3);
        assert_eq!(local(fires[0], &z), "2026-08-06 03:00 EDT");
        assert_eq!(local(fires[1], &z), "2026-08-07 03:00 EDT");
        assert_eq!(local(fires[2], &z), "2026-08-08 03:00 EDT");
    }

    /// The confusion the selector's preview exists to settle, asserted so the parser
    /// cannot drift from what the preview claims.
    #[test]
    fn day_of_week_and_day_of_month_are_different_schedules() {
        let z = tz("UTC");
        let start = at("2026-08-06T00:00:00Z"); // a Thursday

        let mondays = Cron::parse("0 3 * * 1").unwrap();
        assert_eq!(
            local(mondays.next_after(start, &z).unwrap(), &z),
            "2026-08-10 03:00 UTC"
        );

        let first = Cron::parse("0 3 1 * *").unwrap();
        assert_eq!(
            local(first.next_after(start, &z).unwrap(), &z),
            "2026-09-01 03:00 UTC"
        );
    }

    /// Cron's one non-intersecting rule. Getting this wrong turns "the 1st or any Monday"
    /// into "a Monday falling on the 1st" — roughly annual, and indistinguishable from a
    /// schedule that is simply broken.
    #[test]
    fn restricting_both_day_fields_ors_them() {
        let z = tz("UTC");
        let c = Cron::parse("0 0 1 * 1").unwrap();
        let fires = c.next_n(at("2026-08-06T00:00:00Z"), &z, 4);
        let rendered: Vec<String> = fires.iter().map(|f| local(*f, &z)).collect();
        assert_eq!(
            rendered,
            [
                "2026-08-10 00:00 UTC", // Monday
                "2026-08-17 00:00 UTC", // Monday
                "2026-08-24 00:00 UTC", // Monday
                "2026-08-31 00:00 UTC", // Monday
            ]
        );
        // And the 1st still fires even though it is a Tuesday.
        let sept = c.next_n(at("2026-08-31T12:00:00Z"), &z, 1);
        assert_eq!(local(sept[0], &z), "2026-09-01 00:00 UTC");
    }

    /// Only one field restricted means the other is a wildcard, and the OR must not
    /// kick in — otherwise `0 3 * * 1` would fire every day.
    #[test]
    fn one_restricted_day_field_still_intersects() {
        let z = tz("UTC");
        let c = Cron::parse("0 3 * * 1").unwrap();
        let fires = c.next_n(at("2026-08-06T00:00:00Z"), &z, 2);
        assert_eq!(local(fires[0], &z), "2026-08-10 03:00 UTC");
        assert_eq!(local(fires[1], &z), "2026-08-17 03:00 UTC");
    }

    /// 02:30 does not exist on the spring-forward morning. Skipping it would silently
    /// drop a day of collection once a year.
    #[test]
    fn a_fire_time_inside_a_dst_gap_moves_to_the_next_real_instant() {
        let z = tz("America/New_York");
        // 2026-03-08: clocks jump 02:00 → 03:00.
        let c = Cron::parse("30 2 * * *").unwrap();
        let fire = c.next_after(at("2026-03-08T00:00:00-05:00"), &z).unwrap();
        assert_eq!(
            local(fire, &z),
            "2026-03-08 03:30 EDT",
            "a nonexistent local time must shift forward by the gap, not vanish"
        );

        // And it must still be one fire, not two — the following day is back to normal.
        assert_eq!(
            local(c.next_after(fire, &z).unwrap(), &z),
            "2026-03-09 02:30 EDT"
        );
    }

    /// 01:30 happens twice on the fall-back morning. Firing both would run the schedule
    /// twice, which for a monthly source is a doubled crawl nobody asked for.
    #[test]
    fn a_fire_time_inside_a_dst_fold_happens_once() {
        let z = tz("America/New_York");
        // 2026-11-01: clocks fall back 02:00 → 01:00, so 01:30 occurs at -04:00 and again
        // at -05:00.
        let c = Cron::parse("30 1 * * *").unwrap();
        let first = c.next_after(at("2026-11-01T00:00:00-04:00"), &z).unwrap();
        assert_eq!(first.to_string(), "2026-11-01T05:30:00Z");

        let next = c.next_after(first, &z).unwrap();
        assert_eq!(
            local(next, &z),
            "2026-11-02 01:30 EST",
            "the repeated hour must not fire a second time the same morning"
        );
    }

    /// Without a bound this is an infinite loop inside a server's startup check.
    #[test]
    fn an_expression_that_never_occurs_returns_none_rather_than_hanging() {
        let z = tz("UTC");
        let c = Cron::parse("0 0 30 2 *").unwrap(); // the 30th of February
        assert_eq!(c.next_after(at("2026-08-06T00:00:00Z"), &z), None);
    }

    /// The parsed sets must agree; the `source` field deliberately does not, because it
    /// keeps what the operator wrote so a report can show that rather than a rendering.
    #[test]
    fn shorthands_and_names_parse() {
        let daily = Cron::parse("@daily").unwrap();
        let spelled = Cron::parse("0 0 * * *").unwrap();
        assert_eq!(daily.hours, spelled.hours);
        assert_eq!(daily.minutes, spelled.minutes);
        assert_eq!(daily.days_of_month, spelled.days_of_month);
        assert_eq!(
            daily.as_str(),
            "@daily",
            "the operator's own words are kept"
        );
        assert_eq!(
            Cron::parse("0 0 1 jan *").unwrap().months,
            Cron::parse("0 0 1 1 *").unwrap().months
        );
        assert_eq!(
            Cron::parse("0 0 * * sun").unwrap().days_of_week,
            Cron::parse("0 0 * * 0").unwrap().days_of_week
        );
    }

    /// Every crontab in the world writes Sunday as 0 or 7 interchangeably.
    #[test]
    fn seven_is_sunday() {
        assert_eq!(Cron::parse("0 0 * * 7").unwrap().days_of_week, vec![0]);
        assert_eq!(Cron::parse("0 0 * * 0,7").unwrap().days_of_week, vec![0]);
    }

    #[test]
    fn ranges_lists_and_steps() {
        assert_eq!(Cron::parse("0 9-11 * * *").unwrap().hours, vec![9, 10, 11]);
        assert_eq!(Cron::parse("0 3,15 * * *").unwrap().hours, vec![3, 15]);
        assert_eq!(
            Cron::parse("*/15 * * * *").unwrap().minutes,
            vec![0, 15, 30, 45]
        );
        assert_eq!(
            Cron::parse("0 0-23/6 * * *").unwrap().hours,
            vec![0, 6, 12, 18]
        );
    }

    /// The error has to name the field. "invalid cron expression" sends an operator
    /// counting spaces.
    #[test]
    fn a_bad_field_says_which_field() {
        let e = Cron::parse("0 99 * * *").unwrap_err();
        assert!(e.reason.contains("hour"), "{e}");
        assert!(e.reason.contains("0–23"), "{e}");

        let e = Cron::parse("0 3 * *").unwrap_err();
        assert!(e.reason.contains("5 fields"), "{e}");

        let e = Cron::parse("@fortnightly").unwrap_err();
        assert!(e.reason.contains("@daily"), "{e}");
    }

    #[test]
    fn a_backwards_range_is_refused_rather_than_matching_nothing() {
        let e = Cron::parse("0 17-9 * * *").unwrap_err();
        assert!(e.reason.contains("backwards"), "{e}");
    }

    /// The shortest gap, not the average: catch-up should err towards firing.
    #[test]
    fn the_shortest_interval_is_the_shortest_gap_not_the_mean() {
        let z = tz("UTC");
        // Monday and Tuesday: gaps of one day, then six.
        let c = Cron::parse("0 3 * * 1,2").unwrap();
        let span = c.shortest_interval(at("2026-08-06T00:00:00Z"), &z).unwrap();
        assert_eq!(span.total(Unit::Hour).unwrap().round(), 24.0);
    }

    #[test]
    fn jitter_is_bounded_stable_and_differs_between_installs() {
        let a = jitter_offset("node-a", "tampa-daily", 300);
        assert!(a < 300);
        assert_eq!(a, jitter_offset("node-a", "tampa-daily", 300), "not stable");
        assert_ne!(
            a,
            jitter_offset("node-b", "tampa-daily", 300),
            "two installs must not share a fire time"
        );
        assert_eq!(
            jitter_offset("node-a", "x", 0),
            0,
            "a zero span must not divide by zero"
        );
    }

    /// The next fire is strictly after the instant asked about, or a schedule fires twice
    /// the moment something asks "what is next" during its own run.
    #[test]
    fn the_next_fire_is_strictly_after_the_instant_given() {
        let z = tz("UTC");
        let c = Cron::parse("0 3 * * *").unwrap();
        let exactly_three = at("2026-08-06T03:00:00Z");
        assert_eq!(
            local(c.next_after(exactly_three, &z).unwrap(), &z),
            "2026-08-07 03:00 UTC"
        );
    }
}
