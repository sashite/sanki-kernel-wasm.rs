//! The extreme session of Kernel ABI — Sanki §Bounds (§Conformance of a
//! build, item 5): 600 half-moves, `M = 32` candidates in every slot, every
//! flood content 256 four-byte characters, attested — within the request
//! bound, completing under `wasmi`, its digest recorded for the browser-engine
//! run. Run in release: `cargo test --release --test extreme`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

mod common;

use common::{hex_id, module_bytes, seat_pubkey, session_id, timestamper_pubkey, Record, Runtime};
use sashite_sanki_engine::domain::half_move::Move;
use sashite_sanki_engine::engine::{apply, legal_moves, status};
use sashite_sanki_engine::position::Position;
use sashite_sanki_kernel_wasm::encode::content;
use sashite_sanki_kernel_wasm::REQUEST_BOUND;
use serde_json::{json, Value};
use std::path::PathBuf;

const HALF_MOVES: usize = 600;
const PER_SLOT: usize = 32;
const T0: i64 = 1_700_000_000;
const STRIDE: i64 = 40;

/// A deterministic generator (a linear congruential sequence): the same
/// session on every run.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        usize::try_from((self.0 >> 33) % (bound as u64)).unwrap()
    }
}

/// 600 legal half-moves of random play from the chess/chess initial position;
/// when the play terminates, the remaining half-moves continue from a fresh
/// position — they are never reached by the replay, but every slot must hold
/// its legal candidate for the request to have the shape of the extreme.
fn legal_line(seed: u64) -> Vec<String> {
    let initial = Position::parse(&sashite_sanki_engine::rules::initial_feen(
        sashite_sanki_engine::domain::variant::Variant::Chess,
        sashite_sanki_engine::domain::variant::Variant::Chess,
    ))
    .unwrap();
    let mut rng = Lcg(seed);
    let mut position = initial.clone();
    let mut out = Vec::with_capacity(HALF_MOVES);
    while out.len() < HALF_MOVES {
        let moves = legal_moves(&position);
        if moves.is_empty() || status(&position).is_terminated() {
            position = initial.clone();
            continue;
        }
        let mv: &Move = &moves[rng.next(moves.len())];
        out.push(content(mv));
        position = apply(&position, mv).unwrap();
    }
    out
}

/// The extreme request: `verdict_at` for `first` at a cutoff after every Ply.
fn extreme_request() -> Vec<u8> {
    let line = legal_line(0x5a5a_1234);
    let flood: String = "😀".repeat(250);
    let mut plies = Vec::with_capacity(HALF_MOVES * PER_SLOT);
    let mut attestations = Vec::with_capacity(HALF_MOVES * PER_SLOT);
    for (half_move, legal) in line.iter().enumerate() {
        let seat = if half_move % 2 == 0 {
            "first"
        } else {
            "second"
        };
        let step = half_move / 2 + 1;
        let base = T0 + STRIDE * (half_move as i64 + 1);
        for slot in 0..PER_SLOT {
            // Seven floods before the legal candidate in the informed window,
            // the legal one eighth — the last the cap admits — then the rest.
            let (content, at) = if slot == 7 {
                (legal.clone(), base + 8)
            } else {
                (
                    format!("{flood}{seat}{step:03}{slot:02}"),
                    base + 1 + slot as i64 + usize::from(slot > 7) as i64,
                )
            };
            let id = hex_id(&format!("{seat}{step}-{slot}"));
            plies.push(json!({
                "id": id,
                "signer": seat_pubkey(seat),
                "session": session_id(),
                "step": step,
                "draw": false,
                "content": content,
                "created_at": at,
            }));
            attestations.push(json!({
                "id": hex_id(&format!("a{seat}{step}-{slot}")),
                "signer": timestamper_pubkey(),
                "attests": id,
                "created_at": at,
            }));
        }
    }
    let request = json!({
        "op": "verdict_at",
        "session": {
            "id": session_id(),
            "first": seat_pubkey("first"),
            "second": seat_pubkey("second"),
            "timestamper": timestamper_pubkey(),
            "time_control": [[100_000, Value::Null, Value::Null]],
            "position": sashite_sanki_engine::rules::initial_feen(
                sashite_sanki_engine::domain::variant::Variant::Chess,
                sashite_sanki_engine::domain::variant::Variant::Chess,
            ),
            "start": T0,
        },
        "plies": plies,
        "attestations": attestations,
        "invoker": "first",
        "cutoff": T0 + STRIDE * (HALF_MOVES as i64 + 2),
    });
    serde_json::to_vec(&request).unwrap()
}

#[test]
fn extreme_session_completes_within_the_bounds() {
    let request = extreme_request();
    let size = request.len();
    assert!(
        size <= REQUEST_BOUND,
        "the extreme request is {size} bytes, above the bound"
    );
    let mib = size as f64 / (1024.0 * 1024.0);
    eprintln!("extreme request: {size} bytes ({mib:.2} MiB)");

    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/conformance");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("extreme.request.json"), &request).unwrap();

    let mut runtime = Runtime::new(&module_bytes());
    let started = std::time::Instant::now();
    let response = runtime
        .request(&request)
        .expect("the extreme session answers");
    let elapsed = started.elapsed();
    eprintln!("extreme session answered in {elapsed:?} under wasmi");
    let value: Value = serde_json::from_slice(&response).unwrap();
    assert!(value.get("verdict").is_some(), "a verdict, not {value}");

    let mut record = Record::default();
    record.add("extreme", &response);
    record.write("extreme.wasmi.json");

    // The instance is intact afterwards.
    let describe = runtime.ask(&json!({ "op": "describe" }));
    assert_eq!(describe["game"], "sanki");
}
