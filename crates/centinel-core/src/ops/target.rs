//! Resolving a `TARGET` argument to one collected document.
//!
//! `open` and `read` both take the same thing: whatever you had in front of you when you
//! decided to look at a document. That is usually the blob hash `search` just printed,
//! sometimes a URL, often only the recognisable end of one.
//!
//! A hash is accepted **by prefix**, git-style, because a citation is only useful if the
//! form printed on screen is the form you can type back. Twelve hex characters over a
//! corpus of this size is not a collision anybody will meet, and when two blobs do share
//! a prefix both are reported rather than one silently winning.
//!
//! Hex is checked before URL text but does not consume the target: a URL can perfectly
//! well contain eight hex characters, so a prefix that matches no blob falls through to
//! address matching instead of failing.

use crate::prelude::*;

/// The shortest hash prefix treated as a hash rather than as URL text.
///
/// Below this the guess is worse than useless — `beef` and `2026` are hex, and both are
/// far more likely to be part of an address than the head of a digest.
const MIN_PREFIX: usize = 8;

/// One resolved document, and what else the target could have meant.
pub struct Resolved {
    pub source: SourceId,
    pub resource: Resource,
    pub observation: Observation,
    /// Other addresses that matched. The first was used.
    pub other_matches: Vec<String>,
}

/// Whether `target` should be read as the head of a blob digest.
pub fn looks_like_hash(target: &str) -> bool {
    target.len() >= MIN_PREFIX && target.chars().all(|c| c.is_ascii_hexdigit())
}

/// Finds the document a `TARGET` names, preferring a blob-hash prefix over address text.
pub async fn resolve(ctx: &Ctx, target: &str, source: Option<&str>) -> anyhow::Result<Resolved> {
    let sources = match source {
        Some(s) => vec![SourceId::new(s.to_string())?],
        None => ctx.store.sources().await?,
    };

    let mut candidates: Vec<(SourceId, Resource, Observation)> = Vec::new();
    for source in &sources {
        for (resource, obs) in ctx.store.latest_observations(source).await? {
            candidates.push((source.clone(), resource, obs));
        }
    }

    let mut matches: Vec<(SourceId, Resource, Observation)> = Vec::new();
    if looks_like_hash(target) {
        let prefix = target.to_ascii_lowercase();
        matches = candidates
            .iter()
            .filter(|(_, _, obs)| obs.blob_sha.as_str().starts_with(&prefix))
            .cloned()
            .collect();
    }
    if matches.is_empty() {
        matches = candidates
            .into_iter()
            .filter(|(_, r, _)| r.natural_key == target || r.natural_key.contains(target))
            .collect();
    }

    anyhow::ensure!(
        !matches.is_empty(),
        "nothing in the store matches `{target}` — try `search` first, or pass a blob hash or a full URL."
    );

    // An exact address never loses to a longer one that merely contains it.
    matches.sort_by_key(|(_, r, _)| (r.natural_key != target, r.natural_key.clone()));
    let (source, resource, observation) = matches.first().cloned().expect("non-empty");
    let other_matches = matches
        .iter()
        .skip(1)
        .take(10)
        .map(|(_, r, _)| r.natural_key.clone())
        .collect();

    Ok(Resolved {
        source,
        resource,
        observation,
        other_matches,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_printed_short_sha_is_read_as_a_hash() {
        // Exactly what `search` and `read` put on screen.
        assert!(looks_like_hash("3f8a1c9d0b7e"));
        assert!(looks_like_hash(&"a".repeat(64)));
    }

    #[test]
    fn short_hex_is_url_text_not_a_hash() {
        // `…/2026/agenda.pdf` must still resolve by address.
        assert!(!looks_like_hash("2026"));
        assert!(!looks_like_hash("beef"));
    }

    #[test]
    fn anything_non_hex_is_an_address() {
        assert!(!looks_like_hash("https://example.gov/agenda.pdf"));
        assert!(!looks_like_hash("3f8a1c9d0b7z"));
    }
}
