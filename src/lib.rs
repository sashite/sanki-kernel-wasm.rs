//! The `sanki` reference rule system as a WebAssembly module under
//! **`sashite.sanki.kernel-abi/1`** — [Kernel ABI — Sanki](https://github.com/sashite/web-specs.md/blob/main/nostr/support/kernel-abi-sanki.md).
//!
//! Under [ADR-0034](https://github.com/sashite/web-specs.md/blob/main/adr/adr-0034-reference-build.md)
//! a rule system *is* an executable module, named by digest by a signed Rule
//! System event (kind `3417`): the verdict of a session, and every rule answer
//! on the way to it, is by definition what the module returns on the
//! session's public events. This crate is that module for Sashité's `sanki`
//! game: a thin shim over [`sashite_sanki_engine`] (the position kernel and
//! the clocks) and [`sashite_sanki_session`] (the session kernel) that
//! exposes the ABI's four exports and answers its thirteen operations, and
//! computes no rule of its own.
//!
//! The crate has two faces:
//!
//! - **the module** — built for `wasm32-unknown-unknown`, it exports `memory`
//!   (`min = max = 2048` pages), `abi`, `alloc` and `call` (the `exports`
//!   module, compiled for that target only);
//! - **the library** — [`answer()`] is the module's whole behaviour as a pure
//!   function of bytes, callable natively by the tests and by anyone who
//!   wants the same answers without a WebAssembly runtime.
//!
//! Determinism and totality are by construction (Kernel ABI — Sanki
//! §Determinism and totality): integers only across the boundary and in the
//! kernel, no clock, no randomness, no threads, no iteration over hash
//! containers, no recursion over input-sized structures, a fixed memory, and a
//! response for every request within bound. A build is verified before
//! publication as the ABI's §Conformance of a build requires — the corpus in
//! two runtimes with byte-identical responses, a differential against the
//! native engine, the extreme session — and the verified bytes are what a
//! Rule System event names.

pub mod answer;
pub mod encode;
#[cfg(target_arch = "wasm32")]
pub mod exports;
pub mod fault;
pub mod json;
pub mod ops;
pub mod request;

pub use answer::{answer, evaluate, REQUEST_BOUND, RESPONSE_BOUND};
pub use ops::{ABI, GAME, KERNEL, MAX_STEP};
