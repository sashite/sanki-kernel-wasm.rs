# Changelog

All notable changes to this crate are documented in this file. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

A note on versions: the crate's version is the shim's. A module's identity is
its digest, named by a Rule System event; any change to the module's bytes —
this crate's, the engine's, the session kernel's, a dependency's, the
toolchain's — is a new module, hence a new rule system, whatever the version.

## [0.1.0] — 2026-09-05

The first module under `sashite.sanki.kernel-abi/1`
([ADR-0034](https://github.com/sashite/web-specs.md/blob/main/adr/adr-0034-reference-build.md);
[Kernel ABI — Sanki](https://github.com/sashite/web-specs.md/blob/main/nostr/support/kernel-abi-sanki.md)).

### Added

- **The module.** `memory` (`min = max = 2048` pages), `abi`, `alloc` and
  `call`; an empty import section, no start section; the ABI's core profile
  and nothing else (validated by the harness with exactly that feature set).
  The arena protocol: `alloc` frees the previous request and response buffers
  and reserves the request's, `call` answers and keeps the response until the
  next `alloc`; a length above the bound is refused by `alloc` (`0`) and, when
  `alloc` is bypassed, by `call` (`oversized`) before any memory is read. The
  allocator is `talc` over a 120 MiB arena claimed inside the fixed memory.
- **The thirteen operations** over `sashite-sanki-engine` 0.11.1 and
  `sashite-sanki-session` 0.15.0: `describe` (with `max_step = 300`, the
  parameters in the kernel specification's shape, the nine initial positions),
  `ggn` (the variant's table generated from the module's own geometry, in
  canonical form), `legal_moves`, `apply` (the irreversible bit from
  `engine::apply_ply`), `classify`, `natural_state`, `verdict_at`, `check`,
  `select_conclusion`, and the test-only `select`, `elapsed`, `charges`,
  `clock`.
- **Strictness** (§JSON): a lexical scan before any typed parse refuses a
  duplicate member, nesting beyond eight levels, a fraction, an exponent, a
  leading zero, `-0`, an invalid or unpaired escape, a control character in a
  string, a BOM, trailing bytes; the typed parse refuses unknown and missing
  members and out-of-range values (timings and durations in `[0, 2^40)`,
  counts in `[0, 2^31)`, lowercase 64-hex ids); equal seats and an invalid
  time control are `malformed`, a position with `second` to move
  `invalid_position`. A Ply beyond `max_step`, from another session or from a
  non-player is ignored; a Conclusion's claim is admitted in kind `3425`'s
  domain and judged by the module (`wrong`, never `malformed`).
- **Canonical responses**: sorted members, no whitespace, minimal escaping —
  `serde_json`'s output over ordered maps, byte-identical across runtimes.
- **Verification** as the ABI's §Conformance of a build requires: the corpus
  (v10, every category but `puzzle`) through the module under `wasmi` and
  under V8 with byte-identical responses (247 responses, the extreme session
  included), scenarios both self-timed and attested; a random-session
  differential against the native engine (27 games, 2 083 positions); the
  extreme session (27.8 MiB, attested) completing within the memory; the
  ABI's errors and claim domain natively.
- **The pinned build** (`build/`): `rust:1.96.0-slim-bookworm` by digest, the
  toolchain of `rust-toolchain.toml`, `Cargo.lock`, `wasm32-unknown-unknown`,
  release with `panic = "abort"`, no debug information, paths remapped; the
  digest printed.
