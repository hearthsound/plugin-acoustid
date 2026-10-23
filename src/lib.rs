//! AcoustID verification plugin: a Hearthsound verification provider over the
//! AcoustID fingerprint database.
//!
//! The verification layer hands the plugin the recording the server asked for
//! and a Chromaprint fingerprint of the downloaded file. The plugin looks the
//! fingerprint up at AcoustID and answers whether the file is that recording,
//! a different one, or something the service cannot identify from its
//! fingerprint.
//!
//! The plugin implements `lifecycle` (configuration validation and a health
//! probe) and `verification`, and nothing else. It needs an AcoustID
//! application key, which the administrator registers at
//! <https://acoustid.org/new-application> and stores as the write-only
//! `clientKey` instance setting.
//!
//! A result decides a verdict only when its confidence reaches `minScore`
//! (0.5 by default) and it carries a MusicBrainz recording. A recording
//! matches when its MusicBrainz id is the `mb-recording` id the server
//! expected, or when its normalized title equals the expected title and the
//! expected first artist is among its artists. The second rule matters because
//! one song usually has several MusicBrainz recordings (album, single,
//! remaster), and a correct file often identifies a different one than the
//! server resolved.
//!
//! The pure logic sits in public modules ([`acoustid`], [`normalize`], [`url`],
//! [`verdict`]); the binding-coupled glue - the HTTP transport and the plugin
//! implementation - stays private.
//!
//! # Tests
//!
//! The pure logic is unit-tested on the host target with fixtures written from
//! the documented response shapes:
//!
//! ```text
//! cargo test
//! ```
//!
//! Rustdoc examples in those modules are illustrative `text` blocks rather than
//! doctests: the crate is a `cdylib` for `wasm32-wasip2`, so there is no host
//! library target for a doctest to link against. Every example appears as an
//! assertion in the unit tests of the module that carries it.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use hearthsound_plugin::types::ProviderError;
use hearthsound_plugin::verification_provider::exports::hearthsound::plugin::lifecycle::Guest as LifecycleGuest;
use hearthsound_plugin::verification_provider::exports::hearthsound::plugin::verification::{
    ExpectedTrack, Fingerprint, Guest as VerificationGuest, VerificationResult,
};

pub mod acoustid;
mod api;
pub mod normalize;
pub mod url;
pub mod verdict;

/// The plugin implementation.
struct AcoustIdVerification;

/// Server-scoped configuration of the plugin.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    /// Application API key registered with AcoustID, sent as `client`.
    client_key: String,
    /// Full URL of the lookup endpoint.
    #[serde(default = "default_base_url")]
    base_url: String,
    /// Confidence a lookup result must reach before it can decide a verdict.
    #[serde(default = "default_min_score")]
    min_score: f64,
}

/// Returns the lookup URL used when the instance configuration omits one.
fn default_base_url() -> String {
    url::DEFAULT_BASE_URL.to_string()
}

/// Returns the confidence floor used when the instance configuration omits one.
fn default_min_score() -> f64 {
    0.5
}

/// Reads and checks this instance's configuration.
///
/// # Returns
///
/// The configuration with its lookup URL normalized and its values trimmed.
///
/// # Errors
///
/// Returns [`ProviderError::InvalidConfig`] when the configuration cannot be
/// read or deserialized, when the client key is empty, when the lookup URL is
/// not an absolute `http` or `https` URL, and when the confidence floor is
/// outside 0 to 1.
fn config() -> Result<Config, ProviderError> {
    let config: Config = hearthsound_plugin::config::server_config()?;
    let config = Config {
        client_key: config.client_key.trim().to_string(),
        base_url: url::normalize_base_url(&config.base_url),
        min_score: config.min_score,
    };

    let problem = if config.client_key.is_empty() {
        Some("clientKey must not be empty".to_string())
    } else if !url::is_http_url(&config.base_url) {
        Some(format!(
            "baseUrl must be an http or https URL, got `{}`",
            config.base_url
        ))
    } else if !(0.0..=1.0).contains(&config.min_score) {
        Some(format!(
            "minScore must be between 0 and 1, got `{}`",
            config.min_score
        ))
    } else {
        None
    };

    match problem {
        Some(message) => Err(ProviderError::InvalidConfig(message)),
        None => Ok(config),
    }
}

/// Validates the plugin's server-scoped configuration.
///
/// # Parameters
///
/// - `server_config`: the configuration JSON as the administrator stored it.
///
/// # Returns
///
/// `Ok(())` when the configuration is usable.
///
/// # Errors
///
/// Returns one message per problem: JSON that does not deserialize into the
/// configuration, an empty client key, a lookup URL that is not an absolute
/// `http` or `https` URL, and a confidence floor outside 0 to 1.
fn validate_config(server_config: &str) -> Result<(), Vec<String>> {
    let config: Config = match serde_json::from_str(server_config) {
        Ok(config) => config,
        Err(error) => return Err(vec![format!("server config: {error}")]),
    };

    let mut errors = Vec::new();
    if config.client_key.trim().is_empty() {
        errors.push("clientKey must not be empty".to_string());
    }
    let base_url = url::normalize_base_url(&config.base_url);
    if !url::is_http_url(&base_url) {
        errors.push(format!(
            "baseUrl must be an http or https URL, got `{}`",
            config.base_url
        ));
    }
    if !(0.0..=1.0).contains(&config.min_score) {
        errors.push(format!(
            "minScore must be between 0 and 1, got `{}`",
            config.min_score
        ));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Implements the lifecycle interface.
impl LifecycleGuest for AcoustIdVerification {
    fn validate_config(server_config: String) -> Result<(), Vec<String>> {
        validate_config(&server_config)
    }

    fn health() -> Result<String, ProviderError> {
        let config = config()?;
        api::probe(&config)?;

        Ok(format!("AcoustID answered at {}", config.base_url))
    }
}

/// Implements the verification interface.
impl VerificationGuest for AcoustIdVerification {
    fn verify(
        expected: ExpectedTrack,
        print: Fingerprint,
    ) -> Result<VerificationResult, ProviderError> {
        hearthsound_plugin::debug!(
            "verify `{}` by {:?} with a {} s fingerprint",
            expected.title,
            expected.artists,
            print.duration_secs
        );

        let config = config()?;
        let response = api::lookup(&config, &print)?;
        let result = verdict::decide(&expected, &response, config.min_score);

        hearthsound_plugin::debug!("verdict {:?} at score {}", result.verdict, result.score);

        Ok(result)
    }
}

hearthsound_plugin::export_verification_provider!(AcoustIdVerification);

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = r#""clientKey":"key""#;

    #[test]
    fn accepts_a_minimal_configuration() {
        assert_eq!(validate_config(&format!("{{{KEY}}}")), Ok(()));
    }

    #[test]
    fn accepts_a_test_double_endpoint() {
        assert_eq!(
            validate_config(&format!(
                r#"{{{KEY},"baseUrl":"http://acoustid.internal:8080/v2/lookup/","minScore":0.8}}"#
            )),
            Ok(())
        );
    }

    #[test]
    fn rejects_a_missing_client_key() {
        let errors = validate_config("{}").unwrap_err();

        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("clientKey"), "{}", errors[0]);
    }

    #[test]
    fn rejects_an_empty_client_key() {
        let errors = validate_config(r#"{"clientKey":"  "}"#).unwrap_err();

        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("clientKey"), "{}", errors[0]);
    }

    #[test]
    fn rejects_a_base_url_that_is_not_http() {
        let errors =
            validate_config(&format!(r#"{{{KEY},"baseUrl":"api.acoustid.org"}}"#)).unwrap_err();

        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("baseUrl"), "{}", errors[0]);
    }

    #[test]
    fn rejects_a_confidence_floor_outside_the_range() {
        for value in ["-0.1", "1.5"] {
            let errors = validate_config(&format!(r#"{{{KEY},"minScore":{value}}}"#)).unwrap_err();

            assert_eq!(errors.len(), 1, "{value}");
            assert!(errors[0].contains("minScore"), "{}", errors[0]);
        }
    }

    #[test]
    fn rejects_configuration_of_an_unexpected_shape() {
        let errors = validate_config(r#"{"clientKey":7}"#).unwrap_err();

        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("server config"), "{}", errors[0]);
    }

    #[test]
    fn defaults_match_the_manifest() {
        assert_eq!(default_base_url(), "https://api.acoustid.org/v2/lookup");
        assert_eq!(default_min_score(), 0.5);
    }
}
