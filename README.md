# sashite-sanki-kernel-wasm

The `sanki` reference rule system as a **WebAssembly module** under
[`sashite.sanki.kernel-abi/1`](https://github.com/sashite/web-specs.md/blob/main/nostr/support/kernel-abi-sanki.md)
— the engine and the session kernel behind one deterministic interface, built
for [Sashité](https://sashite.com/).

Under [ADR-0034](https://github.com/sashite/web-specs.md/blob/main/adr/adr-0034-reference-build.md)
a rule system *is* an executable module, named by digest by a signed Rule
System event (kind `3417`): the verdict of a session — and every rule answer on
the way to it — is by definition what the module returns on the session's
public events. This crate is that module for Sashité's `sanki` game: a thin
shim over [`sashite-sanki-engine`](https://crates.io/crates/sashite-sanki-engine)
(the position kernel and the clocks) and
[`sashite-sanki-session`](https://crates.io/crates/sashite-sanki-session) (the
session kernel), exposing the ABI's four exports and answering its thirteen
operations, and computing no rule of its own.

## What the module is

- **Exports** — `memory` (`min = max = 2048` pages, 128 MiB, never grows),
  `abi()`, `alloc(len)` and `call(ptr, len)`; an empty import section, no start
  section, the core profile of the ABI (MVP, mutable globals, sign extension,
  non-trapping float-to-int, multi-value, reference types, bulk memory) and
  nothing else.
- **Requests and responses** — UTF-8 JSON, a request at most 32 MiB, a response
  at most 4 MiB in canonical form (sorted members, no insignificant whitespace,
  minimal escaping), so that two runtimes return byte-identical bytes.
- **Operations** — on the rule system, `describe` and `ggn`; on a position,
  `legal_moves`, `apply` and `classify`; on a session, `natural_state`,
  `verdict_at`, `check` and `select_conclusion`; for the tests, `select`,
  `elapsed`, `charges` and `clock`. Their schemas are the ABI document's.
- **Determinism and totality by construction** — integers only across the
  boundary, no clock, no randomness, no threads, no iteration over hash
  containers, no recursion over input-sized structures (the request's nesting is
  bounded to eight levels and walked iteratively; the engine's clock rollover is
  a loop), a fixed memory, and a response for every request within bound: an
  error is a response, a trap would be a build defect.

The library face of the crate, [`answer`](src/answer.rs), is the module's whole
behaviour as a pure function of bytes — what the tests call natively, and what
anyone can call without a WebAssembly runtime to obtain the same answers.

## Building the module

```sh
cargo build --release --target wasm32-unknown-unknown
# → target/wasm32-unknown-unknown/release/sashite_sanki_kernel_wasm.wasm
```

The **pinned build** — the one a Rule System event names — runs in a container
whose image is pinned by digest, with the toolchain of `rust-toolchain.toml`,
the dependency graph of `Cargo.lock` and the flags of `.cargo/config.toml`,
with source paths remapped so that the bytes do not depend on where the build
ran:

```sh
build/build.sh
# → dist/sanki.wasm, dist/digest.txt (the value of the event's `x` tag)
```

Anyone can rebuild from the same commit and compare the digest — or, simpler,
run the verification below against the published bytes.

## Verifying a build

What *Kernel ABI — Sanki* §Conformance of a build requires, and what CI runs on
every commit:

```sh
cargo test --release          # builds the module, then runs it under wasmi (release: the interpreter is slow unoptimised)
node harness/node/run.mjs     # the same under V8, byte for byte
```

The harness builds the module itself (`cargo build --release --target
wasm32-unknown-unknown`, a no-op when up to date) unless `SANKI_MODULE` names
the module to verify — the published bytes, for instance. The wasm target comes
with `rust-toolchain.toml`; to check the MSRV build of the module, add it to
that toolchain too (`rustup target add wasm32-unknown-unknown --toolchain
1.83.0`).

- `tests/module.rs` — the module's shape (the profile, the exports, the memory,
  no start section, the `abi` buffer), the arena protocol's edges, and the
  **conformance corpus** run through it: `legality` through `apply` and
  `classify`, `selection` through `select`, `time` through `elapsed` and
  `charges`, `clock` through `clock`, `scenarios` through `natural_state` and
  `verdict_at` — each scenario both self-timed and attested, the two required
  to agree. Every response's SHA-256 is recorded under `target/conformance/`.
- `harness/node/run.mjs` — the same corpus under V8 (Chrome's engine, through
  Node), with the same vector mapping, compared with the `wasmi` records: any
  differing byte fails the run.
- `tests/differential.rs` — sessions of random play across the nine variant
  pairings: the module's `legal_moves`, `apply`, `classify`, `natural_state`
  and `verdict_at` equal the native crates' answers, position by position.
- `tests/extreme.rs` — the extreme session of the ABI's §Bounds (600
  half-moves, 32 candidates in every slot, every flood content 256 four-byte
  characters, attested: 27.8 MiB), answered within the memory in about ten
  seconds under `wasmi` and about one under V8.
- `tests/answer.rs` — the errors of the ABI, the claim domain of `check` and
  `select_conclusion`, the `max_step` filter, the value classes and their
  ranges, natively.

The corpus under `tests/conformance/` is a vendored copy of
[Sanki conformance vectors](https://github.com/sashite/web-specs.md/blob/main/nostr/conformance/README.md)
(`scenarios.json` v10); its mapping onto ABI requests is the one that README's
§Through the module states.

## Using the module

A consumer instantiates the module in a sandbox with no host capabilities,
checks `abi()`, then, per request: `alloc(len)`, write the request, `call(ptr,
len)`, read the response buffer (a 4-byte little-endian length, then the bytes)
before the next `alloc`. `tests/common/mod.rs` (Rust, `wasmi`) and
`harness/node/run.mjs` (JavaScript, V8) are two complete consumers of thirty
lines each. What the consumer checks before offering events, and what it does
with the answers, is *Kernel ABI — Sanki* §The consumer's side.

## Status

`0.1.0` — the first module. Its digest is not yet named by a Rule System
event: the first publication (the blob on `blobs.sanki.app` and on a Blossom
server, the signed kind-`3417` event) is the next step of ADR-0034's plan.

## License

Apache-2.0 — see [LICENSE](LICENSE) and [NOTICE](NOTICE).
