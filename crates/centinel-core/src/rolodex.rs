//! The **Rolodex**: the people the corpus names by linking to them.
//!
//! A page says who an address belongs to in exactly one place a scan can trust — the text of
//! the link. `<a href="mailto:jdoe@tampagov.net">Jane Doe</a>` pairs a name with an address,
//! `<a href="/council/viera">Council Member Luis Viera</a>` pairs one with a page about them,
//! and `<a href="https://x.com/LuisVieraTampa">` is an account the site itself points at. Each
//! is a [`Mention`]. Names in running text are a different question, with a model in it, and
//! are not read here.
//!
//! ## Written where the page is in hand, beside the crumbs
//!
//! [`crate::crumbs::scan_page`] already walks every `<a>` of every page `collect` stores, so a
//! mention costs nothing but the test of its text, and lands on the same ledger row as the
//! page's crumbs. The ledger is derived, so the mentions are too: the blob is the floor, a
//! better test of what a name is takes `crumbs --rescan`, and a row written before mentions
//! were read says so — it has no `mentions` at all, which is a different answer from an
//! empty list.
//!
//! ## A name, not a person
//!
//! An entry is a **name as written**, keyed on its lowercased spelling with titles dropped. Two
//! Jane Does in two cities are one entry, and `Jane A. Doe` is another: merging people on
//! anything fuzzier is identity resolution, and a wrong merge corrupts the record quietly.
//! Each entry carries the sources and pages that named it, so a reader can tell two people
//! apart; the store never claims they are one.
//!
//! ## Cached by the ledger, like the graph's linkage
//!
//! One source's ledger is folded into [`Named`] and written to `summaries/<source>.names.json`,
//! stamped with the ledger it was read from — the same bargain as
//! [`crate::graph::Linkage`], for the same reason: the ledger changes once a run, and the
//! Rolodex page asks far more often than that. Derived, and safe to delete.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::crumbs::{Ledger, LedgerStamp, Recorded};
use crate::domain::SourceId;
use crate::html::Link;
use crate::store::Store;

/// Pages named per source on an entry before the rest are counted.
pub const MAX_PAGES: usize = 24;

/// Bumped when the fold changes, so a cache written by an older build is folded again.
const FOLD: u32 = 2;

// ── what a link says about a person ──────────────────────────────────────────

/// One link on one page that names somebody, or an address of theirs.
///
/// `name` is the link's text when that text is a person's name ([`name_of`]), and absent when
/// it is "Email" or "Twitter" — the address is still somebody's, and is kept, but nothing on
/// the page says whose.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Mention {
    /// A `mailto:` link, or Cloudflare's protected form of one.
    Email {
        address: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// A profile on a social platform.
    Profile {
        platform: String,
        handle: String,
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// Any other link whose text is a name: a bio page, a staff directory entry.
    Page { name: String, url: String },
}

impl Mention {
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Email { name, .. } | Self::Profile { name, .. } => name.as_deref(),
            Self::Page { name, .. } => Some(name),
        }
    }

    /// What it identifies, beside any name: the address, or `platform:handle`.
    fn identifier(&self) -> Option<String> {
        match self {
            Self::Email { address, .. } => Some(address.clone()),
            Self::Profile {
                platform, handle, ..
            } => Some(format!("{platform}:{handle}")),
            Self::Page { .. } => None,
        }
    }
}

/// What one link says about a person, given where it resolved to.
///
/// Called by [`crate::crumbs::scan_page`] for every link on a page, on- or off-host, before
/// the page decides whether the link is a crumb.
pub fn mentions(target: &url::Url, link: &Link<'_>) -> Vec<Mention> {
    let text = crate::html::text(link.inner);
    let name = name_of(&text);

    if target.scheme() == "mailto" {
        let addresses = percent_decode(target.path());
        return addresses
            .split([',', ';'])
            .filter_map(email)
            .map(|address| Mention::Email {
                address,
                name: name.clone(),
            })
            .collect();
    }
    if !matches!(target.scheme(), "http" | "https") {
        return Vec::new();
    }
    if target.path().ends_with("/cdn-cgi/l/email-protection") {
        let encoded = target
            .fragment()
            .map(str::to_string)
            .or_else(|| cloudflare_attr(link));
        return encoded
            .and_then(|hex| cloudflare(&hex))
            .and_then(|plain| email(&plain))
            .map(|address| Mention::Email { address, name })
            .into_iter()
            .collect();
    }
    if let Some((platform, handle)) = profile_of(target) {
        let mut url = target.clone();
        url.set_query(None);
        url.set_fragment(None);
        return vec![Mention::Profile {
            platform: platform.to_string(),
            handle,
            url: url.to_string(),
            name,
        }];
    }
    match name {
        Some(name) => {
            let mut url = target.clone();
            url.set_fragment(None);
            vec![Mention::Page {
                name,
                url: url.to_string(),
            }]
        }
        None => Vec::new(),
    }
}

/// An address, lowercased, or `None` for anything that is not one.
fn email(raw: &str) -> Option<String> {
    let address = raw.trim().to_ascii_lowercase();
    let (local, domain) = address.split_once('@')?;
    let fine = |c: char| c.is_ascii_alphanumeric() || "._%+-'".contains(c);
    let valid = !local.is_empty()
        && local.chars().all(fine)
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && domain
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    valid.then_some(address)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Cloudflare's "email protection": the address XORed with the first byte, as hex.
///
/// A site behind Cloudflare serves every `mailto:` this way, so without it the people on
/// those sites have no address at all.
fn cloudflare(hex: &str) -> Option<String> {
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect::<Option<_>>()?;
    let (key, rest) = bytes.split_first()?;
    String::from_utf8(rest.iter().map(|b| b ^ key).collect()).ok()
}

/// The encoded address when it sits on the element rather than in the link's fragment.
fn cloudflare_attr(link: &Link<'_>) -> Option<String> {
    if let Some(hex) = link.tag.attr("data-cfemail") {
        return Some(hex.to_string());
    }
    crate::html::Scan::new(link.inner)
        .tags(&["span"])
        .into_iter()
        .find_map(|t| t.attr("data-cfemail").map(str::to_string))
}

/// The platform and handle a profile link names, or `None` for a share button, a post, or
/// anything else that is not somebody's account.
pub fn profile_of(url: &url::Url) -> Option<(&'static str, String)> {
    let host = url.host_str()?.to_ascii_lowercase();
    let host = host
        .strip_prefix("www.")
        .or_else(|| host.strip_prefix("m."))
        .unwrap_or(&host);
    let platform = match host {
        "x.com" | "twitter.com" => "x",
        "facebook.com" | "fb.com" => "facebook",
        "instagram.com" => "instagram",
        "linkedin.com" => "linkedin",
        "youtube.com" => "youtube",
        "tiktok.com" => "tiktok",
        "threads.net" | "threads.com" => "threads",
        "bsky.app" => "bluesky",
        "nextdoor.com" => "nextdoor",
        "github.com" => "github",
        "medium.com" => "medium",
        "reddit.com" => "reddit",
        "truthsocial.com" => "truthsocial",
        "linktr.ee" => "linktree",
        _ => return None,
    };
    // The whole path is the account, and nothing past it: `x.com/jane/status/123` is one
    // of Jane's posts, and filing it as her account would open a post where the account
    // should be and fold every post she has under whichever came first. A named link to a
    // post is still a page about her — [`mentions`] files it as one.
    let path = percent_decode(url.path());
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let handle = match platform {
        // A person on LinkedIn is `/in/…`; `/company/…` is an organisation, kept for the
        // same reason an unnamed address is.
        "linkedin" => match parts.as_slice() {
            [kind @ ("in" | "company" | "school"), name] => format!("{kind}/{name}"),
            _ => return None,
        },
        // A channel's tabs are still the channel; a video is `/watch`, never under it.
        "youtube" => {
            let tab = |rest: &[&str]| match rest {
                [] => true,
                [tab] => matches!(
                    *tab,
                    "videos" | "featured" | "streams" | "shorts" | "playlists" | "about"
                ),
                _ => false,
            };
            match parts.as_slice() {
                [at, rest @ ..] if at.starts_with('@') && tab(rest) => at.to_string(),
                [kind @ ("c" | "user" | "channel"), name, rest @ ..] if tab(rest) => {
                    format!("{kind}/{name}")
                }
                _ => return None,
            }
        }
        "bluesky" => match parts.as_slice() {
            ["profile", name] => name.to_string(),
            _ => return None,
        },
        "reddit" => match parts.as_slice() {
            ["u" | "user", name] => format!("u/{name}"),
            _ => return None,
        },
        _ => match parts.as_slice() {
            [one] if !NOT_A_HANDLE.contains(&one.to_ascii_lowercase().as_str()) => {
                one.trim_start_matches('@').to_string()
            }
            _ => return None,
        },
    };
    // Handles are case-insensitive on every platform here; the URL keeps the casing served.
    Some((platform, handle.to_ascii_lowercase()))
}

/// The first segment of a link that is an action or a section of the platform, not an
/// account: share buttons, intents, searches, a single post.
const NOT_A_HANDLE: &[&str] = &[
    "sharer",
    "sharer.php",
    "share",
    "intent",
    "home",
    "search",
    "hashtag",
    "watch",
    "embed",
    "login",
    "signup",
    "dialog",
    "plugins",
    "tr",
    "events",
    "groups",
    "pages",
    "p",
    "reel",
    "reels",
    "explore",
    "feed",
    "status",
    "i",
    "policies",
    "privacy",
    "legal",
    "help",
    "about",
    "settings",
    "sharearticle",
    "sharing",
    "results",
    "playlist",
    "redirect",
    "profile.php",
    "people",
    "photo",
    "photos",
    "photo.php",
    "video",
    "videos",
    "posts",
    "story.php",
    "permalink.php",
    "media",
    "notes",
    "watch.php",
    "l.php",
    "dialog",
    "messages",
    "hashtags",
];

// ── what a name is ───────────────────────────────────────────────────────────

/// The link text as a person's name, or `None` when it is something else.
///
/// Deliberately narrow, because every false yes is an entry in the Rolodex for "Billing
/// Questions" and every false no is only a link the scan did not keep: two to four
/// capitalised words, no digits, no word that names a place, a body or a page, at least two
/// words longer than an initial — and **one of them a given name** ([`is_given_name`]).
/// Titles in front are dropped — `Council Member Luis Viera` is `Luis Viera` — so the same
/// person linked from two pages is one entry.
///
/// The given name is what does the work. On four tampa.gov pages the shape alone admitted
/// 98 entries and 45 of them were navigation: a site's menu is title-cased phrases too, and
/// a stop list chasing them grows by one city's vocabulary at a time. Any word rather than
/// the first, because a roster is often written surname first — `Iorio Pam`.
pub fn name_of(text: &str) -> Option<String> {
    let mut tokens: Vec<&str> = text.split_whitespace().collect();
    while let Some(first) = tokens.first() {
        let word = first.trim_end_matches(['.', ',', ':']).to_ascii_lowercase();
        match TITLES.contains(&word.as_str()) {
            true => {
                tokens.remove(0);
            }
            false => break,
        }
    }
    let tokens: Vec<&str> = tokens
        .into_iter()
        .map(|t| t.trim_end_matches([',', ':', ';']))
        .collect();

    if !(2..=4).contains(&tokens.len()) {
        return None;
    }
    let name = tokens.join(" ");
    if name.len() > 40 || name.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    if !tokens.iter().all(|t| is_name_word(t)) {
        return None;
    }
    // An initial is never a stop word, or every `A.` would read as the article.
    if tokens.iter().any(|t| {
        let bare = t.trim_end_matches('.').to_lowercase();
        bare.chars().count() > 1 && NOT_A_NAME.contains(&bare.as_str())
    }) {
        return None;
    }
    let words: Vec<&str> = tokens
        .iter()
        .map(|t| t.trim_end_matches('.'))
        .filter(|t| t.chars().count() > 1)
        .collect();
    let vouches = |w: &&str| !AMBIGUOUS.contains(&w.to_lowercase().as_str()) && is_given_name(w);
    (words.len() >= 2 && words.iter().any(vouches)).then_some(name)
}

/// Whether a word is somebody's first name: in the SSA's list of given names
/// (`data/given-names.txt`, whose header says how it was made), accents folded, since the
/// list has none. A hyphenated word counts when any half does — `Mary-Kate`.
fn is_given_name(word: &str) -> bool {
    static GIVEN: std::sync::LazyLock<std::collections::HashSet<&'static str>> =
        std::sync::LazyLock::new(|| {
            include_str!("../data/given-names.txt")
                .lines()
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .collect()
        });
    let folded: String = word
        .chars()
        .flat_map(char::to_lowercase)
        .map(fold)
        .collect();
    folded.split(['-', '\'']).any(|part| GIVEN.contains(part))
}

/// A Latin letter without its accent: `é` is `e`. The given-name list is ASCII.
fn fold(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => 'a',
        'ç' | 'ć' | 'č' => 'c',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' | 'ě' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'ī' => 'i',
        'ñ' | 'ń' | 'ň' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => 'o',
        'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' => 'u',
        'ý' | 'ÿ' => 'y',
        'ś' | 'š' => 's',
        'ź' | 'ż' | 'ž' => 'z',
        'ł' => 'l',
        'ř' => 'r',
        c => c,
    }
}

/// `Jane`, `O'Neil`, `Smith-Jones`, `McDonald`, `J.`, `Jr.` — and not `CITY` or `budget`.
fn is_name_word(token: &str) -> bool {
    let bare = token.trim_end_matches('.');
    if matches!(bare, "Jr" | "Sr" | "II" | "III" | "IV") {
        return true;
    }
    let mut chars = bare.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_uppercase() {
        return false;
    }
    // An initial.
    if bare.chars().count() == 1 {
        return true;
    }
    bare.split(['-', '\''])
        .all(|part| part.chars().all(char::is_alphabetic) && !part.is_empty())
        && bare.chars().any(char::is_lowercase)
}

/// Words in front of a name that say what the person is, not who.
const TITLES: &[&str] = &[
    "mayor",
    "vice",
    "council",
    "member",
    "councilmember",
    "councilman",
    "councilwoman",
    "commissioner",
    "chair",
    "chairman",
    "chairwoman",
    "chairperson",
    "director",
    "deputy",
    "chief",
    "dr",
    "mr",
    "mrs",
    "ms",
    "hon",
    "honorable",
    "judge",
    "sheriff",
    "rep",
    "sen",
    "senator",
    "representative",
    "superintendent",
    "manager",
    "city",
    "county",
    "attorney",
    "president",
    "secretary",
    "treasurer",
    "clerk",
    "executive",
    "assistant",
    "interim",
    "acting",
    "officer",
    "trustee",
    "alderman",
    "alderwoman",
    "supervisor",
    "governor",
    "lieutenant",
    "lt",
    "sgt",
    "sergeant",
    "captain",
    "capt",
    "major",
    "colonel",
    "col",
];

/// Words that make a capitalised phrase a place, a body or a page rather than a person.
const NOT_A_NAME: &[&str] = &[
    "museum",
    "hospital",
    "cathedral",
    "chapel",
    "cemetery",
    "memorial",
    "stadium",
    "theatre",
    "theater",
    "institute",
    "academy",
    "clinic",
    "garden",
    "gardens",
    "plaza",
    "square",
    "bridge",
    "tower",
    "city",
    "county",
    "state",
    "department",
    "office",
    "services",
    "service",
    "division",
    "board",
    "commission",
    "council",
    "committee",
    "meeting",
    "meetings",
    "agenda",
    "agendas",
    "minutes",
    "home",
    "contact",
    "us",
    "about",
    "news",
    "events",
    "calendar",
    "more",
    "read",
    "view",
    "download",
    "report",
    "reports",
    "plan",
    "plans",
    "program",
    "programs",
    "center",
    "centre",
    "parks",
    "police",
    "fire",
    "public",
    "works",
    "water",
    "utilities",
    "library",
    "permits",
    "permit",
    "planning",
    "zoning",
    "budget",
    "finance",
    "human",
    "resources",
    "clerk",
    "tax",
    "collector",
    "property",
    "records",
    "request",
    "online",
    "pay",
    "map",
    "maps",
    "project",
    "projects",
    "development",
    "community",
    "housing",
    "transportation",
    "road",
    "avenue",
    "building",
    "facebook",
    "twitter",
    "instagram",
    "youtube",
    "linkedin",
    "email",
    "search",
    "menu",
    "login",
    "sign",
    "skip",
    "main",
    "content",
    "privacy",
    "policy",
    "terms",
    "accessibility",
    "site",
    "employment",
    "jobs",
    "careers",
    "apply",
    "form",
    "forms",
    "documents",
    "document",
    "file",
    "files",
    "united",
    "states",
    "america",
    "government",
    "agency",
    "authority",
    "district",
    "school",
    "schools",
    "university",
    "college",
    "foundation",
    "association",
    "inc",
    "llc",
    "corp",
    "company",
    "group",
    "the",
    "and",
    "of",
    "for",
    "to",
    "in",
    "at",
    "on",
    "by",
    "with",
    "a",
    "an",
    "january",
    "february",
    "march",
    "september",
    "october",
    "november",
    "december",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "new",
    "event",
    "notice",
    "press",
    "release",
    "releases",
    "code",
    "ordinance",
    "resolution",
    "annual",
    "information",
    "info",
    "help",
    "faq",
    "faqs",
    "click",
    "here",
    "learn",
    "watch",
    "live",
    "video",
    "videos",
    "photo",
    "photos",
    "gallery",
    "visit",
    "website",
    "web",
    "back",
    "top",
    "next",
    "previous",
    "all",
    "full",
    "list",
    "directory",
    "staff",
    "team",
    "our",
    "your",
    "my",
    "this",
    "get",
    "find",
    "submit",
    "register",
    "registration",
    "district",
    "ward",
    "precinct",
    "election",
    "elections",
    "vote",
    "voting",
    "airport",
    "transit",
    "bus",
    "trail",
];

/// Words that are names and ordinary words both — `Bill`, `Grant`, `River`, `June`,
/// `Hall`, `West`. They neither veto a name nor vouch for one: `Jane Hall` and `Bill
/// Nelson` are people because `Jane` and `Nelson` say so, and `River Walk` and `Grant
/// Application` are not, because nothing else does. The cost is a person whose only
/// given name is one of these — `April Smith` — which is a link not kept rather than an
/// entry for a park.
const AMBIGUOUS: &[&str] = &[
    "bill", "grant", "park", "river", "lake", "bay", "island", "beach", "port", "court", "street",
    "hall", "church", "page", "west", "east", "north", "south", "april", "may", "june", "july",
    "august", "summer", "autumn", "winter", "spring", "chase", "hope", "faith", "joy", "major",
    "mason", "dean", "will", "rose", "victoria",
];

/// The key an entry is filed under: the name lowercased, one space between words.
pub fn key_of(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

// ── one source's ledger, gathered by name ────────────────────────────────────

/// One name, and everything the pages that named it linked to.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Person {
    /// The spelling used most, which is what the entry is shown as.
    pub name: String,
    /// [`key_of`] the name: what a reader asks for it by.
    pub key: String,
    /// Every spelling that filed here, and how many links used it.
    pub spellings: BTreeMap<String, usize>,
    /// Distinct pages naming them, over every source.
    pub pages: usize,
    /// Per source: how many of its pages named them, and the first few.
    pub sources: BTreeMap<String, Pages>,
    /// Addresses linked under this name, and how many pages did.
    pub emails: BTreeMap<String, usize>,
    /// Accounts linked under this name, keyed `platform:handle`.
    pub profiles: BTreeMap<String, Profile>,
    /// Pages about them that a link under their name pointed at, and how many pages did.
    pub links: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Pages {
    pub pages: usize,
    /// The smallest addresses, up to [`MAX_PAGES`]: order-free, so a rescan names the same.
    pub examples: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Profile {
    pub platform: String,
    pub handle: String,
    /// The address as one page linked it.
    pub url: String,
    pub pages: usize,
}

impl Person {
    fn named(key: String) -> Self {
        Self {
            key,
            ..Default::default()
        }
    }

    /// Adds one page's mentions of this person. Each argument is one mention.
    fn absorb_page(&mut self, source: &str, address: &str, said: &[&Mention]) {
        self.pages += 1;
        let pages = self.sources.entry(source.to_string()).or_default();
        pages.pages += 1;
        keep_smallest(&mut pages.examples, address);

        let mut emails = BTreeSet::new();
        let mut profiles = BTreeSet::new();
        let mut links = BTreeSet::new();
        for mention in said {
            if let Some(name) = mention.name() {
                *self.spellings.entry(name.to_string()).or_default() += 1;
            }
            match mention {
                Mention::Email { address, .. } => {
                    emails.insert(address.clone());
                }
                Mention::Profile {
                    platform,
                    handle,
                    url,
                    ..
                } => {
                    let id = format!("{platform}:{handle}");
                    if profiles.insert(id.clone()) {
                        let profile = self.profiles.entry(id).or_insert_with(|| Profile {
                            platform: platform.clone(),
                            handle: handle.clone(),
                            url: url.clone(),
                            pages: 0,
                        });
                        profile.pages += 1;
                    }
                }
                Mention::Page { url, .. } => {
                    links.insert(url.clone());
                }
            }
        }
        for email in emails {
            *self.emails.entry(email).or_default() += 1;
        }
        for link in links {
            *self.links.entry(link).or_default() += 1;
        }
        self.name = self.spelled();
    }

    /// Adds another source's entry for the same key.
    fn merge(&mut self, other: Person) {
        self.pages += other.pages;
        for (spelling, n) in other.spellings {
            *self.spellings.entry(spelling).or_default() += n;
        }
        for (source, pages) in other.sources {
            let mine = self.sources.entry(source).or_default();
            mine.pages += pages.pages;
            for example in pages.examples {
                keep_smallest(&mut mine.examples, &example);
            }
        }
        for (email, n) in other.emails {
            *self.emails.entry(email).or_default() += n;
        }
        for (id, profile) in other.profiles {
            self.profiles
                .entry(id)
                .and_modify(|p| p.pages += profile.pages)
                .or_insert(profile);
        }
        for (link, n) in other.links {
            *self.links.entry(link).or_default() += n;
        }
        self.name = self.spelled();
    }

    /// Most used, and the first in order on a tie, so two reads show the same spelling.
    fn spelled(&self) -> String {
        self.spellings
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
            .map(|(s, _)| s.clone())
            .unwrap_or_default()
    }
}

fn keep_smallest(examples: &mut Vec<String>, address: &str) {
    if let Err(at) = examples.binary_search_by(|e| e.as_str().cmp(address)) {
        examples.insert(at, address.to_string());
        examples.truncate(MAX_PAGES);
    }
}

/// One source's ledger, gathered by name.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Named {
    fold: u32,
    /// The ledger and the log this was folded from. It answers for them while both still
    /// match: a page re-collected changes the log and not always the ledger — `ingest`
    /// writes no row — and a fold keyed on the ledger alone went on naming whoever the
    /// page used to.
    pub ledger: LedgerStamp,
    pub log: BTreeMap<String, u64>,
    /// Pages the log holds: one per address with an Observation.
    pub pages: usize,
    /// Of those, pages whose row was read for names out of the blob the log says is
    /// current. The rest are pages `crumbs --rescan` has yet to read — a row that predates
    /// mentions, a row for bytes the page no longer serves, or no row at all.
    pub read: usize,
    /// Addresses and handles linked with no name beside them.
    pub unnamed: BTreeSet<String>,
    pub people: BTreeMap<String, Person>,
}

impl Named {
    /// Folds one source's pages: each page the log holds, with the ledger row that answers
    /// for its current blob, if any.
    pub fn of<'a>(source: &str, pages: impl IntoIterator<Item = Option<&'a Recorded>>) -> Self {
        let mut named = Self {
            fold: FOLD,
            ..Default::default()
        };
        for row in pages {
            named.pages += 1;
            let Some((row, mentions)) = row.and_then(|r| Some((r, r.mentions.as_ref()?))) else {
                continue;
            };
            named.read += 1;
            let mut by_key: BTreeMap<String, Vec<&Mention>> = BTreeMap::new();
            for mention in mentions {
                match mention.name() {
                    Some(name) => by_key.entry(key_of(name)).or_default().push(mention),
                    None => {
                        named.unnamed.extend(mention.identifier());
                    }
                }
            }
            for (key, said) in by_key {
                named
                    .people
                    .entry(key.clone())
                    .or_insert_with(|| Person::named(key))
                    .absorb_page(source, &row.page.address, &said);
            }
        }
        named
    }

    /// The fold for this source as its log and ledger are now.
    ///
    /// A row counts only for the blob the log says is current ([`Recorded::is_of`]), the
    /// rule `crumbs` already holds its rows to: a page whose bytes changed since its row
    /// was written is a page with no row, not a page that still names who it used to.
    pub async fn get(store: &Store, source: &SourceId) -> anyhow::Result<Self> {
        let ledger = Ledger::new(store, source.clone());
        let stamp = ledger.stamp().await?;
        let log = store.log_sizes(source).await?;
        crate::summary::cached(
            &store.names_path(source),
            |named: &Named| named.fold == FOLD && named.ledger == stamp && named.log == log,
            async {
                let rows = ledger.current().await?;
                let latest = store.replay(source).await?.latest_observations();
                let mut named = Named::of(
                    source.as_str(),
                    latest.values().map(|obs| {
                        rows.get(&obs.resource.natural_key)
                            .filter(|row| row.is_of(&obs.blob_sha))
                    }),
                );
                named.ledger = stamp;
                named.log = log.clone();
                Ok(named)
            },
        )
        .await
    }
}

// ── the Rolodex ──────────────────────────────────────────────────────────────

/// Every name the selected sources link, merged by key.
#[derive(Clone, Debug, Default)]
pub struct Rolodex {
    pub sources: Vec<String>,
    /// Pages the selected sources hold, from their logs.
    pub pages: usize,
    /// Of those, pages nobody has read for names yet. Counted against the log, never the
    /// ledger alone: a page with no row at all is the case a ledger cannot see.
    pub unread: usize,
    pub unnamed: usize,
    /// Most-named first.
    pub people: Vec<Person>,
}

impl Rolodex {
    /// One source, or every source the store defines.
    pub async fn read(store: &Store, source: Option<&str>) -> anyhow::Result<Self> {
        let catalog = crate::catalog::Sources::new(store);
        let selected = match source {
            Some(id) => vec![catalog.require(id).await?],
            None => catalog.list().await?,
        };

        let mut out = Self::default();
        let mut people: BTreeMap<String, Person> = BTreeMap::new();
        let mut unnamed = BTreeSet::new();
        for source in selected {
            let id = SourceId::new(source.id.clone())?;
            let named = Named::get(store, &id).await?;
            out.pages += named.pages;
            out.unread += named.pages - named.read;
            unnamed.extend(named.unnamed);
            for (key, person) in named.people {
                match people.get_mut(&key) {
                    Some(mine) => mine.merge(person),
                    None => {
                        people.insert(key, person);
                    }
                }
            }
            out.sources.push(source.id);
        }
        out.unnamed = unnamed.len();
        out.people = people.into_values().collect();
        out.people
            .sort_by(|a, b| b.pages.cmp(&a.pages).then(a.key.cmp(&b.key)));
        Ok(out)
    }

    /// The entries whose name holds every word of `query`, any case.
    pub fn matching(mut self, query: &str) -> Self {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        self.people.retain(|p| {
            words.iter().all(|w| {
                p.key.contains(w.as_str())
                    || p.spellings
                        .keys()
                        .any(|s| s.to_lowercase().contains(w.as_str()))
            })
        });
        self
    }

    /// One entry, by name or key.
    pub fn person(&self, name: &str) -> Option<&Person> {
        let key = key_of(name);
        self.people.iter().find(|p| p.key == key)
    }

    /// Every source a name comes up in, among the ones this Rolodex read: a page linking
    /// it, or an indexed document whose text holds it word for word — under the name asked
    /// for and every spelling the links used.
    ///
    /// The text half is a read of the index, not a collection: nothing is written, and no
    /// model decides anything. Without an index there is no text to ask, and every
    /// `named` says so by being absent rather than zero.
    pub async fn appearances(&self, store: &Store, name: &str) -> anyhow::Result<Vec<Appearance>> {
        let person = self.person(name);
        let mut phrases: BTreeMap<String, String> = BTreeMap::new();
        for spelling in std::iter::once(name).chain(
            person
                .into_iter()
                .flat_map(|p| p.spellings.keys().map(String::as_str)),
        ) {
            phrases
                .entry(key_of(spelling))
                .or_insert_with(|| spelling.to_string());
        }
        let phrases: Vec<String> = phrases.into_values().collect();

        // On a blocking thread: an FTS join over a common name is real work, and the runtime
        // serving every other request must not wait on it. Narrowed to the sources this
        // Rolodex read; every source is no narrowing at all.
        let path = store.index_path();
        let named: Option<BTreeMap<String, usize>> = match path.exists() {
            true => {
                let defined = crate::catalog::Sources::new(store).list().await?.len();
                let only = match self.sources.len() == defined {
                    true => Vec::new(),
                    false => self.sources.clone(),
                };
                Some(
                    tokio::task::spawn_blocking(move || {
                        crate::index::Index::open(&path)?.sources_naming(&phrases, &only)
                    })
                    .await??,
                )
            }
            false => None,
        };

        let mut by_source: BTreeMap<&str, Appearance> = BTreeMap::new();
        for source in &self.sources {
            let linked = person
                .and_then(|p| p.sources.get(source))
                .map_or(0, |pages| pages.pages);
            let named = named.as_ref().map(|n| n.get(source).copied().unwrap_or(0));
            if linked > 0 || named.unwrap_or(0) > 0 {
                by_source.insert(
                    source,
                    Appearance {
                        source: source.clone(),
                        linked,
                        named,
                    },
                );
            }
        }
        let mut out: Vec<Appearance> = by_source.into_values().collect();
        out.sort_by(|a, b| b.weight().cmp(&a.weight()).then(a.source.cmp(&b.source)));
        Ok(out)
    }
}

/// One source a name comes up in, and how.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Appearance {
    pub source: String,
    /// Pages whose links name them — the Rolodex's own evidence.
    pub linked: usize,
    /// Indexed documents whose text holds the name word for word. Absent when the store
    /// has no index, which is *not asked*, not *none*.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub named: Option<usize>,
}

impl Appearance {
    fn weight(&self) -> usize {
        self.linked + self.named.unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html::Scan;

    fn said(base: &str, html: &str) -> Vec<Mention> {
        let base = url::Url::parse(base).unwrap();
        Scan::new(html)
            .links()
            .iter()
            .flat_map(|link| {
                let href = link.tag.attr("href").unwrap();
                mentions(&base.join(&crate::html::unescape(href)).unwrap(), link)
            })
            .collect()
    }

    #[test]
    fn a_name_is_two_to_four_capitalised_words_without_a_title() {
        assert_eq!(
            name_of("Council Member Luis Viera").as_deref(),
            Some("Luis Viera")
        );
        assert_eq!(name_of("Mayor Jane Castor").as_deref(), Some("Jane Castor"));
        assert_eq!(
            name_of("Jane A. Doe, Jr.").as_deref(),
            Some("Jane A. Doe Jr.")
        );
        assert_eq!(
            name_of("Mary O'Neil-McDonald").as_deref(),
            Some("Mary O'Neil-McDonald")
        );
        assert_eq!(name_of("José Núñez").as_deref(), Some("José Núñez"));
        // A roster written surname first is still a name.
        assert_eq!(name_of("Iorio Pam").as_deref(), Some("Iorio Pam"));
        // A word that is a place or a month too does not throw a person out…
        for person in [
            "Jane Hall",
            "John West",
            "Bill Nelson",
            "Mary Park",
            "June Carter Cash",
        ] {
            assert_eq!(
                name_of(person).as_deref(),
                Some(person),
                "{person} is a name"
            );
        }
        for not in [
            "City Council",
            "Public Works",
            "Email",
            "Read More",
            "JANE DOE",
            "Agenda 2024",
            "J. D.",
            "Tampa Police Department",
            "Contact Us",
            // Navigation and institutions from four real tampa.gov pages, which the shape
            // alone took for names.
            "Billing Questions",
            "Solid Waste",
            "Key Contacts",
            "Sister Cities",
            "Sacred Heart Church",
            "Tampa General Hospital",
            "Henry B. Plant Museum",
            // …and cannot make a person of a place on its own.
            "River Walk",
            "Grant Application",
            "Lake Park",
            "",
        ] {
            assert_eq!(name_of(not), None, "{not:?} is not a name");
        }
    }

    /// The three ways a page links a person, and the two it links an address with nobody's
    /// name beside it.
    #[test]
    fn a_page_names_people_by_mail_by_profile_and_by_page() {
        let found = said(
            "https://www.tampa.gov/council",
            r#"<a href="mailto:jane.doe@tampagov.net">Jane Doe</a>
               <a href="mailto:Clerk@TampaGov.net?subject=hi">Email</a>
               <a href="/council/luis-viera">Council Member Luis Viera</a>
               <a href="https://twitter.com/LuisVieraTampa?ref=x">Luis Viera</a>
               <a href="https://www.facebook.com/sharer/sharer.php?u=x">Share</a>
               <a href="https://www.facebook.com/CityofTampa">Facebook</a>
               <a href="/departments">Public Works</a>"#,
        );
        assert_eq!(
            found,
            vec![
                Mention::Email {
                    address: "jane.doe@tampagov.net".into(),
                    name: Some("Jane Doe".into())
                },
                Mention::Email {
                    address: "clerk@tampagov.net".into(),
                    name: None
                },
                Mention::Page {
                    name: "Luis Viera".into(),
                    url: "https://www.tampa.gov/council/luis-viera".into()
                },
                Mention::Profile {
                    platform: "x".into(),
                    handle: "luisvieratampa".into(),
                    url: "https://twitter.com/LuisVieraTampa".into(),
                    name: Some("Luis Viera".into())
                },
                Mention::Profile {
                    platform: "facebook".into(),
                    handle: "cityoftampa".into(),
                    url: "https://www.facebook.com/CityofTampa".into(),
                    name: None
                },
            ]
        );
    }

    /// What the page renders is what is read: inline markup inside a name, a character
    /// reference for an accent — through the scan `collect` runs, not just the text helper.
    #[test]
    fn a_name_is_read_as_the_page_renders_it() {
        let row = crate::crumbs::scan_page(
            r#"<a href="/staff/oneil">Mary O<span>'Neil</span></a>
               <a href="mailto:jnunez@tampagov.net">Jos&#233; N&uacute;&ntilde;ez</a>
               <a href="/staff/doe"><abbr>Dr.</abbr> Jane Doe</a>"#,
            crate::crumbs::Carrier::at_address("https://www.tampa.gov/staff"),
        );
        let mut names: Vec<&str> = row
            .mentions
            .iter()
            .flatten()
            .filter_map(Mention::name)
            .collect();
        names.sort();
        assert_eq!(names, ["Jane Doe", "José Núñez", "Mary O'Neil"]);
    }

    /// A post is not the account: it is a page about them, and the account is its own path.
    #[test]
    fn a_link_to_a_post_is_a_page_and_not_an_account() {
        for post in [
            "https://x.com/janedoe/status/123",
            "https://bsky.app/profile/jane.bsky.social/post/3k",
            "https://www.youtube.com/@janedoe/community/9",
            "https://www.reddit.com/user/janedoe/comments/abc",
        ] {
            let url = url::Url::parse(post).unwrap();
            assert_eq!(profile_of(&url), None, "{post} is a post");
        }
        let found = said(
            "https://www.tampa.gov/council",
            r#"<a href="https://x.com/janedoe/status/123">Jane Doe</a>
               <a href="https://x.com/JaneDoe">Jane Doe</a>
               <a href="https://www.youtube.com/@JaneDoe/videos">Jane Doe</a>"#,
        );
        assert_eq!(
            found,
            vec![
                Mention::Page {
                    name: "Jane Doe".into(),
                    url: "https://x.com/janedoe/status/123".into()
                },
                Mention::Profile {
                    platform: "x".into(),
                    handle: "janedoe".into(),
                    url: "https://x.com/JaneDoe".into(),
                    name: Some("Jane Doe".into())
                },
                Mention::Profile {
                    platform: "youtube".into(),
                    handle: "@janedoe".into(),
                    url: "https://www.youtube.com/@JaneDoe/videos".into(),
                    name: Some("Jane Doe".into())
                },
            ]
        );
    }

    /// A site behind Cloudflare serves no `mailto:` at all. Without decoding, nobody on it
    /// has an address.
    #[test]
    fn a_cloudflare_protected_address_is_read() {
        let encoded = {
            let key = 0x42u8;
            let mut hex = format!("{key:02x}");
            for b in "jane.doe@tampagov.net".bytes() {
                hex.push_str(&format!("{:02x}", b ^ key));
            }
            hex
        };
        let in_fragment = said(
            "https://www.tampa.gov/staff",
            &format!(r#"<a href="/cdn-cgi/l/email-protection#{encoded}">Jane Doe</a>"#),
        );
        let on_the_span = said(
            "https://www.tampa.gov/staff",
            &format!(
                r#"<a href="/cdn-cgi/l/email-protection"><span class="__cf_email__" data-cfemail="{encoded}">[email&#160;protected]</span></a>"#
            ),
        );
        assert_eq!(
            in_fragment,
            vec![Mention::Email {
                address: "jane.doe@tampagov.net".into(),
                name: Some("Jane Doe".into())
            }]
        );
        assert_eq!(
            on_the_span,
            vec![Mention::Email {
                address: "jane.doe@tampagov.net".into(),
                name: None
            }]
        );
    }

    /// One entry per spelling of a name, carrying what every page linked under it.
    #[test]
    fn the_fold_files_each_page_under_the_names_it_links() {
        let rows = [
            crate::crumbs::scan_page(
                r#"<a href="mailto:lviera@tampagov.net">Luis Viera</a>
                   <a href="/council/viera">Council Member Luis Viera</a>"#,
                crate::crumbs::Carrier::at_address("https://www.tampa.gov/b"),
            ),
            crate::crumbs::scan_page(
                r#"<a href="/council/viera">Luis Viera</a>"#,
                crate::crumbs::Carrier::at_address("https://www.tampa.gov/a"),
            ),
            // Written before mentions were read: counted, not guessed at.
            Recorded {
                page: crate::crumbs::Carrier::at_address("https://www.tampa.gov/old"),
                links: Vec::new(),
                mentions: None,
            },
        ];
        // A fourth page the log holds and the ledger has no row for.
        let named = Named::of("tampa", rows.iter().map(Some).chain([None]));
        assert_eq!(
            (named.pages, named.read),
            (4, 2),
            "the old row and the missing one are not read for names"
        );

        let viera = &named.people["luis viera"];
        assert_eq!(viera.name, "Luis Viera");
        assert_eq!(viera.pages, 2);
        assert_eq!(
            viera.sources["tampa"].examples,
            ["https://www.tampa.gov/a", "https://www.tampa.gov/b"]
        );
        assert_eq!(viera.emails["lviera@tampagov.net"], 1);
        assert_eq!(viera.links["https://www.tampa.gov/council/viera"], 2);
    }
}
