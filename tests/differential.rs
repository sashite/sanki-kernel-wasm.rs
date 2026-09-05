//! The random-session differential against the native reference engine
//! (Kernel ABI — Sanki §Conformance of a build, item 4): on sessions of random
//! play across the nine variant pairings, the module's `legal_moves`, `apply`,
//! `classify`, `natural_state` and `verdict_at` equal the answers of the
//! crates it is compiled from, called natively. Run in release:
//! `cargo test --release --test differential`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

mod common;

use common::{hex_id, module_bytes, seat_pubkey, session_id, Runtime};
use sashite_sanki_engine::domain::half_move::Move;
use sashite_sanki_engine::domain::side::Side;
use sashite_sanki_engine::domain::time::{Duration, Timestamp};
use sashite_sanki_engine::domain::time_control::{Period, TimeControl};
use sashite_sanki_engine::domain::variant::Variant;
use sashite_sanki_engine::engine::{apply_ply, legal_moves, status};
use sashite_sanki_engine::position::Position;
use sashite_sanki_engine::rules::initial_feen;
use sashite_sanki_kernel_wasm::encode::{content, intrinsic, verdict as encode_verdict};
use sashite_sanki_session::event::{EventId, Ply, PublicKey};
use sashite_sanki_session::natural_state::{natural_state, ChainEnd};
use sashite_sanki_session::session::{Seats, SessionParams};
use sashite_sanki_session::verdict::verdict_at;
use serde_json::{json, Value};

const GAMES: u64 = 27;
const MAX_HALF_MOVES: usize = 80;
const T0: i64 = 1_700_000_000;

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

const VARIANTS: [Variant; 3] = [Variant::Chess, Variant::Ogi, Variant::Xiongqi];

fn seat_key(seat: &str) -> PublicKey {
    PublicKey::parse(&seat_pubkey(seat)).unwrap()
}

#[test]
fn random_sessions_agree_with_the_native_engine() {
    let mut runtime = Runtime::new(&module_bytes());
    let mut rng = Lcg(0x0bad_5eed);
    let mut positions_checked = 0_usize;

    for game in 0..GAMES {
        let first = VARIANTS[usize::try_from(game % 3).unwrap()];
        let second = VARIANTS[usize::try_from((game / 3) % 3).unwrap()];
        let feen = initial_feen(first, second);
        let mut position = Position::parse(&feen).unwrap();
        let mut contents: Vec<String> = Vec::new();

        // Random play, checked position by position.
        for _ in 0..MAX_HALF_MOVES {
            let expected: Vec<String> = {
                let mut moves: Vec<String> = legal_moves(&position).iter().map(content).collect();
                moves.sort_unstable();
                moves
            };
            let answered =
                runtime.ask(&json!({ "op": "legal_moves", "position": position.to_feen() }));
            assert_eq!(
                answered["moves"],
                Value::from(expected.clone()),
                "legal_moves at {}",
                position.to_feen()
            );
            let classified =
                runtime.ask(&json!({ "op": "classify", "position": position.to_feen() }));
            assert_eq!(
                classified["status"],
                intrinsic(&status(&position)),
                "classify at {}",
                position.to_feen()
            );
            positions_checked += 1;
            if expected.is_empty() || status(&position).is_terminated() {
                break;
            }
            let chosen = &expected[rng.next(expected.len())];
            let mv = Move::parse(chosen).unwrap();
            let applied = apply_ply(&position, &mv).unwrap();
            let answered = runtime
                .ask(&json!({ "op": "apply", "position": position.to_feen(), "move": chosen }));
            assert_eq!(
                answered["position"],
                applied.position.to_feen(),
                "apply {chosen} at {}",
                position.to_feen()
            );
            assert_eq!(
                answered["irreversible"], applied.irreversible,
                "irreversible {chosen}"
            );
            assert_eq!(
                answered["status"],
                intrinsic(&status(&applied.position)),
                "status after {chosen}"
            );
            contents.push(chosen.clone());
            position = applied.position;
        }

        // The same play as a self-timed session: the chain, the end and the
        // verdict for each invoker, native and through the module.
        let control = TimeControl::new(
            Period::new(Duration::from_secs(600), Some(Duration::from_secs(5)), None).unwrap(),
            Vec::new(),
        );
        let params = SessionParams::new(
            EventId::parse(&session_id()).unwrap(),
            None,
            Seats::new(seat_key("first"), seat_key("second")).unwrap(),
            control.clone(),
            Position::parse(&feen).unwrap(),
            Timestamp::from_unix(T0),
        )
        .unwrap();
        let mut plies = Vec::new();
        let mut wire = Vec::new();
        for (index, content) in contents.iter().enumerate() {
            let seat = if index % 2 == 0 { "first" } else { "second" };
            let step = u32::try_from(index / 2 + 1).unwrap();
            let at =
                T0 + i64::try_from(index + 1).unwrap() * i64::try_from(1 + rng.next(30)).unwrap();
            let id = hex_id(&format!("g{game}-{index}"));
            plies.push(Ply::new(
                EventId::parse(&id).unwrap(),
                seat_key(seat),
                EventId::parse(&session_id()).unwrap(),
                step,
                false,
                content.clone(),
                Timestamp::from_unix(at),
            ));
            wire.push(json!({
                "id": id,
                "signer": seat_pubkey(seat),
                "session": session_id(),
                "step": step,
                "draw": false,
                "content": content,
                "created_at": at,
            }));
        }
        let cutoff = T0 + 40 * i64::try_from(MAX_HALF_MOVES + 1).unwrap();
        let session = json!({
            "id": session_id(),
            "first": seat_pubkey("first"),
            "second": seat_pubkey("second"),
            "timestamper": Value::Null,
            "time_control": [[600, 5, Value::Null]],
            "position": feen,
            "start": T0,
        });

        let native = natural_state(&params, &plies, &[], Timestamp::from_unix(cutoff));
        let answered = runtime.ask(&json!({
            "op": "natural_state", "session": session, "plies": wire, "attestations": [], "cutoff": cutoff,
        }));
        let native_chain: Vec<Value> = native
            .chain
            .iter()
            .map(|selected| json!({ "at": selected.at.as_unix(), "id": selected.ply.id.to_string() }))
            .collect();
        assert_eq!(
            answered["chain"],
            Value::from(native_chain),
            "chain of game {game}"
        );
        match &native.end {
            ChainEnd::Terminal { verdict, at } => {
                assert_eq!(
                    answered["end"]["terminal"]["verdict"],
                    encode_verdict(verdict),
                    "terminal of game {game}"
                );
                assert_eq!(
                    answered["end"]["terminal"]["at"],
                    at.as_unix(),
                    "terminal at of game {game}"
                );
            }
            ChainEnd::Ongoing(state) => {
                assert_eq!(
                    answered["end"]["ongoing"]["position"],
                    state.position().to_feen(),
                    "end position of game {game}"
                );
                assert_eq!(
                    answered["end"]["ongoing"]["half_move"],
                    state.half_move(),
                    "half_move of game {game}"
                );
                assert_eq!(
                    answered["end"]["ongoing"]["anchor"],
                    state.last_attestation().as_unix(),
                    "anchor of game {game}"
                );
            }
            ChainEnd::Inconsistent => panic!("inconsistent replay of game {game}"),
        }

        for (invoker, name) in [(Side::First, "first"), (Side::Second, "second")] {
            let expected =
                verdict_at(&params, &plies, &[], invoker, Timestamp::from_unix(cutoff)).unwrap();
            let answered = runtime.ask(&json!({
                "op": "verdict_at", "session": session, "plies": wire, "attestations": [],
                "invoker": name, "cutoff": cutoff,
            }));
            assert_eq!(
                answered["verdict"],
                encode_verdict(&expected),
                "verdict of game {game} for {name}"
            );
        }
    }
    eprintln!("differential: {GAMES} games, {positions_checked} positions checked");
    assert!(positions_checked > 500);
}
