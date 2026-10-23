//! The decision that turns an AcoustID lookup into a verification verdict.
//!
//! The rules are the ones the verification interface documents: results below
//! the configured confidence are ignored, and a result decides a verdict only
//! when it carries at least one MusicBrainz recording. A recording matches when
//! its MusicBrainz id is the one the server expected, or when its normalized
//! title equals the expected title and the expected first artist is among its
//! artists. The second rule matters because MusicBrainz keeps several
//! recordings of one song (album, single, remaster), and a correct file often
//! fingerprints to a different recording than the one the server resolved.
//!
//! A recording without a title or artists can only match by id, because a text
//! comparison against absent metadata says nothing. When no recording matched
//! and at least one remaining recording lacks that metadata, the verdict is
//! unknown rather than a mismatch: the service could have identified the file
//! but withheld the fields the comparison needs.
//!
//! Everything here is pure: it never touches the network and never depends on
//! the plugin bindings beyond their plain data types, so it is unit-tested on
//! the host target.

use hearthsound_plugin::types::IdPair;
use hearthsound_plugin::verification_provider::exports::hearthsound::plugin::verification::{
    ExpectedTrack, Verdict, VerificationResult,
};

use crate::acoustid::{LookupResponse, LookupResult, Recording};
use crate::normalize::normalize;

/// Namespace under which identified recordings are reported.
pub const RECORDING_NAMESPACE: &str = "mb-recording";

/// Returns the MusicBrainz recording id the expected track carries, if any.
///
/// # Parameters
///
/// - `expected`: the track the server asked for.
///
/// # Returns
///
/// The value of the first `mb-recording` id, trimmed, or `None` when the track
/// carries no usable one.
///
/// # Example
///
/// ```text
/// expected_recording_id(ExpectedTrack { ids: [id("mb-recording", "abc")], .. }) -> Some("abc")
/// expected_recording_id(ExpectedTrack { ids: [id("isrc", "USQX91300105")], .. }) -> None
/// ```
pub fn expected_recording_id(expected: &ExpectedTrack) -> Option<&str> {
    expected
        .ids
        .iter()
        .find(|id| id.namespace == RECORDING_NAMESPACE)
        .map(|id| id.value.trim())
        .filter(|value| !value.is_empty())
}

/// Decides what a lookup says about the expected track.
///
/// # Parameters
///
/// - `expected`: the track the server asked for.
/// - `response`: the decoded lookup response.
/// - `min_score`: results below this confidence are ignored; values outside 0
///   to 1 and non-finite values are clamped or replaced so a bad configuration
///   cannot change the meaning of a verdict.
///
/// # Returns
///
/// A match when a remaining result carries the expected recording by id or by
/// normalized title and artist, a mismatch when a remaining result carries
/// recordings but none of them matches either way, and unknown when no
/// remaining result carries a recording or when any remaining recording lacks
/// the title or artists a text comparison needs. The score is the deciding
/// result's confidence, and the identified ids are the recordings of the
/// deciding result.
///
/// # Example
///
/// ```text
/// decide(expected, lookup_with_the_expected_recording, 0.5).verdict -> Verdict::Match
/// decide(expected, lookup_below_the_floor, 0.5).verdict -> Verdict::Unknown
/// ```
pub fn decide(
    expected: &ExpectedTrack,
    response: &LookupResponse,
    min_score: f64,
) -> VerificationResult {
    let floor = if min_score.is_finite() {
        min_score.clamp(0.0, 1.0)
    } else {
        0.5
    };
    let expected_id = expected_recording_id(expected);

    let mut ranked: Vec<&LookupResult> = response
        .results
        .iter()
        .filter(|result| result.score >= floor && !result.recordings.is_empty())
        .collect();
    ranked.sort_by(|left, right| right.score.total_cmp(&left.score));

    if let Some(hit) = ranked
        .iter()
        .find(|result| result_matches(result, expected, expected_id))
    {
        return match_result(hit);
    }

    // A recording without a title or artists cannot be compared by text, so a
    // rejection built on it would be a guess. Stay unsure instead.
    if ranked
        .iter()
        .any(|result| result.recordings.iter().any(recording_lacks_metadata))
    {
        return incomplete_result();
    }

    if let Some(best) = ranked.first() {
        return mismatch_result(best);
    }

    unknown_result()
}

/// Returns whether one lookup result identifies the expected recording.
///
/// # Parameters
///
/// - `result`: one remaining lookup result.
/// - `expected`: the track the server asked for.
/// - `expected_id`: the MusicBrainz recording id the track carries, if any.
///
/// # Returns
///
/// `true` when a recording of the result carries `expected_id`, or when a
/// recording's normalized title equals the expected title and the expected
/// first artist is among its artists. Either rule alone is enough: one song
/// often has several MusicBrainz recordings, so a correct file may identify a
/// recording whose id differs from the one the server resolved.
fn result_matches(
    result: &LookupResult,
    expected: &ExpectedTrack,
    expected_id: Option<&str>,
) -> bool {
    result.recordings.iter().any(|recording| {
        expected_id.is_some_and(|id| recording.id.trim().eq_ignore_ascii_case(id))
            || recording_matches_text(recording, expected)
    })
}

/// Returns whether a recording's title and artists match the expected ones.
///
/// # Parameters
///
/// - `recording`: the recording to compare.
/// - `expected`: the track the server asked for.
///
/// # Returns
///
/// `true` when both normalized titles are equal and non-empty and the expected
/// first artist is present, normalizes non-empty, and equals one of the
/// recording's artists; `false` otherwise, including when either side lacks an
/// artist.
fn recording_matches_text(recording: &Recording, expected: &ExpectedTrack) -> bool {
    let Some(title) = recording.title.as_deref() else {
        return false;
    };
    let title = normalize(title);
    if title.is_empty() || title != normalize(&expected.title) {
        return false;
    }

    match expected.artists.first() {
        Some(wanted) => {
            let wanted = normalize(wanted);
            !wanted.is_empty()
                && recording
                    .artists
                    .iter()
                    .any(|found| normalize(&found.name) == wanted)
        }
        None => false,
    }
}

/// Returns whether a recording lacks the metadata a text comparison needs.
///
/// # Parameters
///
/// - `recording`: the recording to inspect.
///
/// # Returns
///
/// `true` when the title is absent or blank, or when the recording credits no
/// artist with a non-blank name. Such a recording can still match by id, but a
/// text comparison against it proves nothing either way.
fn recording_lacks_metadata(recording: &Recording) -> bool {
    let title_missing = recording
        .title
        .as_deref()
        .is_none_or(|title| title.trim().is_empty());
    title_missing
        || recording
            .artists
            .iter()
            .all(|artist| artist.name.trim().is_empty())
}

/// Builds the result of a matching lookup result.
fn match_result(result: &LookupResult) -> VerificationResult {
    VerificationResult {
        verdict: Verdict::Match,
        score: confidence(result),
        detail: None,
        identified: identified(result),
    }
}

/// Builds the result of the best remaining lookup result, which identifies a
/// different recording.
fn mismatch_result(result: &LookupResult) -> VerificationResult {
    VerificationResult {
        verdict: Verdict::Mismatch,
        score: confidence(result),
        detail: Some(describe(result)),
        identified: identified(result),
    }
}

/// Builds the result of a lookup without any usable recording.
fn unknown_result() -> VerificationResult {
    VerificationResult {
        verdict: Verdict::Unknown,
        score: 0.0,
        detail: Some("AcoustID knows no recording for this fingerprint".to_string()),
        identified: Vec::new(),
    }
}

/// Builds the result of a lookup whose recordings lack the metadata a text
/// comparison needs.
fn incomplete_result() -> VerificationResult {
    VerificationResult {
        verdict: Verdict::Unknown,
        score: 0.0,
        detail: Some("AcoustID returned recordings without title or artist metadata".to_string()),
        identified: Vec::new(),
    }
}

/// Returns a lookup result's confidence, clamped to the documented range.
fn confidence(result: &LookupResult) -> f32 {
    result.score.clamp(0.0, 1.0) as f32
}

/// Names the recording a mismatching result identified, for the log.
fn describe(result: &LookupResult) -> String {
    let Some(recording) = result.recordings.first() else {
        return "identified a different recording".to_string();
    };
    let id = recording.id.trim();
    match (recording.title.as_deref(), recording.artists.first()) {
        (Some(title), Some(artist)) => format!("identified `{title}` by {}", artist.name),
        (Some(title), None) => format!("identified `{title}`"),
        (None, Some(artist)) => format!("identified a recording by {}", artist.name),
        (None, None) if !id.is_empty() => format!("identified recording {id}"),
        (None, None) => "identified a different recording".to_string(),
    }
}

/// Collects a result's recording ids as `mb-recording` id pairs.
///
/// Duplicates are dropped case-insensitively, so one recording reported twice
/// appears once.
fn identified(result: &LookupResult) -> Vec<IdPair> {
    let mut pairs: Vec<IdPair> = Vec::new();
    for recording in &result.recordings {
        let value = recording.id.trim();
        if value.is_empty()
            || pairs
                .iter()
                .any(|pair| pair.value.eq_ignore_ascii_case(value))
        {
            continue;
        }
        pairs.push(IdPair {
            namespace: RECORDING_NAMESPACE.to_string(),
            value: value.to_string(),
        });
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acoustid::LookupResponse;

    /// Recording id the matching fixture reports.
    const MATCHED_ID: &str = "11111111-1111-4111-8111-111111111111";

    /// Recording id the mismatching fixture reports.
    const OTHER_ID: &str = "33333333-3333-4333-8333-333333333333";

    /// Recording id the alternate-recording fixture reports.
    const ALTERNATE_ID: &str = "55555555-5555-4555-8555-555555555555";

    /// Recording id the id-only fixture reports.
    const ID_ONLY_ID: &str = "77777777-7777-4777-8777-777777777777";

    fn expected(ids: &[(&str, &str)]) -> ExpectedTrack {
        ExpectedTrack {
            title: "Hello".to_string(),
            artists: vec!["Hearthsound".to_string()],
            album: Some("First Light".to_string()),
            duration_ms: Some(180_000),
            ids: ids
                .iter()
                .map(|(namespace, value)| IdPair {
                    namespace: (*namespace).to_string(),
                    value: (*value).to_string(),
                })
                .collect(),
        }
    }

    fn response(fixture: &str) -> LookupResponse {
        serde_json::from_str(fixture).unwrap()
    }

    #[test]
    fn matches_by_musicbrainz_recording_id() {
        let result = decide(
            &expected(&[("mb-recording", MATCHED_ID)]),
            &response(include_str!("fixtures/match.json")),
            0.5,
        );

        assert_eq!(result.verdict, Verdict::Match);
        assert_eq!(result.score, 0.95);
        assert_eq!(result.identified.len(), 1);
        assert_eq!(result.identified[0].namespace, RECORDING_NAMESPACE);
        assert_eq!(result.identified[0].value, MATCHED_ID);
    }

    #[test]
    fn matches_by_title_and_artist_without_an_expected_id() {
        let mut expected = expected(&[]);
        expected.title = "  h\u{e9}llo ".to_string();
        expected.artists = vec!["HEARTHSOUND".to_string()];

        let result = decide(
            &expected,
            &response(include_str!("fixtures/match.json")),
            0.5,
        );

        assert_eq!(result.verdict, Verdict::Match);
    }

    #[test]
    fn matches_a_recording_under_another_musicbrainz_id() {
        // The server resolved one recording of the song; AcoustID identifies a
        // different one with the same title and artist. The file is still the
        // track the server asked for, because MusicBrainz keeps several
        // recordings of one song.
        let result = decide(
            &expected(&[("mb-recording", MATCHED_ID)]),
            &response(include_str!("fixtures/alternate_recording.json")),
            0.5,
        );

        assert_eq!(result.verdict, Verdict::Match);
        assert_eq!(result.score, 0.88);
        assert_eq!(result.identified.len(), 1);
        assert_eq!(result.identified[0].value, ALTERNATE_ID);
    }

    #[test]
    fn matches_a_recording_by_id_without_metadata() {
        // A lookup that only returns recording ids still decides by id.
        let result = decide(
            &expected(&[("mb-recording", ID_ONLY_ID)]),
            &response(include_str!("fixtures/ids_only.json")),
            0.5,
        );

        assert_eq!(result.verdict, Verdict::Match);
        assert_eq!(result.identified.len(), 1);
        assert_eq!(result.identified[0].value, ID_ONLY_ID);
    }

    #[test]
    fn stays_unknown_when_ids_differ_and_metadata_is_missing() {
        // The id differs and the recording carries no title or artists, so a
        // text comparison cannot reject the file.
        let result = decide(
            &expected(&[("mb-recording", MATCHED_ID)]),
            &response(include_str!("fixtures/ids_only.json")),
            0.5,
        );

        assert_eq!(result.verdict, Verdict::Unknown);
        assert_eq!(result.score, 0.0);
        assert!(result.identified.is_empty());
        let detail = result.detail.unwrap_or_default();
        assert!(detail.contains("metadata"), "{detail}");
    }

    #[test]
    fn one_incomplete_recording_makes_the_verdict_unknown() {
        let result = decide(
            &expected(&[("mb-recording", MATCHED_ID)]),
            &response(include_str!("fixtures/mixed_metadata.json")),
            0.5,
        );

        assert_eq!(result.verdict, Verdict::Unknown);
    }

    #[test]
    fn text_match_accepts_the_expected_artist_among_the_credits() {
        let mut expected = expected(&[]);
        expected.artists = vec!["  GUEST VOICE ".to_string()];

        let result = decide(
            &expected,
            &response(include_str!("fixtures/alternate_recording.json")),
            0.5,
        );

        assert_eq!(result.verdict, Verdict::Match);
    }

    #[test]
    fn a_different_artist_does_not_match() {
        let mut expected = expected(&[]);
        expected.artists = vec!["Somebody Else".to_string()];

        let result = decide(
            &expected,
            &response(include_str!("fixtures/alternate_recording.json")),
            0.5,
        );

        assert_eq!(result.verdict, Verdict::Mismatch);
    }

    #[test]
    fn reports_a_mismatch_with_the_best_score() {
        let result = decide(
            &expected(&[("mb-recording", MATCHED_ID)]),
            &response(include_str!("fixtures/mismatch.json")),
            0.5,
        );

        assert_eq!(result.verdict, Verdict::Mismatch);
        assert_eq!(result.score, 0.72);
        assert_eq!(result.identified.len(), 1);
        assert_eq!(result.identified[0].value, OTHER_ID);
        let detail = result.detail.unwrap_or_default();
        assert!(detail.contains("Goodbye"), "{detail}");
        assert!(detail.contains("Other Band"), "{detail}");
    }

    #[test]
    fn ignores_results_below_the_confidence_floor() {
        let result = decide(
            &expected(&[]),
            &response(include_str!("fixtures/low_score.json")),
            0.5,
        );

        assert_eq!(result.verdict, Verdict::Unknown);
        assert_eq!(result.score, 0.0);
        assert!(result.identified.is_empty());
    }

    #[test]
    fn ignores_results_without_recordings() {
        let result = decide(
            &expected(&[("mb-recording", MATCHED_ID)]),
            &response(include_str!("fixtures/no_recordings.json")),
            0.5,
        );

        assert_eq!(result.verdict, Verdict::Unknown);
    }

    #[test]
    fn answers_unknown_without_results() {
        let result = decide(
            &expected(&[("mb-recording", MATCHED_ID)]),
            &response(include_str!("fixtures/empty.json")),
            0.5,
        );

        assert_eq!(result.verdict, Verdict::Unknown);
        assert_eq!(result.score, 0.0);
        assert!(result.detail.is_some());
        assert!(result.identified.is_empty());
    }

    #[test]
    fn text_match_needs_an_artist_on_both_sides() {
        let mut expected = expected(&[]);
        expected.artists.clear();

        let result = decide(
            &expected,
            &response(include_str!("fixtures/match.json")),
            0.5,
        );

        assert_eq!(result.verdict, Verdict::Mismatch);
    }

    #[test]
    fn expected_recording_id_reads_only_that_namespace() {
        let track = expected(&[("isrc", "USQX91300105"), ("mb-recording", " abc ")]);
        assert_eq!(expected_recording_id(&track), Some("abc"));

        let track = expected(&[("isrc", "USQX91300105"), ("mb-recording", "  ")]);
        assert_eq!(expected_recording_id(&track), None);
    }

    #[test]
    fn a_non_finite_floor_falls_back_to_the_default() {
        let result = decide(
            &expected(&[]),
            &response(include_str!("fixtures/low_score.json")),
            f64::NAN,
        );

        assert_eq!(result.verdict, Verdict::Unknown);
    }

    #[test]
    fn identifies_each_recording_once() {
        let result = decide(
            &expected(&[]),
            &response(include_str!("fixtures/duplicate_recordings.json")),
            0.5,
        );

        assert_eq!(result.verdict, Verdict::Match);
        assert_eq!(result.identified.len(), 1);
        assert_eq!(result.identified[0].value, MATCHED_ID);
    }
}
