//! Recognising a site, and walking it.
//!
//! [`crawl`] asks one question — **where are the addresses** — and answers with a walk: a
//! sitemap index, a directory listing, a product that serves its records a particular way.
//!
//! ## Reading is not a second registry here
//!
//! It was, briefly, and that was a mistake worth recording. Recognition tells you how to
//! *find* the pages and says nothing about reading them — `hillsclerk.com` is recognised by
//! `sitemap`, enumerates 177 addresses without a mistake, and hands back 23,213 characters
//! of navigation for a page whose content is one sentence. True, and it does not follow
//! that the fix belongs beside a crawl strategy.
//!
//! It belongs in [`crate::extract`], which already dispatches on
//! [`crate::content::ContentKind`] to a list of readers tried in order. A second registry
//! in front of that list is a second answer to "how do we get text out of this", and it
//! opted out of every invariant the list exists to hold: one definition of *produced
//! nothing*, one definition of a tool name, `recovered_by_fallback`, and the notes a
//! reader leaves when it gives way. `marked` — the page's own content region — is
//! [`crate::extract::Reader::Marked`], the first element of the HTML row, and it is held to
//! all four.
//!
//! ## The unit of contribution is a strategy, never a site
//!
//! A strategy keys on a **product**, a **framework**, a **server default**, or a
//! **standard** — never on a jurisdiction. Every one of those ships to many cities, which
//! is what makes the work amortise: recognising Hyland OnBase collects every city running
//! OnBase, where teaching it Tampa collects Tampa.
//!
//! [`Keyed`] has no `Jurisdiction` variant, so the rule is enforced by the type rather
//! than by review.
//!
//! ## Evidence, not a verdict
//!
//! [`Recognition`] carries what was seen, because an operator accepts or rejects on it.
//! `docs/FIELD-NOTES.md` entry 1 is what accepting a bare verdict costs — 75 Resources, 75
//! successful acquisitions, liveness `live` on every one, and 75 copies of a navigation
//! menu reading "Preview link expired".

pub mod crawl;

use crate::domain::{Note, NoteMark};

/// What a strategy recognised, and **on what evidence**.
///
/// The evidence is not decoration. The operator accepts or rejects a recognition on it,
/// and entry 1 is what accepting a bare verdict costs.
#[derive(Clone, Debug)]
pub struct Recognition {
    /// The strategy that spoke. Equal to its `name()` — see each registry's test.
    pub strategy: &'static str,
    /// What it keyed on, which decides precedence. See [`Keyed::specificity`].
    pub keyed_on: Keyed,
    /// Why it is sure. Shown to the operator before anything is collected.
    pub evidence: Vec<Note>,
    /// What will go wrong later, said now. Carried forward into every run rather than
    /// printed once — entry 2's host answers HTTP 200 on its error page, and that is a
    /// fact about every future acquisition, not about the moment it was noticed.
    pub warnings: Vec<Note>,
}

impl Recognition {
    pub fn new(strategy: &'static str, keyed_on: Keyed) -> Self {
        Self {
            strategy,
            keyed_on,
            evidence: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn seeing(mut self, label: impl Into<String>, detail: impl Into<String>) -> Self {
        self.evidence.push(Note::new(label, detail));
        self
    }

    pub fn warning(mut self, label: impl Into<String>, detail: impl Into<String>) -> Self {
        self.warnings
            .push(Note::marked(label, detail, NoteMark::Warn));
        self
    }
}

/// What a strategy keys on.
///
/// **There is no `Jurisdiction` variant, and there will not be one.** A strategy that
/// could key on a city is a fork with extra steps: it collects that city and nothing else,
/// and the next city needs another one. The four variants here each ship to many
/// jurisdictions, which is the entire argument for a registry over a pile of forks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyed {
    /// A named vendor application. `Hyland OnBase Agenda Online`, `OpenGov Stories`.
    Product(&'static str),
    /// A web framework and its widgets. `ASP.NET WebForms + Telerik RadGrid`.
    Framework(&'static str),
    /// What a server does when nobody configured it. An IIS directory index.
    ServerDefault(&'static str),
    /// A published standard anyone may implement. `sitemap.xml`.
    Standard(&'static str),
}

impl Keyed {
    /// Lower is more specific, and **more specific wins**.
    ///
    /// Not a tiebreak detail — it is the difference between collecting a site and
    /// collecting its front door. Entry 2's OnBase host also serves a `robots.txt`, so
    /// both the product strategy and the sitemap standard answer for it. The sitemap
    /// answer is *true*: there is a sitemap and it enumerates cleanly. It is also nearly
    /// worthless, because the meetings are in a JSON literal that no sitemap names.
    ///
    /// A recogniser that keyed on the vendor saw more than one that keyed on a standard
    /// every server can satisfy, so it is ranked ahead of it.
    pub fn specificity(self) -> u8 {
        match self {
            Self::Product(_) => 0,
            Self::Framework(_) => 1,
            Self::ServerDefault(_) => 2,
            Self::Standard(_) => 3,
        }
    }

    /// How it reads on a report: `product`, `framework`, `server default`, `standard`.
    pub fn kind(self) -> &'static str {
        match self {
            Self::Product(_) => "product",
            Self::Framework(_) => "framework",
            Self::ServerDefault(_) => "server default",
            Self::Standard(_) => "standard",
        }
    }

    /// The thing recognised: `Hyland OnBase Agenda Online`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Product(n) | Self::Framework(n) | Self::ServerDefault(n) | Self::Standard(n) => n,
        }
    }
}

impl std::fmt::Display for Keyed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.name(), self.kind())
    }
}
