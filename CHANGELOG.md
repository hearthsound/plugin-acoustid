# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- The lookup asks for `meta=recordings` only. Asking for `recordingids` as
  well makes the service return bare recording ids without titles or artists,
  which left the title and artist rule unable to match.
- A recording without a title or artists can only match by id. When no
  recording matched and at least one remaining recording lacks that metadata,
  the verdict is `unknown` instead of `mismatch`.

## [0.1.1] - 2026-10-23

### Fixed

- A file whose fingerprint identifies a different MusicBrainz recording of the
  same title and artist is now accepted. One song usually has several
  recordings (album, single, remaster), so the title and artist rule runs
  alongside the recording-id comparison instead of only when the server
  expected no id.
- The title and artist rule now accepts the expected first artist anywhere in a
  recording's credits, not only as its first credit.

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

[Unreleased]: https://github.com/hearthsound/plugin-acoustid/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/hearthsound/plugin-acoustid/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/hearthsound/plugin-acoustid/releases/tag/v0.1.0
