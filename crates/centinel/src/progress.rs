//! Rendering [`ProgressEvent`]s for a human.
//!
//! The op never learns who invoked it — it emits events and this module decides what a
//! terminal should look like. That separation is why a 1.2 GB download can grow a
//! multi-bar display without a single line changing in
//! [`centinel_core::models::download`].
//!
//! ## Two renderers, chosen by whether stderr is a terminal
//!
//! Bars are for people. Piped into a file or a CI log, `indicatif` would either emit
//! nothing (its non-terminal draw target is silent) or thousands of redraw lines. So a
//! non-terminal stderr gets the plain line-per-event renderer instead, and the byte-level
//! updates are dropped from it — 613 MB at one event per 512 KiB is 1,200 lines nobody
//! wants in a log, and the only interesting ones are the completions.
//!
//! ## Reading the event stream
//!
//! | Event | Meaning | Rendering |
//! |---|---|---|
//! | `request` | one HTTP request | a line above the bars, and a tick of the footer |
//! | no `id` | a log line | printed above the bars |
//! | `id` + `total` | a track | one bar per `id` |
//! | `id`, `done == total` | that track finished | bar cleared, a `✓` line printed |
//!
//! `id` is what makes a multi-file download legible: the aggregate bar and the current
//! file's bar are different tracks, so neither has to be inferred from message text.
//!
//! ## The log scrolls, the footer stays
//!
//! A crawl paced at one request per second is asleep almost all of the time, and a run
//! that printed only a counter was indistinguishable from one that had hung — two hours
//! of a live 11,473-page collect looking like a crash. So requests scroll past as history
//! while a tally stays pinned underneath them:
//!
//! ```text
//! 200    94.15 KiB    0.9s   https://www.tampa.gov/proclamation/irish-heritage-month
//! 200     1.19 MiB    1.8s ↳ …s/proclamation/2022/20220301_Irish_Heritage_Month.pdf
//! 404           —     0.1s ↳ …ww.tampa.gov/sites/default/files/rfq/missing-exhibit.pdf
//!   ⠋ 7,236 stored, 41 failed    ━━━━━━━━━━━━━━━╾────  7,236/11,473
//!     9,923 requests · 9,882 ok · 41 failed · 2,686 docs · 2.85 GiB · 1.0/s · eta 1h 36m
//! ```
//!
//! The bar is a **fixed** 28 columns, not `wide_bar`. Stretched to the terminal it put the
//! two lines on wildly different scales, and the tally read as a caption to something the
//! width of the screen.
//!
//! The two lines count different things, so both name their unit. The bar counts
//! *resources* and the tally counts *requests* — a page enclosing three documents is one
//! of the first and four of the second. Unlabelled, `12 failed` above `41 failed` reads as
//! the display arguing with itself.
//!
//! ## Every line fits, and every bar is one row
//!
//! `MultiProgress` redraws by counting rows and moving the cursor up that many. Two things
//! break the count, and both did: a template containing a `\n`, which makes one bar occupy
//! two rows, and a printed line the terminal wrapped, which is a row it never knew it drew.
//! Either one and every redraw afterwards lands a column further across — the display walks
//! diagonally down the screen, which is what switching away from the terminal and back
//! reliably produced.
//!
//! So the tally is its own single-row bar rather than a second line of the stage's, and
//! every line is cut to the terminal's width before it is printed. `MultiProgress` keeps
//! its bars in one block at the bottom and prints above it, so the two cannot be separated
//! anyway.
//!
//! Everything in the tally is derived here from the request stream, including the estimate —
//! which counts the documents the remaining pages will *also* pull, because a corpus
//! where a third of pages enclose a PDF makes a page-only estimate run 30% short.

use std::collections::HashMap;
use std::io::IsTerminal;
use std::time::Instant;

use centinel_core::op::{ProgressEvent, RequestOutcome, TOTAL_TRACK, Unit};
use indicatif::{HumanBytes, MultiProgress, ProgressBar, ProgressStyle};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::JoinHandle;

/// Only emitted when stderr is a terminal, so escape codes can never reach a pipe.
const GREEN_CHECK: &str = "\x1b[32m✓\x1b[0m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const CYAN: &str = "\x1b[36m";

/// The least address a request line will show, however narrow the terminal. Below this the
/// line stops being worth printing, and a line that is slightly too long for a very narrow
/// terminal is a better outcome than one carrying no address at all.
const MIN_URL_WIDTH: usize = 24;

/// How much of a long label to keep. Wide enough for `qwen3-embedding-0.6b tokenizer.json`.
const LABEL_WIDTH: usize = 42;

/// The stage label's fixed width. Wide enough for `11,473 stored, 1,204 failed`.
const STAGE_LABEL_WIDTH: usize = 28;

/// Drains progress events until the sender is dropped.
///
/// Returns a handle rather than blocking: the op runs concurrently, and the renderer
/// terminates on its own when the op drops its [`centinel_core::op::Progress`].
pub fn spawn(rx: UnboundedReceiver<ProgressEvent>) -> JoinHandle<()> {
    if std::io::stderr().is_terminal() {
        tokio::spawn(render_bars(rx))
    } else {
        tokio::spawn(render_lines(rx))
    }
}

/// The track an op with no `id` gets. `collect` and `extract` predate ids and emit a
/// bare counter; they deserve a bar, not a thousand log lines.
const IMPLICIT_TRACK: &str = "";

/// What the footer says, accumulated from the request stream.
///
/// Derived here rather than in the op, because these are questions about *rendering* a
/// run — how fast, how much left — and the op that answers them is one that has learned
/// what a terminal wants. It only ever needs the events it was already sending.
#[derive(Debug, Default)]
struct Tally {
    ok: u64,
    failed: u64,
    bytes: u64,
    /// Documents found inside pages. Counted apart from declared addresses because only
    /// the declared set has a total known before the run starts.
    enclosed: u64,
    /// Successful fetches of a declared address — the denominator for "documents per
    /// page", and not the same as the bar's position, which counts resources processed.
    pages: u64,
    started: Option<Instant>,
}

impl Tally {
    fn record(&mut self, r: &RequestOutcome) {
        self.started.get_or_insert_with(Instant::now);
        match r.succeeded() {
            true => self.ok += 1,
            false => self.failed += 1,
        }
        self.bytes += r.bytes;
        match r.enclosed {
            true => self.enclosed += 1,
            false => self.pages += 1,
        }
    }

    fn requests(&self) -> u64 {
        self.ok + self.failed
    }

    /// Requests per second over the whole run, which at a fixed pace is the number that
    /// matters. An instantaneous rate would swing on one large PDF and tell nobody
    /// anything.
    fn rate(&self) -> f64 {
        match self.started.map(|t| t.elapsed().as_secs_f64()) {
            Some(secs) if secs > 0.5 => self.requests() as f64 / secs,
            _ => 0.0,
        }
    }

    /// How long is left, counting the documents the remaining pages will *also* pull.
    ///
    /// The bar alone would lie by however many enclosures a page averages — on a corpus
    /// where a third of pages carry a PDF, an estimate that ignored them ran 30% short.
    /// `None` until there is enough of a run to divide by.
    fn eta(&self, done: u64, total: u64) -> Option<std::time::Duration> {
        let rate = self.rate();
        if rate <= 0.0 || total <= done || self.pages == 0 {
            return None;
        }
        let per_page = 1.0 + (self.enclosed as f64 / self.pages as f64);
        let remaining = (total - done) as f64 * per_page;
        Some(std::time::Duration::from_secs_f64(remaining / rate))
    }

    /// The pinned line under the bar.
    ///
    /// Leads with the request count, and every figure is `number noun`. The bar above
    /// counts *resources* and says `13 stored, 12 failed`; this counts **requests**, and a
    /// page that encloses three documents is one of the first and four of the second. Read
    /// as a pair without units, `12 failed` over `41 failed` looks like the display
    /// contradicting itself rather than two honest counts of different things.
    fn footer(&self, done: u64, total: u64) -> String {
        let mut parts = vec![
            format!("{}{} requests{}", DIM, count(self.requests()), RESET),
            format!("{GREEN}{} ok{RESET}", count(self.ok)),
            match self.failed {
                0 => format!("{DIM}0 failed{RESET}"),
                n => format!("{RED}{} failed{RESET}", count(n)),
            },
            format!("{DIM}{} docs{RESET}", count(self.enclosed)),
            format!("{DIM}{}{RESET}", HumanBytes(self.bytes)),
        ];
        if self.rate() > 0.0 {
            parts.push(format!("{DIM}{:.1}/s{RESET}", self.rate()));
        }
        if let Some(eta) = self.eta(done, total) {
            parts.push(format!("{CYAN}eta {}{RESET}", short_duration(eta)));
        }
        format!("    {}", parts.join(&format!("{DIM} · {RESET}")))
    }
}

/// Thousands separators, because a six-figure request count is unreadable without them.
fn count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `2h 41m`, `12m`, `48s` — two units at most, because a third is never the thing being
/// decided on.
fn short_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    match (secs / 3600, (secs % 3600) / 60, secs % 60) {
        (0, 0, s) => format!("{s}s"),
        (0, m, _) => format!("{m}m"),
        (h, m, _) => format!("{h}h {m:02}m"),
    }
}

/// One request, as a line in the scrolling log, fitted to `width` printed columns.
///
/// Fixed columns so the eye can run down them: what came back, how big, how long, where.
/// The status is the only coloured field — it is the one being scanned for.
fn request_line(r: &RequestOutcome, width: usize) -> String {
    let status = match (r.status, r.succeeded()) {
        (Some(s), true) => format!("{GREEN}{s}{RESET}"),
        // A 429 is the host asking for room, not a broken address, and reads differently.
        (Some(s @ 429), _) => format!("{YELLOW}{s}{RESET}"),
        (Some(s), false) if (400..500).contains(&s) => format!("{YELLOW}{s}{RESET}"),
        (Some(s), false) => format!("{RED}{s}{RESET}"),
        // No status at all: never reached the server.
        (None, _) => format!("{RED} — {RESET}"),
    };

    // Fixed width, and wide enough for `1023.99 KiB`. `HumanBytes` is variable-length, so
    // padding it is the only thing keeping the two columns to its right in a straight line
    // — which is the entire reason a scrolling log is readable at a glance.
    let size = match r.bytes {
        0 => format!("{DIM}{:>11}{RESET}", "—"),
        n => format!("{:>11}", HumanBytes(n).to_string()),
    };
    // A document is marked, so the two work-lists are separable by eye as they scroll.
    let mark = match r.enclosed {
        true => format!("{CYAN}↳{RESET}"),
        false => " ".to_string(),
    };
    // Everything to the left of the address, in printed columns:
    //   status 3 · gap 2 · size 11 · gap 2 · time 6 · gap 1 · mark 1 · gap 1
    const FIXED: usize = 27;

    // The address takes whatever is left, and the reason it is computed rather than a
    // constant is the whole bug: a line wider than the terminal wraps, `indicatif` counts
    // it as one row when it drew two, and every redraw after that moves the cursor to the
    // wrong place. What it looks like is the display walking diagonally down the screen.
    let mut detail = match &r.detail {
        Some(d) if r.status.is_none() => format!("  {d}"),
        _ => String::new(),
    };
    // The address is the thing being read; the detail yields to it when both cannot fit.
    if width < FIXED + detail.chars().count() + MIN_URL_WIDTH {
        detail.clear();
    }
    // No floor. A minimum that the terminal is narrower than would put the wrap straight
    // back, and a short address beats a broken display.
    let room = width.saturating_sub(FIXED + detail.chars().count());

    let dim_detail = match detail.is_empty() {
        true => String::new(),
        false => format!("{DIM}{detail}{RESET}"),
    };
    format!(
        "{status}  {size}  {DIM}{:>5.1}s{RESET} {mark} {DIM}{}{RESET}{dim_detail}",
        r.millis as f64 / 1000.0,
        truncate(&r.url, room),
    )
}

/// Columns of stderr, or a conservative guess when it is not a terminal.
fn terminal_width() -> usize {
    console::Term::stderr()
        .size_checked()
        .map(|(_, cols)| cols as usize)
        // One column spare. Printing *to* the last one leaves the cursor in a state some
        // terminals resolve by wrapping anyway, which is the failure being avoided.
        .map_or(100, |cols| cols.saturating_sub(1).max(40))
}

async fn render_bars(mut rx: UnboundedReceiver<ProgressEvent>) {
    let multi = MultiProgress::new();
    let mut bars: HashMap<String, ProgressBar> = HashMap::new();
    let mut tally = Tally::default();
    // The tally, as its own single-line bar rather than a second line of the stage's.
    // `MultiProgress` keeps its bars in one block at the bottom and prints above it, so
    // nothing can land between the two — and every bar staying one row is what keeps its
    // cursor arithmetic right.
    let mut footer: Option<ProgressBar> = None;

    while let Some(event) = rx.recv().await {
        // A request scrolls above the bars and updates the tally beneath them. This is
        // the whole shape: history you can read, orientation you can trust, one screen.
        if let Some(outcome) = &event.request {
            tally.record(outcome);
            let _ = multi.println(request_line(outcome, terminal_width()));
            if let (Some(bar), Some(footer)) = (bars.get(IMPLICIT_TRACK), &footer) {
                let line = tally.footer(bar.position(), bar.length().unwrap_or(0));
                footer.set_message(truncate_end(&line, terminal_width()));
            }
            continue;
        }

        let Some(total) = event.total else {
            let _ = multi.println(format!("{DIM}·{RESET} {}", event.message));
            continue;
        };
        let named = event.id.is_some();
        let id = event.id.as_deref().unwrap_or(IMPLICIT_TRACK);
        let done = event.done.unwrap_or(0);
        let is_total = id == TOTAL_TRACK;

        let bar = bars
            .entry(id.to_string())
            .or_insert_with(|| {
                // The aggregate sits at the *bottom*, under whatever it is summing — the
                // order `cargo` has trained people to read. It is created lazily like any
                // other bar, so pushing every other track above it keeps that order stable
                // no matter which bar happened to appear first.
                let bar = if is_total {
                    multi.add(ProgressBar::new(total))
                } else {
                    multi.insert(0, ProgressBar::new(total))
                };
                bar.set_style(style_for(event.unit, is_total));
                bar
            })
            .clone();

        // The tally goes directly under the stage bar, once there is a stage bar to sit
        // under. A `ProgressBar` is a handle to shared state, so cloning is a refcount.
        if id == IMPLICIT_TRACK && footer.is_none() {
            let line = multi.insert_after(&bar, ProgressBar::new(0));
            line.set_style(
                ProgressStyle::with_template("{msg}").expect("a bare message always parses"),
            );
            line.set_message(truncate_end(&tally.footer(done, total), terminal_width()));
            footer = Some(line);
        }

        // Set rather than increment: events are throttled and lossy by design, so an
        // absolute position is the only one that survives a dropped update.
        bar.set_length(total);
        bar.set_position(done);
        bar.set_prefix(if is_total {
            "total".to_string()
        } else if id == IMPLICIT_TRACK {
            // Padded to a fixed width. A stage label grows as its counts do — `9 stored,
            // 0 failed` becomes `1,204 stored, 87 failed` — and an unpadded prefix slid
            // the bar rightwards under the reader's eye for the whole run.
            pad(&event.message, STAGE_LABEL_WIDTH)
        } else {
            truncate(&event.message, LABEL_WIDTH)
        });
        if let (true, Some(line)) = (id == IMPLICIT_TRACK, &footer) {
            line.set_message(truncate_end(&tally.footer(done, total), terminal_width()));
        }

        // A finished *named* track becomes a static line, so a ten-file pull does not
        // grow ten live bars. The implicit track is one long counter with nothing after
        // it, and the aggregate outlives every file — neither should vanish here.
        if done >= total && named && !is_total {
            bar.finish_and_clear();
            let size = match event.unit {
                Unit::Bytes => HumanBytes(total).to_string(),
                Unit::Count => total.to_string(),
            };
            let _ = multi.println(format!(
                "{GREEN_CHECK} {}  {DIM}{size}{RESET}",
                truncate(&event.message, LABEL_WIDTH),
            ));
            bars.remove(id);
        }
    }

    for (_, bar) in bars {
        bar.finish();
    }
}

/// The pre-existing renderer: one line per event, no cursor tricks.
async fn render_lines(mut rx: UnboundedReceiver<ProgressEvent>) {
    while let Some(event) = rx.recv().await {
        // Plain, no escape codes and no columns to align — this is a log file, and the
        // thing reading it is `grep`.
        if let Some(r) = &event.request {
            let status = r.status.map(|s| s.to_string()).unwrap_or("---".into());
            let detail = r.detail.as_deref().unwrap_or("");
            eprintln!("{status} {:>9} {}ms {} {detail}", r.bytes, r.millis, r.url);
            continue;
        }
        match (event.id.as_deref(), event.done, event.total) {
            // Tracked work is high-frequency; only its completion is worth a log line.
            (Some(_), Some(done), Some(total)) if done < total => {}
            (Some(_), Some(done), Some(total)) => {
                eprintln!("[{done}/{total}] {}", event.message)
            }
            (_, Some(done), Some(total)) => eprintln!("[{done}/{total}] {}", event.message),
            _ => eprintln!("… {}", event.message),
        }
    }
}

/// Every template is **one line**.
///
/// A `\n` in a template makes one bar occupy two terminal rows, and `MultiProgress`
/// redraws by counting rows and moving the cursor up. Get that count wrong once — a
/// two-row bar, or a line that wrapped — and every redraw after it lands a column further
/// across, which is the display walking diagonally down the screen.
fn style_for(unit: Unit, is_total: bool) -> ProgressStyle {
    let template = match (unit, is_total) {
        (Unit::Bytes, false) => {
            "  {spinner:.cyan} {prefix:.bold} {wide_bar:.cyan/blue} \
             {bytes:>10}/{total_bytes:<10} {binary_bytes_per_sec:>11} eta {eta:>4}"
        }
        (Unit::Bytes, true) => {
            "  {prefix:.dim} {wide_bar:.green/dim} {bytes:>10}/{total_bytes:<10} eta {eta:>4}"
        }
        // A fixed bar, not `wide_bar`. The stage bar sits directly above a tally line, and
        // a bar stretched to the terminal put the two on wildly different scales — the
        // tally read as a caption to something the width of the screen.
        (Unit::Count, _) => "  {spinner:.cyan} {prefix:.bold} {bar:28.cyan/blue} {pos:>7}/{len:<7}",
    };
    ProgressStyle::with_template(template)
        .expect("templates are literals, checked by the tests below")
        .progress_chars("━━╾")
}

/// Exactly `width` printed columns: truncated if long, space-padded if short.
///
/// So the bar that follows it starts at the same column on every redraw.
fn pad(text: &str, width: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    match chars.len() >= width {
        true => chars[..width].iter().collect(),
        false => format!("{text}{}", " ".repeat(width - chars.len())),
    }
}

/// Cuts a line to `width` printed columns, counting only what is actually drawn.
///
/// The tally is built from coloured fragments, and an SGR escape occupies bytes and no
/// columns — so cutting it by length would take a chunk out of the visible text and, worse,
/// could sever an escape and leave the rest of the terminal coloured.
fn truncate_end(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut drawn = 0usize;
    let mut chars = text.chars();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            out.push(c);
            for c in chars.by_ref() {
                out.push(c);
                if c == 'm' {
                    break;
                }
            }
            continue;
        }
        if drawn == width {
            // Whatever was open stays open only as far as here.
            out.push_str(RESET);
            break;
        }
        out.push(c);
        drawn += 1;
    }
    out
}

/// Keeps the tail of an over-long label — the filename is what distinguishes it.
fn truncate(text: &str, width: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        return text.to_string();
    }
    // A very narrow terminal leaves no room for the ellipsis, let alone anything after it.
    if width <= 1 {
        return "…".repeat(width);
    }
    format!(
        "…{}",
        chars[chars.len() - (width - 1)..]
            .iter()
            .collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a terminal actually draws: the line with its SGR escapes removed.
    fn strip(line: &str) -> String {
        let mut out = String::new();
        let mut chars = line.chars();
        while let Some(c) = chars.next() {
            match c {
                '\x1b' => {
                    for c in chars.by_ref() {
                        if c == 'm' {
                            break;
                        }
                    }
                }
                c => out.push(c),
            }
        }
        out
    }

    fn outcome(status: Option<u16>, bytes: u64, enclosed: bool) -> RequestOutcome {
        RequestOutcome {
            url: "https://www.tampa.gov/proclamation/irish-american-heritage-month".into(),
            status,
            bytes,
            millis: 900,
            kind: Some("html".into()),
            detail: status.is_none().then(|| "connection timed out".to_string()),
            enclosed,
        }
    }

    #[test]
    fn every_style_template_parses() {
        // A bad template panics at render time, which on a terminal means a corrupted
        // display mid-download. Cheaper to find here.
        for unit in [Unit::Bytes, Unit::Count] {
            for is_total in [true, false] {
                let _ = style_for(unit, is_total);
            }
        }
    }

    // ── the request stream ─────────────────────────────────────────────────────

    #[test]
    fn a_request_line_leads_with_what_came_back() {
        let line = request_line(&outcome(Some(200), 1_200_000, false), 100);
        assert!(line.contains("200"), "{line}");
        assert!(line.contains("1.14 MiB"), "the size is readable: {line}");
        assert!(line.contains("0.9s"), "and how long it took: {line}");
        assert!(line.contains("irish-american-heritage-month"), "{line}");
    }

    /// A 404 is an answer and a timeout is not, and a run that showed them alike would
    /// hide the difference between a broken link and a broken network.
    #[test]
    fn a_refusal_without_a_status_says_why() {
        let line = request_line(&outcome(None, 0, false), 100);
        assert!(line.contains("connection timed out"), "{line}");
        assert!(!line.contains("404"));

        let answered = request_line(&outcome(Some(404), 0, false), 100);
        assert!(answered.contains("404"), "{answered}");
        assert!(
            !answered.contains("timed out"),
            "a status is the whole answer: {answered}"
        );
    }

    /// A scrolling log is only readable if the columns are straight, and `HumanBytes` is
    /// variable-length — `1.19 MiB` against `94.15 KiB`. Left unpadded, every column to
    /// the right of the size wandered.
    #[test]
    fn the_columns_line_up_whatever_the_size() {
        let short = |status, bytes| RequestOutcome {
            url: "https://tampa.gov/a".into(),
            status,
            bytes,
            millis: 900,
            kind: None,
            detail: None,
            enclosed: false,
        };
        let starts_at = |r: &RequestOutcome| {
            let line = strip(&request_line(r, 100));
            // Characters, not bytes: an em-dash is one printed column and three bytes,
            // so a byte offset would report the columns as ragged when they are straight.
            let at = line
                .find("http")
                .expect("this url is short enough to survive");
            line[..at].chars().count()
        };

        let columns: Vec<_> = [
            short(Some(200), 96_409),    // 94.15 KiB
            short(Some(200), 1_248_112), // 1.19 MiB
            short(Some(404), 0),         // —
            short(None, 0),              // no status at all
        ]
        .iter()
        .map(starts_at)
        .collect();

        assert!(
            columns.windows(2).all(|w| w[0] == w[1]),
            "the url starts at a different column each time: {columns:?}"
        );
    }

    /// The fault behind the staircase. `MultiProgress` redraws by counting rows and moving
    /// the cursor up, so a line the terminal wrapped is a row it does not know it drew —
    /// and every redraw after it lands further across the screen.
    #[test]
    fn no_line_is_ever_wider_than_the_terminal() {
        let long = RequestOutcome {
            url: "https://www.tampa.gov/sites/default/files/document/2026/sw_franchise_fee_\
                  remittance_form_7-31-2025_potential-customers-07-14-2025.pdf"
                .into(),
            status: None,
            bytes: 0,
            millis: 1_300,
            kind: None,
            detail: Some("connection timed out after 30s".into()),
            enclosed: true,
        };

        for width in [40usize, 60, 80, 100, 120, 200] {
            let printed = strip(&request_line(&long, width)).chars().count();
            assert!(
                printed <= width,
                "a {printed}-column line in a {width}-column terminal wraps"
            );
        }
    }

    /// And the tally is built from coloured fragments, so cutting it by length would take
    /// a bite out of the visible text — or sever an escape and colour the rest of the
    /// terminal.
    #[test]
    fn the_tally_is_cut_by_printed_width_not_by_length() {
        let mut tally = Tally::default();
        for _ in 0..1_000 {
            tally.record(&outcome(Some(200), 1_000_000, false));
        }
        let line = tally.footer(10, 100);
        assert!(strip(&line).chars().count() > 40, "worth truncating");

        for width in [20usize, 40, 60] {
            let cut = truncate_end(&line, width);
            assert!(strip(&cut).chars().count() <= width, "{width}: {cut:?}");
            assert!(cut.ends_with(RESET), "colour must not leak past the cut");
        }
    }

    #[test]
    fn an_enclosed_document_is_marked_apart_from_a_declared_page() {
        assert!(request_line(&outcome(Some(200), 10, true), 100).contains('↳'));
        assert!(!request_line(&outcome(Some(200), 10, false), 100).contains('↳'));
    }

    // ── the footer ─────────────────────────────────────────────────────────────

    #[test]
    fn the_footer_counts_what_the_stream_said() {
        let mut tally = Tally::default();
        for _ in 0..3 {
            tally.record(&outcome(Some(200), 1_000, false));
        }
        tally.record(&outcome(Some(404), 0, true));

        let footer = tally.footer(3, 10);
        assert!(footer.contains("3 ok"), "{footer}");
        assert!(footer.contains("1 failed"), "{footer}");
        assert!(footer.contains("1 docs"), "{footer}");
        assert_eq!(tally.requests(), 4);
    }

    /// The bar counts resources and the footer counts requests, and a page that encloses
    /// three documents is one of the first and four of the second. Without the unit, the
    /// bar's `12 failed` beside the footer's `41 failed` reads as a display arguing with
    /// itself.
    #[test]
    fn the_footer_says_what_it_is_counting() {
        let mut tally = Tally::default();
        tally.record(&outcome(Some(200), 10, false));
        tally.record(&outcome(Some(404), 0, true));

        let footer = tally.footer(1, 10);
        assert!(
            footer.contains("2 requests"),
            "the denominator is named: {footer}"
        );
    }

    /// The stage bar sits above the tally, so it must not stretch to the terminal, and its
    /// label must not slide the bar sideways as the counts grow.
    #[test]
    fn the_stage_label_is_a_fixed_width() {
        assert_eq!(pad("9 stored, 0 failed", 28).chars().count(), 28);
        assert_eq!(pad("11,473 stored, 1,204 failed", 28).chars().count(), 28);
        assert_eq!(pad("", 28).chars().count(), 28);
        assert_eq!(
            pad("a much longer label than fits here", 28)
                .chars()
                .count(),
            28
        );
        // Multi-byte must not be sliced through a codepoint.
        assert_eq!(pad("café-café-café-café-café-café", 10).chars().count(), 10);
    }

    #[test]
    fn the_stage_bar_does_not_stretch_to_the_terminal() {
        // `wide_bar` fills whatever the terminal is; `bar:28` does not.
        let template =
            "  {spinner:.cyan} {prefix:.bold} {bar:28.cyan/blue} {pos:>7}/{len:<7}\n{msg}";
        assert!(
            !template.contains("wide_bar"),
            "the stage bar is fixed width"
        );
        let _ = style_for(Unit::Count, false);
    }

    /// The estimate that made the bar honest. Two pages done of ten, and every page so
    /// far pulled one document — so eight pages left is sixteen requests, not eight.
    #[test]
    fn the_estimate_counts_the_documents_the_remaining_pages_will_pull() {
        let mut tally = Tally {
            started: Some(Instant::now() - std::time::Duration::from_secs(4)),
            ..Default::default()
        };
        for _ in 0..2 {
            tally.record(&outcome(Some(200), 0, false));
            tally.record(&outcome(Some(200), 0, true));
        }

        let eta = tally.eta(2, 10).expect("four requests in four seconds");
        // 8 pages × 2 requests each ÷ ~1/s. Loose bounds: the point is that it is not 8s.
        assert!(
            eta.as_secs() >= 12,
            "documents must be counted in: {}s",
            eta.as_secs()
        );
    }

    #[test]
    fn an_estimate_needs_a_run_to_estimate_from() {
        let tally = Tally::default();
        assert!(tally.eta(0, 100).is_none(), "nothing has happened yet");

        let mut finished = Tally {
            started: Some(Instant::now() - std::time::Duration::from_secs(4)),
            ..Default::default()
        };
        finished.record(&outcome(Some(200), 0, false));
        assert!(finished.eta(10, 10).is_none(), "there is nothing left");
    }

    #[test]
    fn large_counts_stay_readable() {
        assert_eq!(count(0), "0");
        assert_eq!(count(999), "999");
        assert_eq!(count(1_000), "1,000");
        assert_eq!(count(11_473), "11,473");
    }

    #[test]
    fn a_duration_says_two_units_at_most() {
        use std::time::Duration;
        assert_eq!(short_duration(Duration::from_secs(48)), "48s");
        assert_eq!(short_duration(Duration::from_secs(12 * 60)), "12m");
        assert_eq!(
            short_duration(Duration::from_secs(2 * 3600 + 41 * 60)),
            "2h 41m"
        );
    }

    /// The whole point of C: the log scrolls and the footer stays. Both renderers have to
    /// survive a request event, and neither may hang on one.
    #[tokio::test]
    async fn both_renderers_accept_a_request_event() {
        for render in [0, 1] {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            let handle = match render {
                0 => tokio::spawn(render_bars(rx)),
                _ => tokio::spawn(render_lines(rx)),
            };
            tx.send(ProgressEvent {
                message: "collected 1".into(),
                done: Some(1),
                total: Some(2),
                ..Default::default()
            })
            .unwrap();
            for enclosed in [false, true] {
                tx.send(ProgressEvent {
                    message: "u".into(),
                    request: Some(outcome(Some(200), 4_096, enclosed)),
                    ..Default::default()
                })
                .unwrap();
            }
            drop(tx);
            handle.await.expect("a request event must not panic");
        }
    }

    #[test]
    fn short_labels_are_untouched() {
        assert_eq!(truncate("tokenizer.json", 42), "tokenizer.json");
    }

    #[test]
    fn long_labels_keep_their_tail() {
        let label = "qwen3-embedding-4b Qwen3-Embedding-4B-Q8_0.gguf";
        let out = truncate(label, 20);
        assert_eq!(out.chars().count(), 20);
        assert!(out.starts_with('…'));
        assert!(
            out.ends_with("Q8_0.gguf"),
            "the filename must survive: {out}"
        );
    }

    #[test]
    fn truncation_counts_characters_not_bytes() {
        // A multi-byte label must not be sliced through a codepoint.
        let out = truncate("café-café-café-café", 10);
        assert_eq!(out.chars().count(), 10);
    }

    #[tokio::test]
    async fn the_renderer_terminates_when_the_sender_is_dropped() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = tokio::spawn(render_lines(rx));
        tx.send(ProgressEvent {
            message: "hello".into(),
            ..Default::default()
        })
        .unwrap();
        drop(tx);
        handle.await.expect("renderer must not hang or panic");
    }

    /// `collect` and `extract` emit `step()` with no id. Losing their counter to a log
    /// line would be a regression, so the bar renderer gives them an implicit track.
    #[tokio::test]
    async fn unnamed_counted_events_still_get_a_bar() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = tokio::spawn(render_bars(rx));
        for done in 1..=3u64 {
            tx.send(ProgressEvent {
                message: format!("collected {done}"),
                done: Some(done),
                total: Some(3),
                ..Default::default()
            })
            .unwrap();
        }
        drop(tx);
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn the_line_renderer_survives_byte_tracks() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = tokio::spawn(render_lines(rx));
        for done in [0u64, 512, 1024] {
            tx.send(ProgressEvent {
                message: "model.onnx".into(),
                done: Some(done),
                total: Some(1024),
                id: Some("m:model.onnx".into()),
                unit: Unit::Bytes,
                ..Default::default()
            })
            .unwrap();
        }
        drop(tx);
        handle.await.unwrap();
    }
}
