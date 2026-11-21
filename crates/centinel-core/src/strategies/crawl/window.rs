//! A range of days, both ends included: how a meeting search is asked.
//!
//! Shared by the strategies whose seed bounds a walk by date. Each product spells the
//! range its own way in its search address, so the address stays with the strategy and
//! only the range lives here.

use std::fmt;

use jiff::civil::Date;

/// A range of days, both ends included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub from: Date,
    pub to: Date,
}

impl Window {
    /// The two halves, older first, or `None` for a single day.
    pub fn halves(&self) -> Option<(Window, Window)> {
        let days = self.from.until(self.to).ok()?.get_days();
        if days < 1 {
            return None;
        }
        let mid = self
            .from
            .checked_add(jiff::Span::new().days(i64::from(days / 2)))
            .ok()?;
        let next = mid.tomorrow().ok()?;
        Some((
            Window {
                from: self.from,
                to: mid,
            },
            Window {
                from: next,
                to: self.to,
            },
        ))
    }

    /// One calendar year, January 1 to December 31.
    pub fn year(year: i16) -> Option<Window> {
        Some(Window {
            from: Date::new(year, 1, 1).ok()?,
            to: Date::new(year, 12, 31).ok()?,
        })
    }

    /// The days both windows hold, or `None` where they do not meet.
    pub fn within(&self, other: &Window) -> Option<Window> {
        let from = self.from.max(other.from);
        let to = self.to.min(other.to);
        (from <= to).then_some(Window { from, to })
    }
}

impl fmt::Display for Window {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} to {}", self.from, self.to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halving_covers_every_day_once() {
        let w = Window {
            from: jiff::civil::date(2024, 1, 1),
            to: jiff::civil::date(2024, 1, 4),
        };
        let (a, b) = w.halves().unwrap();
        assert_eq!((a.from, a.to), (w.from, jiff::civil::date(2024, 1, 2)));
        assert_eq!((b.from, b.to), (jiff::civil::date(2024, 1, 3), w.to));
        let day = Window {
            from: w.from,
            to: w.from,
        };
        assert!(day.halves().is_none());
    }

    /// A year cut to a range keeps only the range's days in it, and a year outside the
    /// range is no window at all.
    #[test]
    fn a_year_is_cut_to_the_range_it_meets() {
        let range = Window {
            from: jiff::civil::date(2025, 8, 22),
            to: jiff::civil::date(2026, 11, 20),
        };
        let cut = |y| Window::year(y).unwrap().within(&range);
        assert_eq!(
            cut(2025),
            Some(Window {
                from: jiff::civil::date(2025, 8, 22),
                to: jiff::civil::date(2025, 12, 31),
            })
        );
        assert_eq!(
            cut(2026),
            Some(Window {
                from: jiff::civil::date(2026, 1, 1),
                to: jiff::civil::date(2026, 11, 20),
            })
        );
        assert_eq!(cut(2024), None);
    }
}
