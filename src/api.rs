//! HTTP access to the AcoustID web service through the `host-http` import.
//!
//! This is the only module that touches the network. It builds the form body,
//! posts it to the configured lookup URL, and turns the response into either a
//! decoded lookup or a provider error. The parameter encoding lives in
//! [`crate::url`] and the verdict in [`crate::verdict`].
//!
//! AcoustID answers a rejected request with a non-2xx status and, for some
//! failures, with a 2xx status and an `error` object, so a response body is
//! never believed before it has been inspected.

use hearthsound_plugin::host::http;
use hearthsound_plugin::types::ProviderError;
use hearthsound_plugin::verification_provider::exports::hearthsound::plugin::verification::Fingerprint;

use crate::Config;
use crate::acoustid::LookupResponse;
use crate::url;

/// Wall-clock budget for one AcoustID request, in milliseconds.
///
/// The host clamps this to its own ceiling and to the remaining call deadline.
const REQUEST_TIMEOUT_MS: u32 = 10_000;

/// Delay reported with a rate-limited error, in seconds.
///
/// The service allows three requests per second and sends no `Retry-After`
/// header, so the plugin reports the smallest useful delay.
const RATE_LIMIT_RETRY_SECS: u32 = 1;

/// Looks a fingerprint up in the AcoustID database.
///
/// # Parameters
///
/// - `config`: the instance configuration.
/// - `print`: the compressed fingerprint and the file's duration.
///
/// # Returns
///
/// The decoded lookup, asking for recording metadata.
///
/// # Errors
///
/// Returns the error of a request that did not happen, the mapped HTTP status
/// of a rejected request, the mapped AcoustID error object, and
/// [`ProviderError::Unavailable`] when the body is not a lookup response.
pub(crate) fn lookup(
    config: &Config,
    print: &Fingerprint,
) -> Result<LookupResponse, ProviderError> {
    let body = url::lookup_form(&config.client_key, print.duration_secs, &print.value);
    let response = post(config, body)?;

    decode(response.status, &response.body, &config.base_url)
}

/// Checks that the configured lookup endpoint answers and accepts the key.
///
/// The probe is a shape-valid lookup (see [`url::probe_form`]) whose verdict
/// [`probe_outcome`] decides from the response body, because AcoustID reports
/// a rejected key with HTTP 200 and an `error` object.
///
/// # Parameters
///
/// - `config`: the instance configuration.
///
/// # Returns
///
/// `Ok(())` when the service answered and did not reject the client key.
///
/// # Errors
///
/// Returns the error of a request that did not happen,
/// [`ProviderError::InvalidConfig`] when the service rejected the client key,
/// and [`ProviderError::Unavailable`] for a server-side failure status.
pub(crate) fn probe(config: &Config) -> Result<(), ProviderError> {
    let response = post(config, url::probe_form(&config.client_key))?;

    probe_outcome(response.status, &response.body, &config.base_url)
}

/// Decides one probe response.
///
/// # Parameters
///
/// - `status`: HTTP status of the response.
/// - `body`: raw response body.
/// - `url`: request URL, used in error messages.
///
/// # Returns
///
/// `Ok(())` when the service answered, did not reject the client key, and did
/// not fail server-side. A body that is not JSON counts as an answer: the
/// endpoint is reachable and the lookup path reports the malformed body.
///
/// # Errors
///
/// Returns [`ProviderError::InvalidConfig`] when the service rejected the
/// client key, and [`ProviderError::Unavailable`] for every status of 500 or
/// above.
pub(crate) fn probe_outcome(status: u16, body: &[u8], url: &str) -> Result<(), ProviderError> {
    if status >= 500 {
        return Err(status_error(status, url));
    }

    let Ok(response) = serde_json::from_slice::<LookupResponse>(body) else {
        return Ok(());
    };

    if response.status == "ok" {
        return Ok(());
    }

    match api_error(&response, url) {
        // The key was rejected: exactly what the probe exists to report.
        error @ ProviderError::InvalidConfig(_) => Err(error),
        // Any other error object, a fingerprint complaint included, proves the
        // service accepted the key.
        _ => Ok(()),
    }
}

/// Sends one form-encoded POST to the lookup endpoint.
///
/// # Parameters
///
/// - `config`: the instance configuration; its lookup URL is the target.
/// - `body`: the form body.
///
/// # Returns
///
/// The raw response.
///
/// # Errors
///
/// Returns [`ProviderError::Unavailable`] when the host refuses or cannot
/// complete the request.
fn post(config: &Config, body: String) -> Result<http::Response, ProviderError> {
    let request = http::Request {
        method: "POST".to_string(),
        url: config.base_url.clone(),
        headers: vec![
            ("accept".to_string(), "application/json".to_string()),
            (
                "content-type".to_string(),
                "application/x-www-form-urlencoded".to_string(),
            ),
        ],
        body: Some(body.into_bytes()),
        timeout_ms: Some(REQUEST_TIMEOUT_MS),
    };

    http::send(&request).map_err(|error| http_error(&config.base_url, error))
}

/// Decodes one lookup response.
///
/// # Parameters
///
/// - `status`: HTTP status of the response.
/// - `body`: raw response body.
/// - `url`: request URL, used in error messages.
///
/// # Returns
///
/// The decoded lookup.
///
/// # Errors
///
/// Returns [`ProviderError::RateLimited`] for HTTP 429,
/// [`ProviderError::Unauthorized`] for HTTP 401 and 403,
/// [`ProviderError::Unavailable`] for any other failure status or a body that
/// is not JSON, [`ProviderError::InvalidConfig`] when the body reports an
/// invalid API key, and [`ProviderError::Unavailable`] for any other error
/// object the plugin cannot classify.
pub(crate) fn decode(status: u16, body: &[u8], url: &str) -> Result<LookupResponse, ProviderError> {
    if !(200..300).contains(&status) {
        return Err(status_error(status, url));
    }

    let response: LookupResponse = serde_json::from_slice(body)
        .map_err(|error| ProviderError::Unavailable(format!("{url}: invalid JSON: {error}")))?;

    if response.status == "ok" {
        Ok(response)
    } else {
        Err(api_error(&response, url))
    }
}

/// Maps a failure HTTP status to a provider error.
///
/// # Parameters
///
/// - `status`: the status the service answered with.
/// - `url`: request URL, used in the message.
///
/// # Returns
///
/// Rate-limited for 429, unauthorized for 401 and 403, and unavailable for
/// every other failure status.
fn status_error(status: u16, url: &str) -> ProviderError {
    match status {
        401 | 403 => ProviderError::Unauthorized(format!("{url}: HTTP {status}")),
        429 => ProviderError::RateLimited(RATE_LIMIT_RETRY_SECS),
        _ => ProviderError::Unavailable(format!("{url}: HTTP {status}")),
    }
}

/// Maps the `error` object of a lookup response to a provider error.
///
/// # Parameters
///
/// - `response`: the decoded response, whose status is not `ok`.
/// - `url`: request URL, used in the message.
///
/// # Returns
///
/// Invalid-config when the service reports the API key as unusable, because
/// that is an administrator problem, and unavailable for every other error.
fn api_error(response: &LookupResponse, url: &str) -> ProviderError {
    let message = response
        .error
        .as_ref()
        .map_or("unknown error", |error| error.message.as_str());
    let code = response
        .error
        .as_ref()
        .and_then(|error| error.code)
        .map_or_else(|| "unknown".to_string(), |code| code.to_string());

    if message.to_ascii_lowercase().contains("api key") {
        ProviderError::InvalidConfig(format!("{url}: {message}"))
    } else {
        ProviderError::Unavailable(format!("{url}: AcoustID error {code}: {message}"))
    }
}

/// Maps a `host-http` failure to a provider error.
///
/// # Parameters
///
/// - `url`: request URL, used in the message.
/// - `error`: failure reported by the host.
///
/// # Returns
///
/// [`ProviderError::Unavailable`], because every `host-http` failure is the
/// request not happening rather than an answer from AcoustID.
fn http_error(url: &str, error: http::HttpError) -> ProviderError {
    ProviderError::Unavailable(match error {
        http::HttpError::Denied(message) => format!("{url}: request denied: {message}"),
        http::HttpError::Timeout => format!("{url}: request timed out"),
        http::HttpError::BodyTooLarge => format!("{url}: response body too large"),
        http::HttpError::Network(message) => format!("{url}: network error: {message}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://api.acoustid.org/v2/lookup";

    #[test]
    fn decodes_a_successful_lookup() {
        let response = decode(200, include_str!("fixtures/match.json").as_bytes(), URL).unwrap();

        assert_eq!(response.status, "ok");
        assert_eq!(response.results.len(), 1);
    }

    #[test]
    fn maps_a_rate_limited_status() {
        let error = decode(429, b"", URL).unwrap_err();

        assert!(matches!(
            error,
            ProviderError::RateLimited(RATE_LIMIT_RETRY_SECS)
        ));
    }

    #[test]
    fn maps_unauthorized_statuses() {
        for status in [401, 403] {
            let error = decode(status, b"", URL).unwrap_err();

            assert!(matches!(error, ProviderError::Unauthorized(_)), "{status}");
        }
    }

    #[test]
    fn maps_other_failure_statuses() {
        for status in [400, 404, 500, 503] {
            let error = decode(status, b"", URL).unwrap_err();

            assert!(matches!(error, ProviderError::Unavailable(_)), "{status}");
        }
    }

    #[test]
    fn maps_an_invalid_api_key_to_invalid_config() {
        let error = decode(200, include_str!("fixtures/error.json").as_bytes(), URL).unwrap_err();

        match error {
            ProviderError::InvalidConfig(message) => assert!(message.contains("invalid API key")),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn maps_an_unclassifiable_error_object_to_unavailable() {
        let body = br#"{"status":"error","error":{"code":3,"message":"invalid fingerprint"}}"#;
        let error = decode(200, body, URL).unwrap_err();

        match error {
            ProviderError::Unavailable(message) => {
                assert!(message.contains("invalid fingerprint"), "{message}");
                assert!(message.contains('3'), "{message}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn maps_a_body_that_is_not_json() {
        let error = decode(200, b"<html>", URL).unwrap_err();

        assert!(matches!(error, ProviderError::Unavailable(_)));
    }

    #[test]
    fn reports_a_probe_that_the_service_rejected_the_key_for() {
        let body = br#"{"status":"error","error":{"code":3,"message":"invalid API key"}}"#;
        let error = probe_outcome(200, body, URL).unwrap_err();

        match error {
            ProviderError::InvalidConfig(message) => assert!(message.contains("invalid API key")),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn accepts_a_probe_that_the_service_answered_without_results() {
        let outcome = probe_outcome(200, br#"{"status":"ok","results":[]}"#, URL);

        assert!(outcome.is_ok(), "{outcome:?}");
    }

    #[test]
    fn accepts_a_probe_the_service_rejected_for_its_fingerprint() {
        // A fingerprint complaint proves the key itself was accepted.
        let body = br#"{"status":"error","error":{"code":4,"message":"fingerprint is invalid"}}"#;
        let outcome = probe_outcome(200, body, URL);

        assert!(outcome.is_ok(), "{outcome:?}");
    }

    #[test]
    fn reports_a_probe_that_reached_a_broken_server() {
        let error = probe_outcome(500, b"", URL).unwrap_err();

        assert!(matches!(error, ProviderError::Unavailable(_)));
    }

    #[test]
    fn accepts_a_probe_whose_body_is_not_json() {
        let outcome = probe_outcome(200, b"not json", URL);

        assert!(outcome.is_ok(), "{outcome:?}");
    }
}
