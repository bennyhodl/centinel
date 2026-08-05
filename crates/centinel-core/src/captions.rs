//! YouTube caption tracks, `json3` → timestamped text.
//!
//! `json3` is archived rather than `srt`/`vtt` because it is the only format carrying
//! **per-word** offsets; the others are lossy conversions that collapse to cue timing.
//! This module is the derivation that turns that archive into something searchable,
//! producing the same timestamped-paragraph shape as [`crate::transcribe`] so a passage
//! from a caption track and one from Whisper are indistinguishable downstream.
//!
//! ## The format, as actually served
//!
//! Measured on a real 2h 55m council recording — 8,502 events:
//!
//! ```json
//! { "wireMagic": "pb3", "pens": [], "wsWinStyles": [], "wpWinPositions": [],
//!   "events": [
//!     { "tStartMs": 28560, "dDurationMs": 3010, "wWinId": 1,
//!       "segs": [ {"utf8": "the motion to adopt the zoning hearing", "acAsrConf": 0} ] },
//!     { "tStartMs": 31189, "dDurationMs": 10, "aAppend": 1, "segs": [ {"utf8": "\n"} ] }
//!   ] }
//! ```
//!
//! Half the events (4,250 of 8,502) are `aAppend` newline markers driving the scrolling
//! caption window, and one carries no `segs` at all. They are whitespace once joined, so
//! dropping empties handles both without special-casing a private display protocol.
//!
//! ## What these captions are, and are not
//!
//! On the measured channel: **0 manual tracks, 157 automatic ones** — machine
//! translations off a single English ASR pass, exactly the asymmetry the research
//! predicted for municipal channels. So this text has no speaker labels, unreliable
//! punctuation across turns, and degrades on the proper nouns a civic index is built
//! around — street names, ordinance numbers, surnames. It is worth indexing because it
//! exists today and costs nothing; it is **not** a reason to skip local transcription.
//! `>>` is YouTube's speaker-change marker and is preserved: it is the only turn
//! structure an auto-caption track carries, and §8 keeps diarization out of scope.

use serde::{Deserialize, Serialize};

/// Markers that identify a `json3` document among ordinary JSON blobs.
///
/// **Two of them, because JSON object order is not a contract.** YouTube emits
/// `wireMagic` first, but any re-serialisation that sorts keys — `serde_json`'s own
/// default map does — puts the `events` array ahead of it, and a 3 MB array would push
/// the magic string far past any head window. `tStartMs` appears inside the first event
/// under that ordering, so between them one always lands early.
const MARKERS: &[&str] = &["wireMagic", "tStartMs"];

/// How much of a blob the sniff reads. Generous enough to survive a reordered document's
/// preamble, small enough that this stays free on a path that runs per blob.
pub const SNIFF_BYTES: usize = 4096;

/// One caption cue.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cue {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// A parsed caption track.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Captions {
    pub cues: Vec<Cue>,
    /// Events in the source document, including the display-protocol ones.
    pub events: usize,
    /// Events that carried no text — `aAppend` newlines and window definitions.
    pub empty_events: usize,
}

impl Captions {
    /// Renders as timestamped markdown, identical in shape to a Whisper transcript.
    pub fn to_markdown(&self, title: Option<&str>) -> String {
        crate::transcribe::render_markdown(
            self.cues
                .iter()
                .map(|c| (c.start_ms, c.end_ms, c.text.as_str())),
            title,
        )
    }

    /// Time ranges for [`crate::domain::Derivation::anchors`].
    pub fn anchors(&self) -> Vec<crate::domain::Anchor> {
        self.cues
            .iter()
            .map(|c| crate::domain::Anchor::TimeRange {
                start_ms: c.start_ms.max(0) as u64,
                end_ms: c.end_ms.max(0) as u64,
            })
            .collect()
    }

    pub fn duration_ms(&self) -> i64 {
        self.cues.last().map(|c| c.end_ms).unwrap_or(0)
    }
}

/// Cheap recognition of a `json3` track, for content sniffing.
///
/// Looks only at the head, because this runs per blob on a path that must stay fast —
/// parsing a 3 MB caption document to find out whether it is a caption document would
/// make `extract` quadratic in exactly the corpus this exists to serve.
pub fn looks_like_json3(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(SNIFF_BYTES)];
    MARKERS.iter().any(|marker| {
        let needle = marker.as_bytes();
        head.len() >= needle.len() && head.windows(needle.len()).any(|w| w == needle)
    })
}

/// Parses a `json3` caption track.
pub fn parse_json3(bytes: &[u8]) -> anyhow::Result<Captions> {
    let doc: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| anyhow::anyhow!("not valid JSON: {e}"))?;

    let events = doc
        .get("events")
        .and_then(|e| e.as_array())
        .ok_or_else(|| anyhow::anyhow!("no `events` array — this is not a json3 caption track"))?;

    let mut cues = Vec::new();
    let mut empty_events = 0usize;

    for event in events {
        let start_ms = event.get("tStartMs").and_then(|v| v.as_i64()).unwrap_or(0);
        let duration = event
            .get("dDurationMs")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let text: String = event
            .get("segs")
            .and_then(|s| s.as_array())
            .map(|segs| {
                segs.iter()
                    .filter_map(|s| s.get("utf8").and_then(|u| u.as_str()))
                    .collect()
            })
            .unwrap_or_default();

        // Collapses the newline-only `aAppend` events and the segs-less window
        // definition into one case: no text is no cue.
        let text = text.trim();
        if text.is_empty() {
            empty_events += 1;
            continue;
        }

        cues.push(Cue {
            start_ms,
            end_ms: start_ms + duration.max(0),
            text: text.to_string(),
        });
    }

    anyhow::ensure!(
        !cues.is_empty(),
        "caption track has {} events but no text",
        events.len()
    );

    Ok(Captions {
        cues,
        events: events.len(),
        empty_events,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The verbatim shape of a real track's opening events.
    fn json3() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "wireMagic": "pb3",
            "pens": [{}],
            "wsWinStyles": [{}],
            "events": [
                { "tStartMs": 0, "wWinId": 1, "id": 1, "wpWinPosId": 1 },
                { "tStartMs": 6799, "dDurationMs": 7161, "wWinId": 1, "segs": [
                    {"utf8": "comments.", "acAsrConf": 0},
                    {"utf8": " Amen.", "tOffsetMs": 1201, "acAsrConf": 0}
                ]},
                { "tStartMs": 10950, "dDurationMs": 3010, "aAppend": 1,
                  "segs": [{"utf8": "\n"}] },
                { "tStartMs": 28560, "dDurationMs": 3010, "wWinId": 1, "segs": [
                    {"utf8": "the motion to adopt the zoning hearing", "acAsrConf": 0}
                ]}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn a_json3_track_is_recognised_from_its_head() {
        assert!(looks_like_json3(&json3()));
        assert!(!looks_like_json3(br#"{"id":"abc","title":"a video"}"#));
        assert!(!looks_like_json3(b""));
        assert!(!looks_like_json3(b"%PDF-1.7"));
    }

    /// JSON object order is not a contract. YouTube puts `wireMagic` first; a
    /// key-sorting re-serialisation puts the `events` array first and pushes the magic
    /// string megabytes down. Either ordering must still be recognised.
    #[test]
    fn recognition_does_not_depend_on_key_order() {
        let magic_first = br#"{"wireMagic":"pb3","events":[{"tStartMs":0,"segs":[]}]}"#;
        let events_first = br#"{"events":[{"tStartMs":0,"segs":[]}],"wireMagic":"pb3"}"#;
        assert!(looks_like_json3(magic_first));
        assert!(looks_like_json3(events_first));

        // A big leading array must not hide the evidence — `tStartMs` is inside it.
        let mut bulky = br#"{"events":[{"tStartMs":6799,"segs":[{"utf8":""#.to_vec();
        bulky.extend(std::iter::repeat_n(b'x', 100_000));
        bulky.extend_from_slice(br#""}]}],"wireMagic":"pb3"}"#);
        assert!(looks_like_json3(&bulky));
    }

    /// The metadata document sits in the same store and is also `application/json`.
    /// Mistaking it for a caption track would replace a video's metadata with an error.
    #[test]
    fn a_video_metadata_document_is_not_mistaken_for_captions() {
        let metadata = serde_json::to_vec(&serde_json::json!({
            "id": "_vdkiPxxyTM",
            "title": "Charter Review Advisory Commission 07/28/2026",
            "duration": 10635,
            "automatic_captions": { "en": [{ "ext": "json3" }] },
        }))
        .unwrap();
        assert!(!looks_like_json3(&metadata));
    }

    /// Half the events in a real track are display-protocol noise. They must vanish
    /// without being special-cased as a private protocol we would have to track.
    #[test]
    fn append_markers_and_window_definitions_produce_no_cues() {
        let caps = parse_json3(&json3()).unwrap();
        assert_eq!(caps.events, 4);
        assert_eq!(caps.cues.len(), 2);
        assert_eq!(
            caps.empty_events, 2,
            "one newline marker, one window definition"
        );
    }

    #[test]
    fn segments_join_into_one_cue_with_a_time_range() {
        let caps = parse_json3(&json3()).unwrap();
        assert_eq!(caps.cues[0].text, "comments. Amen.");
        assert_eq!(caps.cues[0].start_ms, 6799);
        assert_eq!(caps.cues[0].end_ms, 6799 + 7161);
        assert_eq!(caps.duration_ms(), 28560 + 3010);
    }

    #[test]
    fn anchors_mirror_the_cues() {
        let caps = parse_json3(&json3()).unwrap();
        assert_eq!(caps.anchors().len(), caps.cues.len());
        assert_eq!(
            caps.anchors()[0],
            crate::domain::Anchor::TimeRange {
                start_ms: 6799,
                end_ms: 13960
            }
        );
    }

    /// §6.4 again: the offset has to be in the prose, or chunking loses the citation.
    #[test]
    fn the_rendered_markdown_opens_every_paragraph_with_an_offset() {
        let caps = parse_json3(&json3()).unwrap();
        let md = caps.to_markdown(Some("Charter Review Advisory Commission"));

        assert!(md.starts_with("# Charter Review Advisory Commission\n\n"));
        for line in md.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
            assert!(line.starts_with('['), "untimestamped line: {line}");
        }

        // 14.6 s of silence separates these two cues, so they are cited separately
        // rather than sharing the first one's offset.
        assert!(md.contains("[00:00:06] comments. Amen."), "{md}");
        assert!(
            md.contains("[00:00:28] the motion to adopt the zoning hearing"),
            "{md}"
        );
    }

    /// Auto-captions routinely have no sentence punctuation at all. Without a hard cap
    /// the whole 3-hour meeting would render as a single unbroken paragraph.
    #[test]
    fn unpunctuated_captions_still_break_into_paragraphs() {
        let cues: Vec<(i64, i64, &str)> = (0..400)
            .map(|i| {
                let start = i as i64 * 4_000;
                (
                    start,
                    start + 4_000,
                    "and then the commission discussed the matter",
                )
            })
            .collect();
        let md = crate::transcribe::render_markdown(cues, None);

        let paragraphs: Vec<_> = md.lines().filter(|l| !l.trim().is_empty()).collect();
        assert!(
            paragraphs.len() > 10,
            "expected many paragraphs, got {}",
            paragraphs.len()
        );
        assert!(paragraphs.iter().all(|p| p.starts_with('[')));
        // Distinct offsets, or the citations would all point at the start.
        assert_ne!(paragraphs[0], paragraphs[1]);
    }

    #[test]
    fn a_document_that_is_not_a_caption_track_fails_with_a_reason() {
        let err = parse_json3(br#"{"wireMagic":"pb3"}"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("events"), "{err}");
        assert!(parse_json3(b"not json").is_err());
    }
}
