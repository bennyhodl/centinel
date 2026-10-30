//! iCalendar (RFC 5545): a calendar file, read as a feed of events.
//!
//! Line-based, and read here rather than through a crate because what is asked of it is
//! small — find each `VEVENT`, keep its bytes, read a dozen properties — and the one thing
//! that has to be exactly right is the slice: an event is stored as the lines the
//! calendar served, `BEGIN:VEVENT` to `END:VEVENT`, and nothing reformatted.

use super::{Entries, Entry, Format, Readable, body_from, capped, fragment_for};

pub(super) fn entries(bytes: &[u8]) -> anyhow::Result<Entries> {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let text = std::str::from_utf8(bytes)?;
    let mut out: Vec<Entry> = Vec::new();

    // Component nesting is tracked so an alarm's `END:VALARM` inside an event does not end
    // it. A folded line always begins with whitespace, so a `BEGIN:` at the start of a line
    // is always a real one.
    let mut start: Option<usize> = None;
    let mut depth = 0usize;
    let mut pos = 0usize;
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\r', '\n']);
        match start {
            None if bare.eq_ignore_ascii_case("BEGIN:VEVENT") => {
                start = Some(pos);
                depth = 0;
            }
            None => {}
            Some(from) => {
                let upper = bare.to_ascii_uppercase();
                if upper.starts_with("BEGIN:") {
                    depth += 1;
                } else if upper == "END:VEVENT" && depth == 0 {
                    let served = &text.as_bytes()[from..pos + line.len()];
                    push(&mut out, served);
                    start = None;
                } else if upper.starts_with("END:") {
                    depth = depth.saturating_sub(1);
                }
            }
        }
        pos += line.len();
    }
    anyhow::ensure!(
        text.trim_start()
            .to_ascii_uppercase()
            .starts_with("BEGIN:VCALENDAR"),
        "not an iCalendar file"
    );

    let calendar = properties(text);
    let (entries, dropped) = capped(out);
    Ok(Entries {
        format: Format::Calendar,
        title: value(&calendar, "X-WR-CALNAME"),
        entries,
        dropped,
    })
}

fn push(out: &mut Vec<Entry>, served: &[u8]) {
    let props = properties(&String::from_utf8_lossy(served));
    // A moved or edited instance of a recurring event shares its series' UID and is told
    // apart by `RECURRENCE-ID`. Both are the identity.
    let id = value(&props, "UID").map(|uid| match raw(&props, "RECURRENCE-ID") {
        Some(rid) => format!("{uid};{rid}"),
        None => uid,
    });
    let fragment = fragment_for(id.as_deref(), served);
    if out.iter().any(|e| e.fragment == fragment) {
        return;
    }
    out.push(Entry {
        fragment,
        bytes: served.to_vec(),
        link: value(&props, "URL"),
        title: value(&props, "SUMMARY"),
    });
}

pub(super) fn read(bytes: &[u8]) -> anyhow::Result<Readable> {
    let text = decode(bytes);
    let props = properties(&text);
    let head = text.trim_start().to_ascii_uppercase();

    if head.starts_with("BEGIN:VCALENDAR") {
        // The calendar's own text is its name and its size. Each event is its own
        // artifact, and listing nine hundred of them here would put every one in the
        // index twice.
        let events = entries(bytes)?;
        let mut fields = vec![("Events", events.entries.len().to_string())];
        if let Some(tz) = value(&props, "X-WR-TIMEZONE") {
            fields.push(("Time zone", tz));
        }
        return Ok(Readable {
            title: events.title,
            fields,
            body: value(&props, "X-WR-CALDESC").map(body_from),
            listed: Vec::new(),
        });
    }

    anyhow::ensure!(head.starts_with("BEGIN:VEVENT"), "not an iCalendar event");
    let mut fields = Vec::new();
    if let Some(start) = find(&props, "DTSTART") {
        let mut when = time(start);
        if let Some(end) = find(&props, "DTEND") {
            when.push_str(&format!(" – {}", time(end)));
        }
        fields.push(("When", when));
    }
    if let Some(place) = value(&props, "LOCATION") {
        fields.push(("Where", place));
    }
    if let Some(rule) = raw(&props, "RRULE") {
        fields.push(("Repeats", rule));
    }
    if let Some(status) = value(&props, "STATUS").filter(|s| !s.eq_ignore_ascii_case("CONFIRMED")) {
        fields.push(("Status", status));
    }
    if let Some(link) = value(&props, "URL") {
        fields.push(("Link", link));
    }
    Ok(Readable {
        title: value(&props, "SUMMARY"),
        fields,
        body: value(&props, "DESCRIPTION").map(body_from),
        listed: Vec::new(),
    })
}

/// One content line: `NAME;PARAM=x:value`.
#[derive(Debug)]
struct Prop {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

/// The properties of the outermost component, unfolded — an event's own, and not its
/// alarm's `DESCRIPTION`.
fn properties(text: &str) -> Vec<Prop> {
    let unfolded = text
        .replace("\r\n ", "")
        .replace("\r\n\t", "")
        .replace("\n ", "")
        .replace("\n\t", "");
    let mut out = Vec::new();
    let mut depth = 0usize;
    for line in unfolded.lines() {
        let line = line.trim_end_matches('\r');
        let upper = line.to_ascii_uppercase();
        if upper.starts_with("BEGIN:") {
            depth += 1;
            continue;
        }
        if upper.starts_with("END:") {
            depth = depth.saturating_sub(1);
            continue;
        }
        if depth == 1
            && let Some(prop) = parse_line(line)
        {
            out.push(prop);
        }
    }
    out
}

/// Splits a content line at its first `:` outside a quoted parameter value.
fn parse_line(line: &str) -> Option<Prop> {
    let mut quoted = false;
    let colon = line.char_indices().find_map(|(i, c)| match c {
        '"' => {
            quoted = !quoted;
            None
        }
        ':' if !quoted => Some(i),
        _ => None,
    })?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let mut parts = head.split(';');
    let name = parts.next()?.trim().to_ascii_uppercase();
    let params = parts
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| {
            (
                k.trim().to_ascii_uppercase(),
                v.trim_matches('"').to_string(),
            )
        })
        .collect();
    Some(Prop {
        name,
        params,
        value: value.to_string(),
    })
}

fn find<'p>(props: &'p [Prop], name: &str) -> Option<&'p Prop> {
    props.iter().find(|p| p.name == name)
}

/// A property's value as written.
fn raw(props: &[Prop], name: &str) -> Option<String> {
    find(props, name)
        .map(|p| p.value.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// A text property's value, unescaped: `\n` is a line break and `\,` a comma.
fn value(props: &[Prop], name: &str) -> Option<String> {
    let raw = raw(props, name)?;
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            },
            c => out.push(c),
        }
    }
    let out = out.trim().to_string();
    (!out.is_empty()).then_some(out)
}

/// A date or date-time as a person reads it, with the zone it was written in.
///
/// `20261101T190000Z` → `2026-11-01 19:00 UTC`; with `TZID=America/New_York` →
/// `2026-11-01 19:00 America/New_York`; a whole day → `2026-11-01`. Anything else is
/// shown as written rather than guessed at.
fn time(prop: &Prop) -> String {
    let v = prop.value.trim();
    let b = v.as_bytes();
    let digits =
        |r: std::ops::Range<usize>| b.get(r).is_some_and(|s| s.iter().all(u8::is_ascii_digit));
    if !digits(0..8) {
        return v.to_string();
    }
    let date = format!("{}-{}-{}", &v[0..4], &v[4..6], &v[6..8]);
    if v.len() == 8 {
        return date;
    }
    if b.get(8) != Some(&b'T') || !digits(9..13) {
        return v.to_string();
    }
    let clock = format!("{date} {}:{}", &v[9..11], &v[11..13]);
    match (
        v.ends_with('Z'),
        prop.params.iter().find(|(k, _)| k == "TZID"),
    ) {
        (true, _) => format!("{clock} UTC"),
        (false, Some((_, tz))) => format!("{clock} {tz}"),
        (false, None) => clock,
    }
}

fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Google Calendar's shape: CRLF, folded lines, escaped commas, an alarm inside an
    /// event, and a moved instance of a recurring series.
    const GOOGLE: &str = "BEGIN:VCALENDAR\r\nPRODID:-//Google Inc//Google Calendar 70.9054//EN\r\nVERSION:2.0\r\nX-WR-CALNAME:Pinellas DSA\r\nX-WR-TIMEZONE:America/New_York\r\nBEGIN:VEVENT\r\nDTSTART;TZID=America/New_York:20261105T190000\r\nDTEND;TZID=America/New_York:20261105T203000\r\nRRULE:FREQ=MONTHLY;BYDAY=1TH\r\nUID:gbm@google.com\r\nSUMMARY:General Body Meeting\r\nLOCATION:St. Petersburg Main Library\\, 3745 9th Ave N\r\nDESCRIPTION:Monthly meeting.\\nAgenda: rent stabilization and the bus fa\r\n re vote.\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nDESCRIPTION:This is an event reminder\r\nEND:VALARM\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nDTSTART;TZID=America/New_York:20261210T190000\r\nRECURRENCE-ID;TZID=America/New_York:20261203T190000\r\nUID:gbm@google.com\r\nSUMMARY:General Body Meeting (moved)\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn each_event_is_the_lines_the_calendar_served() {
        let got = entries(GOOGLE.as_bytes()).unwrap();
        assert_eq!(got.format, Format::Calendar);
        assert_eq!(got.title.as_deref(), Some("Pinellas DSA"));
        assert_eq!(got.entries.len(), 2);

        let first = std::str::from_utf8(&got.entries[0].bytes).unwrap();
        assert!(first.starts_with("BEGIN:VEVENT\r\n") && first.ends_with("END:VEVENT\r\n"));
        assert!(
            GOOGLE.contains(first),
            "a slice of the response, not a rewrite"
        );
        assert!(
            first.contains("END:VALARM"),
            "the alarm did not end the event"
        );
    }

    /// A moved instance shares its series' UID, and is a different entry.
    #[test]
    fn a_moved_instance_is_its_own_entry() {
        let got = entries(GOOGLE.as_bytes()).unwrap();
        assert_eq!(got.entries[0].fragment, "entry=gbm@google.com");
        assert_eq!(
            got.entries[1].fragment,
            "entry=gbm@google.com;20261203T190000"
        );
    }

    #[test]
    fn an_event_reads_as_what_when_and_where() {
        let got = entries(GOOGLE.as_bytes()).unwrap();
        let read = read(&got.entries[0].bytes).unwrap();
        assert_eq!(read.title.as_deref(), Some("General Body Meeting"));
        assert!(read.fields.contains(&(
            "When",
            "2026-11-05 19:00 America/New_York – 2026-11-05 20:30 America/New_York".into()
        )));
        assert!(read.fields.contains(&(
            "Where",
            "St. Petersburg Main Library, 3745 9th Ave N".into()
        )));
        assert_eq!(
            read.body,
            Some(crate::feed::Body::Text(
                "Monthly meeting.\nAgenda: rent stabilization and the bus fare vote.".into()
            )),
            "unfolded, unescaped, and not the alarm's description"
        );
    }
}
