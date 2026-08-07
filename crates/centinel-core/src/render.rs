//! Rendering a report for a human at a terminal.
//!
//! Every op returns a serializable report. That report is exactly right for HTTP and for
//! MCP — a model reads JSON better than it reads a table — and exactly wrong for a person,
//! who gets forty lines of quoted keys where four lines of prose would do.
//!
//! So the CLI renders. This module is the vocabulary it renders with, and the [`Render`]
//! trait is where each report says how it reads.
//!
//! ## Presence is uniform, prose is not
//!
//! The same rule [`crate::op`] applies to descriptions applies here, one level further.
//! Every op reaches every surface, and every surface renders the *same* report — but a
//! terminal gets it in a terminal's idiom. `store_root` is a field an HTTP caller needs
//! and a person already knows; `action: "list"` is a discriminant on the wire and noise
//! under a command literally named `list`. Dropping them is not hiding information, it is
//! declining to repeat the question back as part of the answer.
//!
//! ## Every report must implement this
//!
//! There is no blanket structural fallback, so a new op will not compile until its report
//! says how it reads. That is deliberate, and it is the *opposite* of the central list
//! [`crate::op`] avoids: forgetting is impossible because the compiler asks, at the
//! definition site, in the one place that knows what the numbers mean.
//!
//! ## The vocabulary
//!
//! Deliberately small, and shared with the progress renderer so a command does not change
//! its visual language halfway through:
//!
//! | | Meaning |
//! |---|---|
//! | green `✓` | present, healthy, done |
//! | red `✗` | absent, failed, gone |
//! | yellow `!` | present but degraded — blocked, truncated, partial |
//! | dim `·` | a separator or an aside |
//!
//! No emoji, no box drawing. Columns are aligned with spaces, which survives being
//! copied out of a terminal into an issue.

use std::io::{self, Write};

use unicode_width::UnicodeWidthStr;

/// Terminal width to assume when nobody tells us. Wide enough for the tables here,
/// narrow enough not to wrap the common 100-column window.
pub const DEFAULT_WIDTH: usize = 100;

/// How a report reads for a person.
///
/// Implemented next to the report it renders, because the field that matters and the
/// field that is noise is a question only the code that built them can answer.
pub trait Render {
    fn render(&self, p: &mut Painter<'_>) -> io::Result<()>;
}

// ---------------------------------------------------------------------------------------
// Ink
// ---------------------------------------------------------------------------------------

/// A colour. Applied only when the destination is a terminal — see [`Painter::new`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Ink {
    #[default]
    Plain,
    /// De-emphasised: units, provenance, anything the eye should skip on the first pass.
    Dim,
    /// A heading or a figure that carries the answer.
    Bold,
    Green,
    Red,
    Yellow,
    Cyan,
    /// A label: dim, but structurally rather than decoratively.
    Label,
}

impl Ink {
    /// The escape pair this ink opens and closes with.
    ///
    /// Public and `const` so the progress display can build its own constants from this
    /// table rather than keeping a second copy of the same seven escapes — which it did,
    /// in another crate, where nothing would ever have caught them drifting.
    pub const fn codes(self) -> (&'static str, &'static str) {
        match self {
            Ink::Plain => ("", ""),
            Ink::Dim => ("\x1b[2m", "\x1b[0m"),
            Ink::Bold => ("\x1b[1m", "\x1b[0m"),
            Ink::Green => ("\x1b[32m", "\x1b[0m"),
            Ink::Red => ("\x1b[31m", "\x1b[0m"),
            Ink::Yellow => ("\x1b[33m", "\x1b[0m"),
            Ink::Cyan => ("\x1b[36m", "\x1b[0m"),
            Ink::Label => ("\x1b[2m", "\x1b[0m"),
        }
    }
}

/// The status glyphs. Three states, because "present but degraded" is a fact this
/// project refuses to collapse into either of the other two — a blocked page is not a
/// deleted page, and a truncated list is not an empty one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Ok,
    Bad,
    Warn,
    /// Occupies the same column without asserting anything.
    None,
}

impl Mark {
    /// `true` → [`Mark::Ok`], `false` → [`Mark::Bad`].
    pub fn from_ok(ok: bool) -> Self {
        if ok { Mark::Ok } else { Mark::Bad }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Mark::Ok => "✓",
            Mark::Bad => "✗",
            Mark::Warn => "!",
            Mark::None => " ",
        }
    }

    pub fn ink(self) -> Ink {
        match self {
            Mark::Ok => Ink::Green,
            Mark::Bad => Ink::Red,
            Mark::Warn => Ink::Yellow,
            Mark::None => Ink::Plain,
        }
    }

    /// `✓` and its colour as one painted span — the roll-up idiom, where a glyph appears
    /// inline rather than in a column of its own.
    pub fn painted(self, p: &Painter<'_>) -> String {
        p.paint(self.glyph(), self.ink())
    }
}

// ---------------------------------------------------------------------------------------
// Painter
// ---------------------------------------------------------------------------------------

/// Where a report writes itself.
///
/// Holds the indent so a renderer can nest without threading a width parameter through
/// every call, and holds the colour decision so no renderer has to ask whether it is
/// talking to a terminal.
pub struct Painter<'w> {
    out: &'w mut dyn Write,
    color: bool,
    width: usize,
    indent: usize,
}

impl<'w> Painter<'w> {
    /// `color` should be false for a pipe, a file, or when `NO_COLOR` is set. Deciding it
    /// here rather than per-call is what keeps escape codes out of a redirected stdout.
    pub fn new(out: &'w mut dyn Write, color: bool, width: usize) -> Self {
        Self {
            out,
            color,
            width: width.max(40),
            indent: 0,
        }
    }

    pub fn width(&self) -> usize {
        self.width.saturating_sub(self.indent)
    }

    /// Runs `f` one level deeper. Indentation is two spaces, everywhere.
    pub fn nest<F>(&mut self, f: F) -> io::Result<()>
    where
        F: FnOnce(&mut Painter<'_>) -> io::Result<()>,
    {
        self.indent += 2;
        let r = f(self);
        self.indent -= 2;
        r
    }

    /// Colours `text`, or returns it untouched when colour is off.
    pub fn paint(&self, text: &str, ink: Ink) -> String {
        if !self.color || ink == Ink::Plain || text.is_empty() {
            return text.to_string();
        }
        let (on, off) = ink.codes();
        format!("{on}{text}{off}")
    }

    pub fn blank(&mut self) -> io::Result<()> {
        writeln!(self.out)
    }

    /// One line, at the current indent. `text` may already contain painted spans.
    pub fn line(&mut self, text: impl AsRef<str>) -> io::Result<()> {
        let text = text.as_ref();
        if text.is_empty() {
            return self.blank();
        }
        writeln!(self.out, "{:indent$}{text}", "", indent = self.indent)
    }

    /// The one-line answer to "what am I looking at". Bold, with a dim trailing aside.
    pub fn title(&mut self, text: &str, aside: &str) -> io::Result<()> {
        let head = self.paint(text, Ink::Bold);
        if aside.is_empty() {
            self.line(head)
        } else {
            let aside = self.paint(aside, Ink::Dim);
            self.line(format!("{head}  {aside}"))
        }
    }

    /// A group label. Dim and lowercase — it separates without competing with the data.
    pub fn section(&mut self, text: &str) -> io::Result<()> {
        self.blank()?;
        let label = self.paint(text, Ink::Label);
        self.line(label)
    }

    /// `✓ something`, at the current indent.
    pub fn marked(&mut self, mark: Mark, text: impl AsRef<str>) -> io::Result<()> {
        let glyph = self.paint(mark.glyph(), mark.ink());
        self.line(format!("{glyph} {}", text.as_ref()))
    }

    /// An aside under the line above it — a fix command, an error detail.
    pub fn note(&mut self, text: impl AsRef<str>) -> io::Result<()> {
        let text = self.paint(text.as_ref(), Ink::Dim);
        self.line(format!("  {text}"))
    }

    /// `key  value`, with the key dim and padded to `pad`.
    pub fn kv(&mut self, key: &str, pad: usize, value: impl AsRef<str>) -> io::Result<()> {
        let label = self.paint(&format!("{key:<pad$}"), Ink::Label);
        self.line(format!("{label}  {}", value.as_ref()))
    }

    /// Renders a built [`Table`] at the current indent.
    pub fn table(&mut self, table: &Table) -> io::Result<()> {
        let widths = table.widths();

        if table.has_headers() {
            let mut cells = Vec::with_capacity(table.cols.len());
            for (col, w) in table.cols.iter().zip(&widths) {
                cells.push(self.paint(&col.pad(&col.title, *w), Ink::Label));
            }
            self.line(cells.join(GUTTER).trim_end())?;
        }

        for row in &table.rows {
            let mut cells = Vec::with_capacity(row.len());
            let last = row.len() - 1;
            for (i, cell) in row.iter().enumerate() {
                let col = &table.cols[i];
                // The final left-aligned column is not padded. Its padding would sit
                // *inside* the colour span, where `trim_end` cannot reach it, and every
                // line would carry invisible trailing whitespace into whatever the output
                // was pasted into.
                let text = if i == last && col.align == Align::Left {
                    cell.text.clone()
                } else {
                    col.pad(&cell.text, widths[i])
                };
                cells.push(self.paint(&text, cell.ink));
            }
            self.line(cells.join(GUTTER).trim_end())?;
        }
        Ok(())
    }

    /// A column of figures with their labels — the shape of every "what did this run do"
    /// report.
    ///
    /// Values are right-aligned so magnitudes line up under each other, and a zero is
    /// dimmed rather than dropped. Dropping it would make "nothing failed" and "failures
    /// were not counted" look identical, which for this project is the whole difference
    /// between a clean run and an unreported one.
    pub fn figures(&mut self, rows: &[(u64, &str)]) -> io::Result<()> {
        let mut table = Table::bare(&[Align::Right, Align::Left]);
        for (n, label) in rows {
            let ink = if *n == 0 { Ink::Dim } else { Ink::Bold };
            table.push(vec![Cell::new(count(*n), ink), Cell::new(*label, Ink::Dim)]);
        }
        self.table(&table)
    }

    /// Wraps `text` to the remaining width and writes it at the current indent.
    pub fn wrapped(&mut self, text: &str, ink: Ink) -> io::Result<()> {
        for line in wrap(text, self.width()) {
            let painted = self.paint(&line, ink);
            self.line(painted)?;
        }
        Ok(())
    }
}

/// Two spaces between columns. Enough to read, little enough to fit.
const GUTTER: &str = "  ";

// ---------------------------------------------------------------------------------------
// Table
// ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Right,
}

struct Col {
    title: String,
    align: Align,
}

impl Col {
    fn pad(&self, text: &str, width: usize) -> String {
        let w = UnicodeWidthStr::width(text);
        let fill = width.saturating_sub(w);
        match self.align {
            Align::Left => format!("{text}{:fill$}", ""),
            Align::Right => format!("{:fill$}{text}", ""),
        }
    }
}

/// One cell: raw text plus the ink to paint it with.
///
/// Text is stored unpainted so column widths are measured on what is *seen* rather than
/// on escape codes — the bug that makes every hand-rolled colour table misalign.
pub struct Cell {
    text: String,
    ink: Ink,
}

impl Cell {
    pub fn new(text: impl Into<String>, ink: Ink) -> Self {
        Self {
            text: text.into(),
            ink,
        }
    }

    pub fn plain(text: impl Into<String>) -> Self {
        Self::new(text, Ink::Plain)
    }

    pub fn dim(text: impl Into<String>) -> Self {
        Self::new(text, Ink::Dim)
    }

    /// A status glyph as a cell, so it can hold a column of its own.
    pub fn mark(mark: Mark) -> Self {
        Self::new(mark.glyph(), mark.ink())
    }
}

/// A space-aligned table. No rules, no borders — the alignment is the structure.
#[derive(Default)]
pub struct Table {
    cols: Vec<Col>,
    rows: Vec<Vec<Cell>>,
}

impl Table {
    /// A table with column headers.
    pub fn new(cols: &[(&str, Align)]) -> Self {
        Self {
            cols: cols
                .iter()
                .map(|(t, a)| Col {
                    title: (*t).to_string(),
                    align: *a,
                })
                .collect(),
            rows: Vec::new(),
        }
    }

    /// A headerless table of `n` left-aligned columns — for lists where the columns are
    /// self-evident and a header row would be three words of chrome over four of data.
    pub fn bare(aligns: &[Align]) -> Self {
        Self {
            cols: aligns
                .iter()
                .map(|a| Col {
                    title: String::new(),
                    align: *a,
                })
                .collect(),
            rows: Vec::new(),
        }
    }

    pub fn push(&mut self, cells: Vec<Cell>) {
        debug_assert_eq!(
            cells.len(),
            self.cols.len(),
            "a row must have one cell per column"
        );
        self.rows.push(cells);
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    fn has_headers(&self) -> bool {
        self.cols.iter().any(|c| !c.title.is_empty())
    }

    /// Widest cell per column, headers included.
    fn widths(&self) -> Vec<usize> {
        let mut widths: Vec<usize> = self
            .cols
            .iter()
            .map(|c| UnicodeWidthStr::width(c.title.as_str()))
            .collect();
        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                let w = UnicodeWidthStr::width(cell.text.as_str());
                if w > widths[i] {
                    widths[i] = w;
                }
            }
        }
        widths
    }
}

// ---------------------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------------------

/// Bytes as a person reads them. Binary units, because that is what a file size is.
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Thousands separators. `215` stays `215`; `1048576` becomes `1,048,576`.
pub fn count(n: impl Into<u64>) -> String {
    let n = n.into();
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `1` → `1 source`, `2` → `2 sources`.
pub fn plural(n: usize, singular: &str, plural: &str) -> String {
    if n == 1 {
        format!("{n} {singular}")
    } else {
        format!("{} {plural}", count(n as u64))
    }
}

/// The first 12 hex characters of a digest.
///
/// Enough to recognise and to grep for, short enough not to own the line. The full digest
/// is one `--json` away, and that is the copy anybody verifying against should use.
pub fn short_sha(sha: &str) -> String {
    sha.chars().take(12).collect()
}

/// An RFC 3339 timestamp as `2026-08-04 00:18`. Sub-second precision is provenance, not
/// something a person reads off a table.
pub fn short_time(ts: &str) -> String {
    let mut chars = ts.chars();
    let date: String = chars.by_ref().take(10).collect();
    // Skip the `T`.
    let time: String = chars.skip(1).take(5).collect();
    if time.len() == 5 {
        format!("{date} {time}")
    } else {
        date
    }
}

/// Seconds as `1.4s`, `2m 30s`, `1h 04m`.
pub fn duration(secs: f64) -> String {
    if secs < 60.0 {
        return format!("{secs:.1}s");
    }
    let total = secs.round() as u64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}h {m:02}m")
    } else {
        format!("{m}m {s:02}s")
    }
}

/// Keeps the head of an over-long string, marking the cut.
pub fn truncate(text: &str, width: usize) -> String {
    if UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in text.chars() {
        let cw = UnicodeWidthStr::width(c.to_string().as_str());
        if w + cw > width.saturating_sub(1) {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

/// Keeps the *tail* — for paths and URLs, where the end is what distinguishes them.
pub fn truncate_start(text: &str, width: usize) -> String {
    if UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let keep = width.saturating_sub(1);
    let tail: String = chars[chars.len().saturating_sub(keep)..].iter().collect();
    format!("…{tail}")
}

/// Collapses whitespace and wraps to `width`, breaking on spaces.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let w = UnicodeWidthStr::width(word);
        if current.is_empty() {
            current = word.to_string();
        } else if UnicodeWidthStr::width(current.as_str()) + 1 + w <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Squashes a multi-line message onto one line — error details arrive with newlines and
/// a table cell cannot hold them.
pub fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_to_string<F>(color: bool, f: F) -> String
    where
        F: FnOnce(&mut Painter<'_>) -> io::Result<()>,
    {
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut p = Painter::new(&mut buf, color, 80);
            f(&mut p).unwrap();
        }
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn colour_off_emits_no_escape_codes() {
        let out = render_to_string(false, |p| {
            p.title("tampa", "191 resources")?;
            p.marked(Mark::Ok, "live")?;
            p.marked(Mark::Bad, "gone")
        });
        assert!(
            !out.contains('\x1b'),
            "escape codes leaked into a pipe: {out:?}"
        );
        assert!(out.contains('✓') && out.contains('✗'));
    }

    #[test]
    fn colour_on_wraps_the_glyphs() {
        let out = render_to_string(true, |p| p.marked(Mark::Ok, "live"));
        assert!(out.contains("\x1b[32m✓\x1b[0m"));
    }

    /// The classic colour-table bug: widths measured over escape codes. Cells store raw
    /// text for exactly this reason, so the coloured and uncoloured layouts must agree.
    #[test]
    fn colour_does_not_shift_columns() {
        let build = |p: &mut Painter<'_>| {
            let mut t = Table::new(&[("name", Align::Left), ("size", Align::Right)]);
            t.push(vec![Cell::new("ffmpeg", Ink::Green), Cell::plain("12")]);
            t.push(vec![
                Cell::new("a-much-longer-name", Ink::Red),
                Cell::plain("4"),
            ]);
            p.table(&t)
        };
        let plain = render_to_string(false, build);
        let colored = render_to_string(true, build);

        let strip = |s: &str| -> Vec<usize> {
            s.lines()
                .map(|l| {
                    let mut out = String::new();
                    let mut in_escape = false;
                    for c in l.chars() {
                        if c == '\x1b' {
                            in_escape = true;
                        } else if in_escape {
                            if c == 'm' {
                                in_escape = false;
                            }
                        } else {
                            out.push(c);
                        }
                    }
                    UnicodeWidthStr::width(out.as_str())
                })
                .collect()
        };
        assert_eq!(strip(&plain), strip(&colored));
    }

    #[test]
    fn nesting_indents_by_two() {
        let out = render_to_string(false, |p| {
            p.line("top")?;
            p.nest(|p| {
                p.line("middle")?;
                p.nest(|p| p.line("inner"))
            })
        });
        assert_eq!(out, "top\n  middle\n    inner\n");
    }

    #[test]
    fn indent_unwinds_even_when_a_nested_render_fails() {
        let mut buf: Vec<u8> = Vec::new();
        let mut p = Painter::new(&mut buf, false, 80);
        let _ = p.nest(|_| -> io::Result<()> { Err(io::Error::other("boom")) });
        p.line("after").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "after\n");
    }

    #[test]
    fn bytes_are_binary_and_short() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(999), "999 B");
        assert_eq!(bytes(1024), "1.0 KiB");
        assert_eq!(bytes(4_279_660_224), "4.0 GiB");
        // Three significant figures drop the decimal, so a column never jumps width.
        assert_eq!(bytes(639_150_592), "610 MiB");
    }

    #[test]
    fn counts_get_thousands_separators() {
        assert_eq!(count(0u64), "0");
        assert_eq!(count(215u64), "215");
        assert_eq!(count(1_048_576u64), "1,048,576");
    }

    #[test]
    fn plurals_agree() {
        assert_eq!(plural(1, "source", "sources"), "1 source");
        assert_eq!(plural(0, "source", "sources"), "0 sources");
        assert_eq!(plural(2000, "chunk", "chunks"), "2,000 chunks");
    }

    #[test]
    fn timestamps_lose_their_sub_second_noise() {
        assert_eq!(
            short_time("2026-08-04T00:18:19.596195Z"),
            "2026-08-04 00:18"
        );
        // A date-only value must not be mangled into something wrong.
        assert_eq!(short_time("2026-08-04"), "2026-08-04");
    }

    #[test]
    fn durations_scale() {
        assert_eq!(duration(1.44), "1.4s");
        assert_eq!(duration(150.0), "2m 30s");
        assert_eq!(duration(3840.0), "1h 04m");
    }

    #[test]
    fn truncation_counts_display_width_not_bytes() {
        let out = truncate("café-café-café", 8);
        assert_eq!(UnicodeWidthStr::width(out.as_str()), 8);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn tail_truncation_keeps_what_distinguishes_a_url() {
        let out = truncate_start("https://example.gov/very/long/path/to/agenda.pdf", 20);
        assert_eq!(UnicodeWidthStr::width(out.as_str()), 20);
        assert!(out.ends_with("agenda.pdf"));
    }

    #[test]
    fn wrapping_never_exceeds_the_width() {
        let text = "the federal government will necessarily absorb the state legislatures";
        for line in wrap(text, 20) {
            assert!(UnicodeWidthStr::width(line.as_str()) <= 20, "{line:?}");
        }
    }

    #[test]
    fn multi_line_details_collapse_to_one() {
        assert_eq!(one_line("a\n  b\tc\n"), "a b c");
    }

    #[test]
    fn a_bare_table_prints_no_header_row() {
        let out = render_to_string(false, |p| {
            let mut t = Table::bare(&[Align::Left, Align::Right]);
            t.push(vec![Cell::plain("live"), Cell::plain("190")]);
            p.table(&t)
        });
        assert_eq!(out, "live  190\n");
    }
}
