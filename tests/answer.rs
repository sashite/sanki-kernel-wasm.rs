//! The module's behaviour as a pure function of bytes, natively: the errors of
//! Kernel ABI — Sanki §The request and the response, the claim domain of
//! `check` and `select_conclusion`, the `max_step` filter, the bounds.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use sashite_sanki_kernel_wasm::{answer, evaluate, REQUEST_BOUND};
use serde_json::{json, Value};

fn ask(request: &Value) -> Value {
    let bytes = serde_json::to_vec(request).unwrap();
    serde_json::from_slice(&answer(&bytes).unwrap()).unwrap()
}

fn ask_text(text: &str) -> Value {
    serde_json::from_slice(&answer(text.as_bytes()).unwrap()).unwrap()
}

fn code(value: &Value) -> &str {
    value["error"]["code"].as_str().unwrap_or("")
}

const FIRST: &str = "0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a";
const SECOND: &str = "1414141414141414141414141414141414141414141414141414141414141414";
const OTHER: &str = "2121212121212121212121212121212121212121212121212121212121212121";
const SESSION: &str = "73657373696f6e00000000000000000000000000000000000000000000000000";
const SESSION_B: &str = "6f74686572000000000000000000000000000000000000000000000000000000";
const ROOK_ENDING: &str = "4k^3/8/8/8/8/8/8/R3K^3 / W/w";

fn id(n: u8) -> String {
    format!("{n:02x}").repeat(32)
}

fn session(timestamper: Option<&str>) -> Value {
    json!({
        "id": SESSION,
        "first": FIRST,
        "second": SECOND,
        "timestamper": timestamper,
        "time_control": [[3600, 3, Value::Null]],
        "position": ROOK_ENDING,
        "start": 1000,
    })
}

fn ply(n: u8, signer: &str, step: i64, content: &str, at: i64) -> Value {
    json!({
        "id": id(n), "signer": signer, "session": SESSION, "step": step,
        "draw": false, "content": content, "created_at": at,
    })
}

fn conclusion(
    n: u8,
    signer: &str,
    session: &str,
    status: &str,
    first: i64,
    second: i64,
    at: i64,
) -> Value {
    json!({
        "id": id(n), "signer": signer, "session": session, "status": status,
        "result": { "first": first, "second": second }, "created_at": at,
    })
}

#[test]
fn errors_of_the_abi() {
    assert_eq!(code(&ask(&json!({ "op": "nope" }))), "unsupported_op");
    assert_eq!(
        code(&ask(&json!({ "op": "describe", "extra": 1 }))),
        "malformed"
    );
    assert_eq!(code(&ask(&json!({ "op": "ggn" }))), "malformed");
    assert_eq!(
        code(&ask(&json!({ "op": "ggn", "variant": "go" }))),
        "unknown_variant"
    );
    assert_eq!(
        code(&ask(&json!({ "op": "classify", "position": "not a feen" }))),
        "invalid_position"
    );
    assert_eq!(
        code(&ask(
            &json!({ "op": "apply", "position": ROOK_ENDING, "move": "[\"h4\",\"h5\",null]" })
        )),
        "illegal"
    );
    assert_eq!(
        code(&ask(
            &json!({ "op": "apply", "position": ROOK_ENDING, "move": "garbage" })
        )),
        "illegal"
    );
    assert_eq!(
        code(&ask_text("{\"op\":\"describe\",\"op\":\"describe\"}")),
        "malformed"
    );
    assert_eq!(
        code(&ask_text("{\"op\":\"elapsed\",\"anchor\":-0,\"timing\":1}")),
        "malformed"
    );
    assert_eq!(
        code(&ask_text(
            "{\"op\":\"elapsed\",\"anchor\":1.0,\"timing\":1}"
        )),
        "malformed"
    );
    assert_eq!(code(&ask_text("[1]")), "malformed");
    assert_eq!(
        code(&ask_text("\u{feff}{\"op\":\"describe\"}")),
        "malformed"
    );
    // Out-of-range value classes.
    assert_eq!(
        code(&ask(
            &json!({ "op": "elapsed", "anchor": 1_i64 << 40, "timing": 1 })
        )),
        "malformed"
    );
    assert_eq!(
        code(&ask(&json!({ "op": "elapsed", "anchor": -1, "timing": 1 }))),
        "malformed"
    );
    assert_eq!(
        code(&ask(
            &json!({ "op": "select", "boundary": 1, "cap": 0, "candidates": [] })
        )),
        "malformed"
    );
    assert_eq!(
        code(&ask(
            &json!({ "op": "select", "boundary": 1, "cap": 1_i64 << 31, "candidates": [] })
        )),
        "malformed"
    );
    // Oversized, evaluated directly.
    let big = vec![b' '; REQUEST_BOUND + 1];
    assert_eq!(evaluate(&big).unwrap_err().code(), "oversized");
}

#[test]
fn session_terms_are_checked() {
    let mut terms = session(None);
    terms["second"] = Value::from(FIRST);
    let request = json!({ "op": "natural_state", "session": terms, "plies": [], "attestations": [], "cutoff": 2000 });
    assert_eq!(code(&ask(&request)), "malformed");

    let mut terms = session(None);
    terms["position"] = Value::from("4k^3/8/8/8/8/8/8/R3K^3 / w/W");
    let request = json!({ "op": "natural_state", "session": terms, "plies": [], "attestations": [], "cutoff": 2000 });
    assert_eq!(code(&ask(&request)), "invalid_position");

    let mut terms = session(None);
    terms["time_control"] = json!([[0, Value::Null, Value::Null]]);
    let request = json!({ "op": "natural_state", "session": terms, "plies": [], "attestations": [], "cutoff": 2000 });
    assert_eq!(code(&ask(&request)), "malformed");

    let mut terms = session(None);
    terms["first"] = Value::from(FIRST.to_uppercase());
    let request = json!({ "op": "natural_state", "session": terms, "plies": [], "attestations": [], "cutoff": 2000 });
    assert_eq!(code(&ask(&request)), "malformed");

    // A missing nullable member is malformed; an explicit null is self-timed.
    let mut terms = session(None);
    terms.as_object_mut().unwrap().remove("timestamper");
    let request = json!({ "op": "natural_state", "session": terms, "plies": [], "attestations": [], "cutoff": 2000 });
    assert_eq!(code(&ask(&request)), "malformed");
}

#[test]
fn natural_state_ignores_what_cannot_enter_a_chain() {
    let plies = vec![
        ply(1, FIRST, 1, "[\"a1\",\"a4\",null]", 1100),
        // Beyond max_step, from another session, from a non-player: ignored.
        ply(2, SECOND, 301, "[\"e8\",\"e7\",null]", 1200),
        json!({ "id": id(3), "signer": SECOND, "session": SESSION_B, "step": 1, "draw": false,
                "content": "[\"e8\",\"e7\",null]", "created_at": 1200 }),
        ply(4, OTHER, 1, "[\"e8\",\"e7\",null]", 1200),
        // A step out of the value class is malformed, not ignored.
    ];
    let request = json!({ "op": "natural_state", "session": session(None), "plies": plies, "attestations": [], "cutoff": 2000 });
    let state = ask(&request);
    assert_eq!(state["chain"].as_array().unwrap().len(), 1, "{state}");
    assert_eq!(state["end"]["ongoing"]["half_move"], 2);
    assert_eq!(state["end"]["ongoing"]["anchor"], 1100);

    let request = json!({ "op": "natural_state", "session": session(None),
        "plies": [ply(5, FIRST, 0, "[\"a1\",\"a4\",null]", 1100)], "attestations": [], "cutoff": 2000 });
    assert_eq!(code(&ask(&request)), "malformed");
}

#[test]
fn verdict_and_check_across_the_claim_domain() {
    // Nobody moved: concluding at 2000 is the concluder's resignation.
    let request = json!({ "op": "verdict_at", "session": session(None), "plies": [], "attestations": [],
        "invoker": "first", "cutoff": 2000 });
    let verdict = ask(&request);
    assert_eq!(verdict["verdict"]["status"], "resignation");
    assert_eq!(
        verdict["verdict"]["result"],
        json!({ "first": 0, "second": 100 })
    );

    let request = json!({ "op": "verdict_at", "session": session(None), "plies": [], "attestations": [],
        "invoker": "first", "cutoff": 500 });
    assert_eq!(ask(&request)["no_verdict"], "before_start");

    let check = |c: Value| {
        ask(
            &json!({ "op": "check", "session": session(None), "plies": [], "attestations": [], "conclusion": c }),
        )
    };
    // The right claim conforms.
    let answer = check(conclusion(9, FIRST, SESSION, "resignation", 0, 100, 2000));
    assert_eq!(answer["conforming"]["status"], "resignation", "{answer}");
    // A coherent wrong claim.
    let answer = check(conclusion(9, FIRST, SESSION, "checkmate", 100, 0, 2000));
    assert_eq!(answer["wrong"]["expected"]["status"], "resignation");
    assert_eq!(
        answer["wrong"]["claimed"],
        json!({ "result": { "first": 100, "second": 0 }, "status": "checkmate" })
    );
    // Claims the kernel never yields, in kind 3425's domain: wrong, never malformed.
    let answer = check(conclusion(9, FIRST, SESSION, "foo", 0, 100, 2000));
    assert_eq!(answer["wrong"]["claimed"]["status"], "foo", "{answer}");
    let answer = check(conclusion(9, FIRST, SESSION, "resignation", 60, 40, 2000));
    assert_eq!(
        answer["wrong"]["claimed"]["result"],
        json!({ "first": 60, "second": 40 }),
        "{answer}"
    );
    let answer = check(conclusion(9, FIRST, SESSION, "checkmate", 50, 50, 2000));
    assert_eq!(
        answer["wrong"]["expected"]["status"], "resignation",
        "{answer}"
    );
    // Outside the domain: malformed.
    assert_eq!(
        code(&check(conclusion(9, FIRST, SESSION, "Foo", 0, 100, 2000))),
        "malformed"
    );
    assert_eq!(
        code(&check(conclusion(
            9,
            FIRST,
            SESSION,
            "resignation",
            70,
            40,
            2000
        ))),
        "malformed"
    );
    // Reach, in order, before the claim — for any claim.
    assert_eq!(
        check(conclusion(9, FIRST, SESSION_B, "foo", 0, 100, 2000))["no_verdict"],
        "other_session"
    );
    assert_eq!(
        check(conclusion(9, OTHER, SESSION, "foo", 0, 100, 2000))["no_verdict"],
        "not_a_player"
    );
    assert_eq!(
        check(conclusion(9, FIRST, SESSION, "foo", 0, 100, 500))["no_verdict"],
        "before_start"
    );
    let pending = ask(
        &json!({ "op": "check", "session": session(Some(OTHER)), "plies": [], "attestations": [],
        "conclusion": conclusion(9, FIRST, SESSION, "resignation", 0, 100, 2000) }),
    );
    assert_eq!(pending["no_verdict"], "pending");
}

#[test]
fn select_conclusion_keeps_the_earliest_conforming_one() {
    let conclusions = vec![
        conclusion(3, SECOND, SESSION, "foo", 0, 100, 1500), // never canonical
        conclusion(2, FIRST, SESSION, "checkmate", 100, 0, 1600), // wrong
        conclusion(1, SECOND, SESSION, "resignation", 100, 0, 1700), // conforming (second resigns)
        conclusion(4, FIRST, SESSION, "resignation", 0, 100, 1650), // conforming, earlier
    ];
    let request = json!({ "op": "select_conclusion", "session": session(None), "plies": [], "attestations": [],
        "conclusions": conclusions });
    let answer = ask(&request);
    assert_eq!(answer["canonical"]["id"], id(4), "{answer}");
    assert_eq!(answer["canonical"]["cutoff"], 1650);
    assert_eq!(answer["canonical"]["verdict"]["status"], "resignation");

    let request = json!({ "op": "select_conclusion", "session": session(None), "plies": [], "attestations": [],
        "conclusions": [conclusion(3, SECOND, SESSION, "foo", 0, 100, 1500)] });
    assert_eq!(ask(&request)["canonical"], Value::Null);
}

#[test]
fn corpus_primitives_and_their_bounds() {
    let answer = ask(&json!({ "op": "charges", "start": 0, "timings": [12, 5, 30] }));
    assert_eq!(
        answer,
        json!({ "anchor": 30, "first": [12, 18], "on_move": "second", "second": [0] })
    );
    let ok = ask(&json!({ "op": "charges", "start": 0, "timings": vec![1_i64; 600] }));
    assert_eq!(ok["on_move"], "first");
    assert_eq!(
        code(&ask(
            &json!({ "op": "charges", "start": 0, "timings": vec![1_i64; 601] })
        )),
        "malformed"
    );
    let flagged = ask(
        &json!({ "op": "clock", "time_control": [[300, 3, Value::Null]],
        "clock": { "period": 5, "plies_in_period": 0, "remaining": 300 }, "elapsed": 1 }),
    );
    assert_eq!(flagged["kind"], "flagged");
    let response = sashite_sanki_kernel_wasm::answer(br#"{"op":"describe"}"#).unwrap();
    // Canonical form: sorted keys, no whitespace.
    let text = String::from_utf8(response).unwrap();
    assert!(
        text.starts_with("{\"abi\":\"sashite.sanki.kernel-abi/1\",\"game\":\"sanki\",\"kernel\":")
    );
    assert!(!text.contains(": "));
}
