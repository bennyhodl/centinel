//! The part of a host a seed asks for.
//!
//! A seed is an address, and its path is part of what the operator said. On a host that
//! is one publisher — a city, a campaign — the root and a deep page name the same site.
//! On a host that is a **platform**, they do not: `linktr.ee/TampaDSA` is one account
//! among millions, and `actionnetwork.org/forms/join-the-tampa-dsa-newsletter` is one
//! form. A walk that read those seeds as "the host" would set out to crawl the platform.
//!
//! So the walks that invent their own frontier — following links — stay under the
//! seed's path, the way [`super::listing`] always stayed under its directory. Nothing here
//! names a platform: the rule is the same for every host, and a seed at `/` is the whole
//! host, which is every source collected before this existed.
//!
//! A host's own sitemap is not bounded by this. A sitemap is the publisher's statement of
//! what the site holds, and narrowing it would change what every existing source collects.

use url::Url;

/// One origin, and a path prefix on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    origin: url::Origin,
    /// Always ends in `/`. See [`directory_of`].
    root: String,
}

impl Scope {
    /// What a seed covers: its origin, under the directory its path names.
    ///
    /// `/TampaDSA` covers `/TampaDSA` and everything under `/TampaDSA/`; `/news/index.html`
    /// covers `/news/`; `/` covers the host.
    pub fn of(seed: &Url) -> Self {
        Self {
            origin: seed.origin(),
            root: directory_of(seed).path().to_string(),
        }
    }

    /// Whether an address is inside. Same origin — scheme, host and port — and under the
    /// root, on a segment boundary: `/TampaDSA` does not cover `/TampaDSAx`.
    pub fn contains(&self, url: &Url) -> bool {
        if url.origin() != self.origin {
            return false;
        }
        let path = url.path();
        path.starts_with(&self.root) || path == self.root.trim_end_matches('/')
    }

    /// Whether this is the whole host, which is how a seed at `/` reads.
    pub fn is_whole_host(&self) -> bool {
        self.root == "/"
    }
}

impl std::fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.origin.ascii_serialization(), self.root)
    }
}

/// The directory a URL sits in: `…/Civil/index.html` → `…/Civil/`, and `…/Civil` → `…/Civil/`.
///
/// A listing served from a file path still bounds its walk by the folder, which is what a
/// person clicking the same link would get.
///
/// **The last segment decides, and getting it wrong unbounds the walk.** A path with no
/// trailing slash is ambiguous — `/Civil` is either a file in `/` or a directory served
/// without the redirect — and reading it as a file cut the root back to `/`, so
/// `target.path().starts_with("/")` admitted every link on the page and a request for one
/// folder became a walk of the whole server. That is the failure [`super::listing`]'s docs
/// call structurally impossible, and `investigate`'s own help text encourages the input that
/// triggered it.
///
/// So a segment is only treated as a file when it **looks** like one, by the same rule
/// `check` uses to name a downloaded file: a short alphanumeric extension. The residual
/// case is a directory whose name ends in something extension-shaped — `/2026.budget` — and
/// it degrades to the old behaviour rather than to anything worse, because every candidate
/// is still bound to the seed's host.
pub(super) fn directory_of(url: &Url) -> Url {
    let path = url.path();
    let mut dir = url.clone();
    // Never carried into the root: `…/Civil/?C=N;O=D` is a sort order, and keeping it made
    // the walk's declared root a different address from the one it fetched.
    dir.set_query(None);
    dir.set_fragment(None);
    if path.ends_with('/') {
        return dir;
    }

    let last = path.rsplit('/').next().unwrap_or_default();
    match names_a_file(last) {
        true => {
            let cut = path.rfind('/').map_or(1, |i| i + 1);
            dir.set_path(&path[..cut]);
        }
        false => dir.set_path(&format!("{path}/")),
    }
    dir
}

/// Whether a path segment reads as a filename rather than as a directory name.
fn names_a_file(segment: &str) -> bool {
    let Some((stem, ext)) = segment.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && (1..=5).contains(&ext.len())
        && ext.chars().all(|c| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn covers(seed: &str, url: &str) -> bool {
        Scope::of(&Url::parse(seed).unwrap()).contains(&Url::parse(url).unwrap())
    }

    /// A page on a platform is one account, not the platform.
    #[test]
    fn a_seed_path_on_a_shared_host_covers_that_path_and_nothing_beside_it() {
        let seed = "https://linktr.ee/TampaDSA";
        assert!(covers(seed, "https://linktr.ee/TampaDSA"));
        assert!(covers(seed, "https://linktr.ee/TampaDSA/events"));
        assert!(!covers(seed, "https://linktr.ee/OtherOrg"));
        assert!(
            !covers(seed, "https://linktr.ee/TampaDSAx"),
            "a prefix is a segment, not a string"
        );
        assert!(!covers(seed, "https://linktr.ee/"));
    }

    /// Every source collected before this existed has a seed at `/`, and it still means
    /// the host — exactly the host, by origin.
    #[test]
    fn a_seed_at_the_root_covers_the_host_and_no_other() {
        let scope = Scope::of(&Url::parse("https://www.pinellasdsa.org/").unwrap());
        assert!(scope.is_whole_host());
        assert!(covers(
            "https://www.pinellasdsa.org/",
            "https://www.pinellasdsa.org/blog"
        ));
        assert!(!covers(
            "https://www.pinellasdsa.org/",
            "https://pinellasdsa.org/blog"
        ));
        assert!(!covers(
            "https://www.pinellasdsa.org/",
            "https://medium.com/@x"
        ));
    }

    #[test]
    fn a_file_seed_covers_its_folder() {
        assert!(covers(
            "https://a.gov/news/index.html",
            "https://a.gov/news/2026/x"
        ));
        assert!(!covers(
            "https://a.gov/news/index.html",
            "https://a.gov/about"
        ));
    }
}
