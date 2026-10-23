//! URL handling and form encoding for the AcoustID web service.
//!
//! The lookup endpoint takes its parameters in the request body as
//! `application/x-www-form-urlencoded`, so the plugin builds that body itself:
//! the fingerprint is long and URL-safe base64, and every value is
//! percent-encoded exactly once.
//!
//! Everything here is pure: it never touches the network and never depends on
//! the plugin bindings, so it is unit-tested on the host target.

/// Full URL of the public AcoustID lookup endpoint.
pub const DEFAULT_BASE_URL: &str = "https://api.acoustid.org/v2/lookup";

/// The `meta` value the lookup asks for.
///
/// Recording metadata fills the identified recording's title and artists, and
/// recording ids are what the server compares a MusicBrainz id against.
pub const LOOKUP_META: &str = "recordings recordingids";

/// Returns whether `value` is an absolute `http` or `https` URL.
///
/// The check is deliberately structural rather than a full URL parse: it
/// accepts exactly the shapes a lookup endpoint can have (the public service, a
/// self-hosted mirror, or a test double on a private host), and rejects
/// everything a request could not be built from.
///
/// # Parameters
///
/// - `value`: the candidate URL; surrounding whitespace is ignored.
///
/// # Returns
///
/// `true` when `value` starts with `http://` or `https://`, carries a non-empty
/// host, and contains no whitespace.
///
/// # Example
///
/// ```text
/// is_http_url("https://api.acoustid.org/v2/lookup")    -> true
/// is_http_url("http://acoustid.internal:8080/v2/lookup") -> true
/// is_http_url("api.acoustid.org/v2/lookup")            -> false
/// is_http_url("ftp://api.acoustid.org/v2/lookup")      -> false
/// ```
pub fn is_http_url(value: &str) -> bool {
    let value = value.trim();
    let rest = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"));
    let Some(rest) = rest else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    !host.is_empty() && !value.chars().any(char::is_whitespace)
}

/// Normalizes a configured lookup URL.
///
/// Surrounding whitespace and trailing slashes are removed, so an endpoint
/// written with or without a trailing slash becomes the same request target.
///
/// # Parameters
///
/// - `value`: the configured lookup URL.
///
/// # Returns
///
/// The trimmed URL without a trailing slash, or the empty string when `value`
/// is empty or only slashes.
///
/// # Example
///
/// ```text
/// normalize_base_url(" https://api.acoustid.org/v2/lookup/ ") -> "https://api.acoustid.org/v2/lookup"
/// normalize_base_url("") -> ""
/// ```
pub fn normalize_base_url(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
}

/// Builds the form body of a fingerprint lookup.
///
/// # Parameters
///
/// - `client_key`: the application API key, sent as `client`.
/// - `duration_secs`: duration of the whole audio file in seconds, sent as
///   `duration`.
/// - `fingerprint`: the compressed fingerprint, sent as `fingerprint`.
///
/// # Returns
///
/// The `application/x-www-form-urlencoded` body, asking for recording metadata
/// and a JSON response.
///
/// # Example
///
/// ```text
/// lookup_form("key", 120, "AQAAAA")
///   -> "client=key&duration=120&fingerprint=AQAAAA&meta=recordings%20recordingids&format=json"
/// ```
pub fn lookup_form(client_key: &str, duration_secs: u32, fingerprint: &str) -> String {
    format!(
        "client={}&duration={duration_secs}&fingerprint={}&meta={}&format=json",
        encode(client_key),
        encode(fingerprint),
        encode(LOOKUP_META)
    )
}

/// Builds the form body of the health probe.
///
/// The probe sends only the client key and the response format: the service
/// answers a request without a fingerprint with an error status, which is
/// enough to show that the endpoint is reachable without spending a lookup.
///
/// # Parameters
///
/// - `client_key`: the application API key, sent as `client`.
///
/// # Returns
///
/// The `application/x-www-form-urlencoded` body of the probe.
///
/// # Example
///
/// ```text
/// probe_form("key") -> "client=key&format=json"
/// ```
pub fn probe_form(client_key: &str) -> String {
    format!("client={}&format=json", encode(client_key))
}

/// Percent-encodes one form value.
///
/// Every byte outside the unreserved set of RFC 3986 is encoded, including the
/// space, so the fingerprint's punctuation and any key's characters survive the
/// form parser unchanged.
///
/// # Parameters
///
/// - `value`: the raw value.
///
/// # Returns
///
/// The encoded value.
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(char::from(byte));
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_absolute_http_urls() {
        assert!(is_http_url("https://api.acoustid.org/v2/lookup"));
        assert!(is_http_url("http://acoustid.internal:8080/v2/lookup"));
        assert!(is_http_url(" https://api.acoustid.org "));
    }

    #[test]
    fn rejects_urls_a_request_cannot_be_built_from() {
        assert!(!is_http_url(""));
        assert!(!is_http_url("api.acoustid.org/v2/lookup"));
        assert!(!is_http_url("ftp://api.acoustid.org/v2/lookup"));
        assert!(!is_http_url("https://"));
        assert!(!is_http_url("https://api.acoustid.org/v2 lookup"));
    }

    #[test]
    fn removes_a_trailing_slash_and_surrounding_space() {
        assert_eq!(
            normalize_base_url(" https://api.acoustid.org/v2/lookup/ "),
            "https://api.acoustid.org/v2/lookup"
        );
        assert_eq!(
            normalize_base_url("https://api.acoustid.org//"),
            "https://api.acoustid.org"
        );
        assert_eq!(normalize_base_url(""), "");
    }

    #[test]
    fn builds_a_lookup_body() {
        assert_eq!(
            lookup_form("key", 120, "AQAAAA"),
            "client=key&duration=120&fingerprint=AQAAAA&meta=recordings%20recordingids&format=json"
        );
    }

    #[test]
    fn encodes_form_values() {
        assert_eq!(encode("a b/c+d"), "a%20b%2Fc%2Bd");
        assert_eq!(encode("AQAA-A_b.c~"), "AQAA-A_b.c~");
        assert_eq!(encode(""), "");
    }

    #[test]
    fn builds_the_probe_body() {
        assert_eq!(probe_form("key"), "client=key&format=json");
    }
}
