//! Serde types for the AcoustID web service lookup response.
//!
//! Only the members the verification decision needs are modeled: the status,
//! each result's confidence score, and the MusicBrainz recordings attached to
//! a result when the lookup asks for them. Members the plugin does not read,
//! such as release groups and durations, are ignored by serde.
//!
//! Everything here is pure: it never touches the network and never depends on
//! the plugin bindings, so it is unit-tested on the host target.

use serde::Deserialize;

/// A decoded `/v2/lookup` response.
///
/// `status` is `ok` for an accepted lookup and `error` for a rejected request,
/// where [`LookupResponse::error`] carries the reason.
///
/// # Example
///
/// ```text
/// {"status": "ok", "results": [{"id": "...", "score": 0.95, "recordings": []}]}
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct LookupResponse {
    /// `ok` when the request was accepted, `error` otherwise.
    pub status: String,
    /// One entry per AcoustID fingerprint cluster the print matched.
    #[serde(default)]
    pub results: Vec<LookupResult>,
    /// Why the request was rejected; present only on an error.
    #[serde(default)]
    pub error: Option<ApiError>,
}

/// One AcoustID fingerprint cluster the print matched.
#[derive(Debug, Clone, Deserialize)]
pub struct LookupResult {
    /// AcoustID's own cluster id.
    #[serde(default)]
    pub id: String,
    /// How confident AcoustID is that the print belongs to this cluster, from
    /// 0 to 1.
    pub score: f64,
    /// The MusicBrainz recordings assigned to the cluster; empty unless the
    /// lookup asked for `recordings`.
    #[serde(default)]
    pub recordings: Vec<Recording>,
}

/// A MusicBrainz recording assigned to an AcoustID cluster.
#[derive(Debug, Clone, Deserialize)]
pub struct Recording {
    /// MusicBrainz recording id (a UUID).
    #[serde(default)]
    pub id: String,
    /// Recording title, when the lookup requested metadata for it.
    #[serde(default)]
    pub title: Option<String>,
    /// Recording artists in credit order.
    #[serde(default)]
    pub artists: Vec<RecordingArtist>,
}

/// One artist credited on a recording.
#[derive(Debug, Clone, Deserialize)]
pub struct RecordingArtist {
    /// MusicBrainz artist id (a UUID), when the service reported one.
    #[serde(default)]
    pub id: Option<String>,
    /// Artist name.
    #[serde(default)]
    pub name: String,
}

/// The error object of a rejected lookup.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiError {
    /// Numeric error code.
    #[serde(default)]
    pub code: Option<i64>,
    /// Human-readable reason.
    #[serde(default)]
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_lookup_with_recordings() {
        let response: LookupResponse =
            serde_json::from_str(include_str!("fixtures/match.json")).unwrap();

        assert_eq!(response.status, "ok");
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].score, 0.95);
        assert_eq!(response.results[0].recordings.len(), 1);
        assert_eq!(
            response.results[0].recordings[0].title.as_deref(),
            Some("Hello")
        );
        assert_eq!(
            response.results[0].recordings[0].artists[0].name,
            "Hearthsound"
        );
    }

    #[test]
    fn decodes_an_empty_lookup() {
        let response: LookupResponse =
            serde_json::from_str(include_str!("fixtures/empty.json")).unwrap();

        assert_eq!(response.status, "ok");
        assert!(response.results.is_empty());
        assert!(response.error.is_none());
    }

    #[test]
    fn decodes_an_error_response() {
        let response: LookupResponse =
            serde_json::from_str(include_str!("fixtures/error.json")).unwrap();

        assert_eq!(response.status, "error");
        assert!(response.results.is_empty());
        assert_eq!(
            response.error.as_ref().and_then(|error| error.code),
            Some(4)
        );
        assert_eq!(
            response.error.as_ref().map(|error| error.message.as_str()),
            Some("invalid API key")
        );
    }

    #[test]
    fn tolerates_a_result_without_recordings() {
        let response: LookupResponse =
            serde_json::from_str(include_str!("fixtures/no_recordings.json")).unwrap();

        assert_eq!(response.results.len(), 1);
        assert!(response.results[0].recordings.is_empty());
    }
}
