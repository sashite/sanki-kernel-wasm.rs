//! The module's exports (Kernel ABI — Sanki §The module): `memory`, `abi`,
//! `alloc`, `call`, and the arena protocol behind them.
//!
//! The memory is fixed by the linker at 2048 pages (`.cargo/config.toml`); the
//! allocator claims a zero-initialised static of 120 MiB inside it as its
//! arena, the rest holding the data, the stack and the `abi` buffer. `alloc`
//! frees the previous request and response buffers — the reset the ABI
//! describes — and reserves the new request's; `call` runs the request and
//! keeps the response buffer alive until the next `alloc`. Everything a call
//! allocates is dropped when it returns, so no answer depends on a previous
//! request.
//!
//! This is the one module of the crate that holds `unsafe` code: the exports
//! read and write raw memory the consumer addresses, and the buffers live in
//! statics because the module is used from one thread with strictly
//! sequential calls, as the ABI requires.

#![allow(unsafe_code)]

use crate::answer::{answer, oversized, REQUEST_BOUND};
use crate::ops::ABI;
use core::cell::UnsafeCell;
use core::mem::MaybeUninit;

/// The arena the allocator claims: 120 MiB of the 128 MiB memory.
const ARENA_BYTES: usize = 120 * 1024 * 1024;

#[global_allocator]
static TALC: talc::wasm::WasmArenaTalc = {
    static mut MEMORY: [MaybeUninit<u8>; ARENA_BYTES] = [MaybeUninit::uninit(); ARENA_BYTES];
    // SAFETY: `MEMORY` is the allocator's alone; nothing else reads or writes
    // it.
    unsafe { talc::wasm::new_wasm_arena_allocator(core::ptr::addr_of_mut!(MEMORY)) }
};

/// A cell for the single-threaded module's buffers.
struct Slot<T>(UnsafeCell<T>);

// SAFETY: the module runs on one thread and its calls are sequential (Kernel
// ABI — Sanki §WebAssembly profile); no two accesses overlap.
unsafe impl<T> Sync for Slot<T> {}

/// The current request buffer: the bytes `alloc` reserved.
static REQUEST: Slot<Option<Box<[u8]>>> = Slot(UnsafeCell::new(None));
/// The current response buffer: a length prefix and the bytes.
static RESPONSE: Slot<Option<Box<[u8]>>> = Slot(UnsafeCell::new(None));

/// The `abi` buffer: a 4-byte little-endian length and the identifier — the
/// 30 bytes `1a 00 00 00` then `sashite.sanki.kernel-abi/1`, laid out at
/// compile time (the indices are constant and in range by construction).
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
static ABI_BUFFER: [u8; 30] = {
    let text = ABI.as_bytes();
    let mut buffer = [0_u8; 30];
    buffer[0] = text.len() as u8;
    let mut i = 0;
    while i < text.len() {
        buffer[i + 4] = text[i];
        i += 1;
    }
    buffer
};

/// The address of the `abi` buffer, valid for the instance's life.
#[no_mangle]
pub extern "C" fn abi() -> u32 {
    ABI_BUFFER.as_ptr() as u32
}

/// Resets the arena and reserves `len` bytes for a request; `0` when `len`
/// exceeds the request bound.
#[no_mangle]
pub extern "C" fn alloc(len: u32) -> u32 {
    // SAFETY: sequential single-threaded access (see `Slot`).
    unsafe {
        *REQUEST.0.get() = None;
        *RESPONSE.0.get() = None;
    }
    let Ok(len) = usize::try_from(len) else {
        return 0;
    };
    if len > REQUEST_BOUND {
        return 0;
    }
    let buffer = vec![0_u8; len].into_boxed_slice();
    let address = buffer.as_ptr() as u32;
    // SAFETY: as above.
    unsafe {
        *REQUEST.0.get() = Some(buffer);
    }
    address
}

/// Runs the request in `memory[ptr, ptr + len)` and returns the address of
/// the response buffer, or `0` when the module could not place a response.
#[no_mangle]
pub extern "C" fn call(ptr: u32, len: u32) -> u32 {
    let Ok(len) = usize::try_from(len) else {
        return 0;
    };
    if len > REQUEST_BOUND {
        // Reachable only by a consumer bypassing `alloc`: refused before any
        // memory is read (Kernel ABI — Sanki §Bounds).
        return respond(Some(oversized()));
    }
    // SAFETY: sequential single-threaded access (see `Slot`).
    let request = unsafe { (*REQUEST.0.get()).as_deref() };
    let Some(buffer) = request else {
        return 0;
    };
    let base = buffer.as_ptr() as u32;
    let Some(end) = ptr.checked_add(len as u32) else {
        return 0;
    };
    if ptr != base || end > base.wrapping_add(buffer.len() as u32) {
        return 0;
    }
    let body = buffer.get(..len).and_then(answer);
    respond(body)
}

/// Stores a response body behind its length prefix and returns its address.
fn respond(body: Option<Vec<u8>>) -> u32 {
    let Some(body) = body else {
        return 0;
    };
    let Ok(prefix) = u32::try_from(body.len()) else {
        return 0;
    };
    let mut out = Vec::with_capacity(body.len().saturating_add(4));
    out.extend_from_slice(&prefix.to_le_bytes());
    out.extend_from_slice(&body);
    let boxed = out.into_boxed_slice();
    let address = boxed.as_ptr() as u32;
    // SAFETY: sequential single-threaded access (see `Slot`).
    unsafe {
        *RESPONSE.0.get() = Some(boxed);
    }
    address
}
