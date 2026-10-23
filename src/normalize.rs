//! Title and artist normalization for AcoustID comparisons.
//!
//! AcoustID reports MusicBrainz titles and artist names, which spell accents
//! and punctuation differently from the tags the server asked about. Both
//! sides of a comparison go through [`normalize`], so "Bjork", "BJORK" and
//! "Bjork!" all compare equal.
//!
//! Everything here is pure: it never touches the network and never depends on
//! the plugin bindings, so it is unit-tested on the host target.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::canonical_combining_class;

/// Normalizes a title or artist name for comparison.
///
/// The value is decomposed (NFKD), combining marks are dropped, letters are
/// lowercased, every other character becomes a space, runs of spaces collapse
/// and the result is trimmed.
///
/// # Parameters
///
/// - `value`: the title, artist name, or other text to normalize.
///
/// # Returns
///
/// The normalized text; empty when the value carries no letters or digits.
///
/// # Example
///
/// ```text
/// normalize("Hello,   World!") -> "hello world"
/// normalize("AC/DC") -> "ac dc"
/// normalize("---") -> ""
/// ```
pub fn normalize(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.nfkd() {
        if canonical_combining_class(character) != 0 {
            continue;
        }
        if character.is_alphanumeric() {
            out.extend(character.to_lowercase());
        } else {
            out.push(' ');
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_accents() {
        // "Bjork" with an umlaut, "Joga" with an acute accent, and "Hello" with
        // an acute accent on the e, written with escapes so this file stays
        // ASCII.
        assert_eq!(normalize("Bj\u{f6}rk"), "bjork");
        assert_eq!(normalize("J\u{f3}ga"), "joga");
        assert_eq!(normalize("H\u{e9}llo"), normalize("Hello"));
    }

    #[test]
    fn ignores_case() {
        assert_eq!(normalize("HELLO"), "hello");
        assert_eq!(normalize("Hearthsound"), normalize("HEARTHSOUND"));
    }

    #[test]
    fn replaces_punctuation_with_spaces() {
        assert_eq!(normalize("Hello, World!"), "hello world");
        assert_eq!(normalize("AC/DC - Back in Black"), "ac dc back in black");
        assert_eq!(normalize("don't stop"), "don t stop");
    }

    #[test]
    fn collapses_whitespace_and_trims() {
        assert_eq!(normalize("  Hello,   World! "), "hello world");
        assert_eq!(normalize("\tHello\nWorld\t"), "hello world");
    }

    #[test]
    fn empty_text_normalizes_to_nothing() {
        assert_eq!(normalize(""), "");
        assert_eq!(normalize("   "), "");
        assert_eq!(normalize("---"), "");
    }
}
