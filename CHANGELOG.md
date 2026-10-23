# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-10-23

### Added

- The verification interface over the AcoustID web service: a fingerprint
  lookup with recording metadata, a confidence floor, matching by the expected
  MusicBrainz recording id or by normalized title and artist, and identified
  recording ids on the verdict.
- Title and artist normalization that ignores accents, case and punctuation.
- Form encoding of the lookup request, with the client key, the duration and
  the compressed fingerprint sent in the body.
- Mapping of HTTP statuses and AcoustID error objects to provider errors, and a
  health probe of the configured endpoint.
- The manifest with the write-only client key, the lookup URL, the confidence
  floor and the service's rate limit, plus a CI workflow that formats, lints,
  tests, builds, checks and packs the plugin.
- Response fixtures and unit tests for decoding, every verdict branch and the
  error mapping.

[Unreleased]: https://github.com/hearthsound/plugin-acoustid/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/hearthsound/plugin-acoustid/releases/tag/v0.1.0
