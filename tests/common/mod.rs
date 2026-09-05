//! The test harness: the module under `wasmi`, driven through the ABI's arena
//! protocol exactly as a consumer drives it, and the corpus mapping of
//! *Sanki conformance vectors* §Through the module.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;
use wasmi::{Config, Engine, Instance, Linker, Memory, Module, Store, TypedFunc};

/// Where the built module is: `SANKI_MODULE`, or the release build of this
/// crate for `wasm32-unknown-unknown`.
pub fn module_path() -> PathBuf {
    if let Ok(path) = std::env::var("SANKI_MODULE") {
        return PathBuf::from(path);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/wasm32-unknown-unknown/release/sashite_sanki_kernel_wasm.wasm")
}

/// The module's bytes. Unless `SANKI_MODULE` names a module, the release
/// build for `wasm32-unknown-unknown` is run first (once per test binary; a
/// no-op when up to date), so that `cargo test --release` verifies the module
/// the sources define, never a stale one.
pub fn module_bytes() -> Vec<u8> {
    static BUILT: std::sync::Once = std::sync::Once::new();
    if std::env::var_os("SANKI_MODULE").is_none() {
        BUILT.call_once(|| {
            let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
            let status = std::process::Command::new(cargo)
                .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .status()
                .expect("cargo runs");
            assert!(
                status.success(),
                "the module builds for wasm32-unknown-unknown"
            );
        });
    }
    let path = module_path();
    std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "the module is not at {} ({error}); build it with `cargo build --release --target wasm32-unknown-unknown`, or set SANKI_MODULE",
            path.display()
        )
    })
}

/// The exact WebAssembly profile of Kernel ABI — Sanki §WebAssembly profile,
/// and nothing more: a module using any other feature fails to compile here.
pub fn profile() -> Config {
    let mut config = Config::default();
    config
        .wasm_mutable_global(true)
        .wasm_sign_extension(true)
        .wasm_saturating_float_to_int(true)
        .wasm_multi_value(true)
        .wasm_bulk_memory(true)
        .wasm_reference_types(true)
        .wasm_multi_memory(false)
        .wasm_tail_call(false)
        .wasm_extended_const(false)
        .wasm_custom_page_sizes(false)
        .wasm_memory64(false)
        .wasm_wide_arithmetic(false);
    config
}

/// The module instantiated in `wasmi`.
pub struct Runtime {
    store: Store<()>,
    memory: Memory,
    alloc: TypedFunc<u32, u32>,
    call: TypedFunc<(u32, u32), u32>,
    abi: TypedFunc<(), u32>,
}

impl Runtime {
    /// Instantiates the module, checking its shape as a consumer does.
    pub fn new(bytes: &[u8]) -> Self {
        let engine = Engine::new(&profile());
        let module = Module::new(&engine, bytes).expect("the module validates under the profile");
        assert_eq!(module.imports().count(), 0, "the import section is empty");
        let linker: Linker<()> = Linker::new(&engine);
        let mut store = Store::new(&engine, ());
        let instance: Instance = linker
            .instantiate_and_start(&mut store, &module)
            .expect("the module instantiates");
        let memory = instance
            .get_memory(&store, "memory")
            .expect("a `memory` export");
        let ty = memory.ty(&store);
        assert_eq!(ty.minimum(), 2048, "memory min = 2048 pages");
        assert_eq!(ty.maximum(), Some(2048), "memory max = 2048 pages");
        let alloc = instance
            .get_typed_func::<u32, u32>(&store, "alloc")
            .expect("an `alloc` export");
        let call = instance
            .get_typed_func::<(u32, u32), u32>(&store, "call")
            .expect("a `call` export");
        let abi = instance
            .get_typed_func::<(), u32>(&store, "abi")
            .expect("an `abi` export");
        Self {
            store,
            memory,
            alloc,
            call,
            abi,
        }
    }

    /// Reads a buffer (a 4-byte little-endian length, then the bytes).
    fn buffer(&self, address: u32) -> Vec<u8> {
        let data = self.memory.data(&self.store);
        let start = address as usize;
        let len = u32::from_le_bytes(data[start..start + 4].try_into().unwrap()) as usize;
        data[start + 4..start + 4 + len].to_vec()
    }

    /// The ABI identifier the module states.
    pub fn abi(&mut self) -> String {
        let address = self.abi.call(&mut self.store, ()).expect("abi() returns");
        String::from_utf8(self.buffer(address)).expect("UTF-8")
    }

    /// One request through the arena protocol: `None` when `alloc` refused
    /// the length or `call` produced no buffer.
    pub fn request(&mut self, bytes: &[u8]) -> Option<Vec<u8>> {
        let len = u32::try_from(bytes.len()).ok()?;
        let address = self
            .alloc
            .call(&mut self.store, len)
            .expect("alloc() returns");
        if address == 0 {
            return None;
        }
        self.memory
            .write(&mut self.store, address as usize, bytes)
            .expect("the reservation holds the request");
        let response = self
            .call
            .call(&mut self.store, (address, len))
            .expect("call() returns without trapping");
        if response == 0 {
            return None;
        }
        Some(self.buffer(response))
    }

    /// A JSON request, answered as JSON.
    pub fn ask(&mut self, request: &Value) -> Value {
        let bytes = serde_json::to_vec(request).unwrap();
        let response = self.request(&bytes).expect("an answer");
        serde_json::from_slice(&response).expect("the response is JSON")
    }

    /// A JSON request, answered as raw bytes.
    pub fn ask_bytes(&mut self, request: &Value) -> Vec<u8> {
        let bytes = serde_json::to_vec(request).unwrap();
        self.request(&bytes).expect("an answer")
    }
}

/// The digest of a response, as the two-runtime comparison records it.
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// The record of a run: vector id → response digest, written for the
/// comparison with the other runtime.
#[derive(Default)]
pub struct Record(pub BTreeMap<String, String>);

impl Record {
    pub fn add(&mut self, id: &str, response: &[u8]) {
        self.0.insert(id.to_owned(), digest(response));
    }

    pub fn write(&self, name: &str) {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/conformance");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, serde_json::to_string_pretty(&self.0).unwrap()).unwrap();
    }
}

// ---- the corpus mapping ----------------------------------------------------

/// A vector id as a 64-hex event id: its ASCII bytes zero-padded to 32 bytes
/// — injective and order-preserving for the corpus's ids.
pub fn hex_id(id: &str) -> String {
    let mut bytes = [0_u8; 32];
    for (i, b) in id.bytes().take(32).enumerate() {
        bytes[i] = b;
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The pubkey of a seat: `0a`×32 for `first`, `14`×32 for `second`.
pub fn seat_pubkey(seat: &str) -> String {
    let byte = match seat {
        "first" => 0x0a_u8,
        "second" => 0x14_u8,
        other => panic!("unknown seat {other}"),
    };
    format!("{byte:02x}").repeat(32)
}

/// The synthetic timestamper of the attested run.
pub fn timestamper_pubkey() -> String {
    "63".repeat(32)
}

/// The session id.
pub fn session_id() -> String {
    hex_id("session")
}

/// A move array as a Ply's `content`.
pub fn content(mv: &Value) -> String {
    serde_json::to_string(mv).unwrap()
}

/// A scenario's time control, or the neutral `[[3600, null, null]]`.
pub fn time_control(vector: &Value) -> Value {
    vector
        .get("timeControl")
        .cloned()
        .unwrap_or_else(|| json!([[3600, Value::Null, Value::Null]]))
}

/// The session terms of a scenario in the given mode.
pub fn session_terms(vector: &Value, attested: bool) -> Value {
    json!({
        "id": session_id(),
        "first": seat_pubkey("first"),
        "second": seat_pubkey("second"),
        "timestamper": if attested { Value::from(timestamper_pubkey()) } else { Value::Null },
        "time_control": time_control(vector),
        "position": vector["position"],
        "start": vector["t0"],
    })
}

/// A scenario's Plies and attestations in the given mode.
pub fn events(vector: &Value, attested: bool) -> (Vec<Value>, Vec<Value>) {
    let mut plies = Vec::new();
    let mut attestations = Vec::new();
    for ply in vector["plies"].as_array().unwrap() {
        let id = ply["id"].as_str().unwrap();
        plies.push(json!({
            "id": hex_id(id),
            "signer": seat_pubkey(ply["seat"].as_str().unwrap()),
            "session": session_id(),
            "step": ply["step"],
            "draw": ply.get("draw").and_then(Value::as_bool).unwrap_or(false),
            "content": content(&ply["move"]),
            "created_at": ply["timedAt"],
        }));
        if attested {
            attestations.push(json!({
                "id": hex_id(&format!("att-{id}")),
                "signer": timestamper_pubkey(),
                "attests": hex_id(id),
                "created_at": ply["timedAt"],
            }));
        }
    }
    (plies, attestations)
}

/// The corpus file of a category.
pub fn corpus(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/conformance")
        .join(name);
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// The reference module's `max_step`: 300 (600 half-moves).
pub const MAX_STEP_HINT: u32 = 300;
