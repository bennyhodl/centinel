//! Resolving a `TARGET` argument to one collected document.
//!
//! `open` and `read` both take the same thing: whatever you had in front of you when you
//! decided to look at a document. That is usually a blob hash something just printed,
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
//!
//! ## Derived blobs are addressable too
//!
//! A derived blob — the markdown an extraction produced, the text a transcription
//! produced — is not an Observation. No server ever served it, so it has no address and
//! never appears in `latest_observations`.
//!
//! It does, however, get **printed**. `open` on a caption track defaults to the
//! transcript and reports that blob's hash; so does `open --derived` on anything. Every
//! one of those hashes used to come back "nothing in the store matches", which is the
//! worst possible answer: the tool printed a handle, and then refused to accept it.
//!
//! So a derived hash resolves to the Observation it was derived *from*, and the caller is
//! told that is what happened. The rule is that anything Centinel prints, Centinel takes
//! back.

use crate::domain::BlobSha;
use crate::prelude::*;
use crate::store::Replay;

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
    /// The resolved source's log, already read.
    ///
    /// Handed on rather than dropped, because both callers' very next question — "what
    /// is the extracted text of this?" — is one this answers, and re-reading the log to
    /// ask it is a cost nobody chose.
    pub replay: Replay,
    /// Set when the target named a **derived** blob rather than the original bytes.
    ///
    /// Callers use it to open the thing that was actually asked for, so that a hash
    /// printed for an extraction round-trips to that same extraction.
    pub matched_derived: Option<BlobSha>,
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

    // One pass per source, not two. This function asks each log two different questions —
    // what was observed, and what was derived — and used to read every one of them twice
    // to do it. On a five-source store that made resolving a single hash eleven full log
    // reads before anything was even opened.
    let mut replays = Vec::with_capacity(sources.len());
    for source in &sources {
        replays.push(ctx.store.replay(source).await?);
    }

    let mut candidates: Vec<(SourceId, Resource, Observation)> = Vec::new();
    for replay in &replays {
        for (resource, obs) in replay.latest_observations() {
            candidates.push((replay.source().clone(), resource, obs));
        }
    }

    let mut matched_derived = None;
    let mut matches: Vec<(SourceId, Resource, Observation)> = Vec::new();

    if looks_like_hash(target) {
        let prefix = target.to_ascii_lowercase();
        matches = candidates
            .iter()
            .filter(|(_, _, obs)| obs.blob_sha.as_str().starts_with(&prefix))
            .cloned()
            .collect();

        // Only when the original bytes did not match. An Observation is the evidentiary
        // anchor and wins any prefix it shares — which, at twelve hex characters, it
        // never will.
        if matches.is_empty() {
            'derived: for replay in &replays {
                for (to, from) in replay.derived_from() {
                    if !to.as_str().starts_with(&prefix) {
                        continue;
                    }
                    let found: Vec<_> = candidates
                        .iter()
                        .filter(|(s, _, obs)| s == replay.source() && &obs.blob_sha == from)
                        .cloned()
                        .collect();
                    if !found.is_empty() {
                        matched_derived = Some(to.clone());
                        matches = found;
                        break 'derived;
                    }
                }
            }
        }
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

    let replay = replays
        .into_iter()
        .find(|r| r.source() == &source)
        .expect("the winning match came from one of these");

    Ok(Resolved {
        source,
        resource,
        observation,
        replay,
        matched_derived,
        other_matches,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Derivation;
    use crate::store::{LogRecord, Store};

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

    // ── resolution ─────────────────────────────────────────────────────────────

    /// A store holding one document and its extracted text.
    ///
    /// Returns `(ctx, original blob, derived blob)` — the two hashes the retrieval
    /// commands print between them.
    async fn corpus(dir: &std::path::Path) -> (Ctx, BlobSha, BlobSha) {
        let store = Store::open(dir.join("store")).await.unwrap();
        let id = SourceId::new("tampa").unwrap();

        let resource = Resource::new(id.clone(), "https://www.tampa.gov/agenda.pdf");
        let obs = store
            .record_observation(
                &resource,
                b"%PDF-1.7 the original bytes",
                jiff::Timestamp::now(),
                Default::default(),
            )
            .await
            .unwrap();

        let derived = store.put_blob(b"# Agenda\n\nextracted text").await.unwrap();
        store
            .append(
                &id,
                &LogRecord::Derivation(Derivation {
                    from_sha: obs.blob_sha.clone(),
                    to_sha: derived.clone(),
                    tool: "pdf-inspector".into(),
                    version: "0.1".into(),
                    model_tier: None,
                    at: jiff::Timestamp::now(),
                    anchors: Vec::new(),
                }),
            )
            .await
            .unwrap();

        (Ctx::new(store), obs.blob_sha, derived)
    }

    #[tokio::test]
    async fn an_original_hash_resolves_by_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, original, _) = corpus(dir.path()).await;

        let found = resolve(&ctx, &original.as_str()[..12], None).await.unwrap();
        assert_eq!(found.observation.blob_sha, original);
        assert_eq!(found.matched_derived, None);
        assert_eq!(
            found.resource.natural_key,
            "https://www.tampa.gov/agenda.pdf"
        );
    }

    /// The defect this module was reopened for: `open` prints a derived blob's hash, and
    /// typing it back said "nothing in the store matches".
    #[tokio::test]
    async fn a_derived_hash_resolves_to_the_document_it_came_from() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, original, derived) = corpus(dir.path()).await;

        let found = resolve(&ctx, &derived.as_str()[..12], None).await.unwrap();
        assert_eq!(
            found.observation.blob_sha, original,
            "a derivation resolves to its input"
        );
        assert_eq!(
            found.matched_derived,
            Some(derived),
            "and the caller is told which half was named"
        );
    }

    /// The whole hash, not just the twelve characters a terminal shows.
    #[tokio::test]
    async fn a_full_derived_hash_resolves_too() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _, derived) = corpus(dir.path()).await;
        let found = resolve(&ctx, derived.as_str(), None).await.unwrap();
        assert_eq!(found.matched_derived, Some(derived));
    }

    #[tokio::test]
    async fn an_address_still_resolves_and_is_not_derived() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, original, _) = corpus(dir.path()).await;

        let found = resolve(&ctx, "agenda.pdf", None).await.unwrap();
        assert_eq!(found.observation.blob_sha, original);
        assert_eq!(found.matched_derived, None);
    }

    #[tokio::test]
    async fn an_unknown_hash_says_what_to_try() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _, _) = corpus(dir.path()).await;

        let err = resolve(&ctx, &"f".repeat(64), None)
            .await
            .map(|_| ())
            .unwrap_err()
            .to_string();
        assert!(err.contains("search"), "{err}");
    }

    /// Restricting to a source must not accidentally widen through the derived lookup.
    #[tokio::test]
    async fn a_derived_hash_is_not_found_under_the_wrong_source() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _, derived) = corpus(dir.path()).await;
        let other = SourceId::new("pinellas").unwrap();
        ctx.store
            .record_observation(
                &Resource::new(other, "https://pinellas.gov/x"),
                b"unrelated",
                jiff::Timestamp::now(),
                Default::default(),
            )
            .await
            .unwrap();

        assert!(
            resolve(&ctx, derived.as_str(), Some("pinellas"))
                .await
                .is_err(),
            "the derivation belongs to tampa"
        );
    }

    /// An exact address never loses to a longer one that merely contains it.
    #[tokio::test]
    async fn an_exact_address_wins_over_one_that_contains_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("store")).await.unwrap();
        let id = SourceId::new("tampa").unwrap();
        for key in [
            "https://tampa.gov/a/b",
            "https://tampa.gov/a",
            "https://tampa.gov/a/c",
        ] {
            store
                .record_observation(
                    &Resource::new(id.clone(), key),
                    key.as_bytes(),
                    jiff::Timestamp::now(),
                    Default::default(),
                )
                .await
                .unwrap();
        }
        let ctx = Ctx::new(store);

        let found = resolve(&ctx, "https://tampa.gov/a", None).await.unwrap();
        assert_eq!(found.resource.natural_key, "https://tampa.gov/a");
        assert_eq!(found.other_matches.len(), 2, "and the rest are reported");
    }
}
