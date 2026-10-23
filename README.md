# Hearthsound AcoustID verification plugin

A [Hearthsound](https://github.com/hearthsound) verification plugin backed by
[AcoustID](https://acoustid.org). It compiles to a WebAssembly component that
implements the `verification` provider interface, so a Hearthsound server can
check a downloaded file against the AcoustID fingerprint database before it is
imported and kept.

The server fingerprints the first 120 seconds of a downloaded file with
Chromaprint and passes the compressed print, the file's duration and the
expected track to the plugin. The plugin looks the print up and answers one of
three verdicts:

- `match`: the file is the expected recording.
- `mismatch`: the file is a different recording.
- `unknown`: AcoustID cannot identify the file, or has no confident match.

## Requirements

- Rust 1.99 or newer, with the `wasm32-wasip2` target. The included
  `rust-toolchain.toml` pins both; `rustup toolchain install` fetches them.
- An AcoustID application API key, registered at
  <https://acoustid.org/new-application>. The service allows three requests per
  second per key.

## Build

```console
cargo build --release --locked --target wasm32-wasip2
```

The component lands at
`target/wasm32-wasip2/release/hearthsound_plugin_acoustid.wasm`.

## Check and pack

Install the plugin CLI from the SDK repository:

```console
cargo install --locked --git https://github.com/hearthsound/plugin-sdk --tag v0.3.0 hearthsound-plugin-cli
```

Then validate the manifest and the component:

```console
hsplugin check
```

It prints `ok: app.hearthsound.acoustid 0.1.1 (provides verification)` on
success.

Finally, build the archive you install on a server:

```console
hsplugin pack --out-dir dist
```

`dist/app.hearthsound.acoustid-0.1.1.hsplugin` contains the manifest and the
component.

## Tests

The normalization, form encoding, response decoding and verdict logic run on the
host target, so their unit tests need no WebAssembly toolchain and no network.
The response fixtures are written from the shapes the web service documentation
publishes, not captured from a live account:

```console
cargo test
```

## Configuration

|Instance setting|Default|Meaning|
|---|---|---|
|`clientKey`|none, required|Application API key registered at <https://acoustid.org/new-application>. Write-only: the server stores it sealed and never shows it again.|
|`baseUrl`|`https://api.acoustid.org/v2/lookup`|Full URL of the lookup endpoint. Point it at a test double to keep traffic off AcoustID.|
|`minScore`|`0.5`|Results below this confidence are ignored when deciding a verdict.|

The instance requests a `host-http` rate limit of 3 requests per second with a
burst of 3, which is the ceiling the service documents.

## How a verdict is decided

The lookup asks for recording metadata (`meta=recordings recordingids`), so each
result carries the MusicBrainz recordings AcoustID associates with the print.

1. Results below `minScore` are ignored, and so are results that carry no
   recording.
2. The remaining results are ranked by confidence, highest first.
3. A result matches when one of its recordings carries the `mb-recording` id the
   server expected, or when a recording's normalized title equals the expected
   title and the expected first artist is among that recording's artists. Either
   rule alone is enough, because MusicBrainz keeps several recordings of one
   song (album, single, remaster) and a correct file often identifies a
   different recording than the one the server resolved.
4. The highest-scoring matching result decides `match`, with its confidence as
   the score and its recording ids as the identified ids.
5. Otherwise, the highest-scoring remaining result decides `mismatch`, with its
   confidence, its recording ids, and a detail naming what the fingerprint
   identified.
6. With no remaining result the verdict is `unknown` with a score of 0.

Titles and artist names are compared after NFKD decomposition with combining
marks dropped, lowercasing, and every non-alphanumeric character replaced by a
space, so accents, case and punctuation do not cause a false mismatch.

## Notes

- The plugin never uploads fingerprints: it only calls the lookup operation.
  Submitting fingerprints to the database is a separate, user-facing action the
  plugin does not take.
- The health probe posts the client key alone. The service answers with a
  request error rather than spending a lookup, and any status below 500 counts
  as reachable.
- An HTTP 429 becomes `rate-limited(1)`, HTTP 401 and 403 become `unauthorized`,
  and every other failure status becomes `unavailable`. An error object that
  names the API key becomes `invalid-config`, because an unusable key is an
  administrator problem; every other error object becomes `unavailable` with
  the service's code and message.
- A provider error or an `unknown` verdict does not fail an acquisition: the
  server logs it and keeps the file. Only a `mismatch` quarantines the source.

## Layout

|Path|Contents|
|---|---|
|`Cargo.toml`|Crate metadata and the SDK dependency.|
|`src/lib.rs`|The plugin: configuration, lifecycle and the verification interface.|
|`src/acoustid.rs`|Serde types for the lookup response.|
|`src/api.rs`|The HTTP transport and the mapping of statuses and error objects.|
|`src/normalize.rs`|Title and artist normalization for comparisons.|
|`src/url.rs`|Lookup URL handling and form encoding.|
|`src/verdict.rs`|The lookup-to-verdict decision.|
|`manifest.json`|The plugin manifest the server reads.|

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your
option.
