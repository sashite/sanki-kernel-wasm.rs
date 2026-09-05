//! The module under `wasmi`: its shape, its exports, and the conformance
//! corpus run through it (Kernel ABI — Sanki §Conformance of a build, items
//! 1–3). Every response's digest is recorded under `target/conformance/` for
//! the comparison with the browser-engine run (`harness/node`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

mod common;

use common::{
    content, corpus, events, module_bytes, session_terms, Record, Runtime, MAX_STEP_HINT,
};
use serde_json::{json, Value};

/// A `start` section (id 8) must be absent: a hand walk of the module's
/// sections, since the runtime does not expose it.
fn has_start_section(bytes: &[u8]) -> bool {
    let mut pos = 8; // magic + version
    while pos < bytes.len() {
        let id = bytes[pos];
        pos += 1;
        // LEB128 size.
        let mut size: usize = 0;
        let mut shift = 0;
        loop {
            let byte = bytes[pos];
            pos += 1;
            size |= usize::from(byte & 0x7f) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                break;
            }
        }
        if id == 8 {
            return true;
        }
        pos += size;
    }
    false
}

#[test]
fn shape_and_abi() {
    let bytes = module_bytes();
    assert!(!has_start_section(&bytes), "no start section");
    let mut runtime = Runtime::new(&bytes);
    assert_eq!(runtime.abi(), "sashite.sanki.kernel-abi/1");
    let describe = runtime.ask(&json!({ "op": "describe" }));
    assert_eq!(describe["abi"], "sashite.sanki.kernel-abi/1");
    assert_eq!(describe["game"], "sanki");
    assert_eq!(describe["max_step"], MAX_STEP_HINT);
    assert_eq!(describe["variants"]["chess"]["style"], "W");
    assert_eq!(describe["positions"].as_object().unwrap().len(), 9);
}

#[test]
fn arena_protocol_edges() {
    let mut runtime = Runtime::new(&module_bytes());
    // An empty request is malformed, not a trap.
    let response: Value = serde_json::from_slice(&runtime.request(b"").unwrap()).unwrap();
    assert_eq!(response["error"]["code"], "malformed");
    // Above the bound, alloc refuses.
    assert!(runtime.request(&vec![b' '; 33_554_433]).is_none());
    // At the bound, the request is read (and is malformed).
    let response: Value =
        serde_json::from_slice(&runtime.request(&vec![b' '; 33_554_432]).unwrap()).unwrap();
    assert_eq!(response["error"]["code"], "malformed");
    // The instance keeps answering afterwards.
    let describe = runtime.ask(&json!({ "op": "describe" }));
    assert_eq!(describe["game"], "sanki");
}

#[test]
fn no_answer_depends_on_a_previous_request() {
    // The same request, asked first, after a large session, after an error
    // and after an oversized refusal, answers the same bytes every time.
    let mut runtime = Runtime::new(&module_bytes());
    let probe = json!({ "op": "legal_moves", "position": "4k^3/8/8/8/8/8/8/R3K^3 / W/w" });
    let first = runtime.ask_bytes(&probe);
    let vector = &corpus("scenarios.json")["vectors"][0];
    let (plies, attestations) = events(vector, true);
    let _ = runtime.ask_bytes(&json!({
        "op": "natural_state", "session": session_terms(vector, true),
        "plies": plies, "attestations": attestations, "cutoff": vector["cutoff"],
    }));
    let _ = runtime.ask_bytes(&json!({ "op": "nope" }));
    assert!(runtime.request(&vec![b' '; 33_554_433]).is_none());
    let _ = runtime.ask_bytes(&json!({ "op": "ggn", "variant": "ogi" }));
    let again = runtime.ask_bytes(&probe);
    assert_eq!(first, again);
    // And a fresh instance answers the same bytes as a used one.
    let mut fresh = Runtime::new(&module_bytes());
    assert_eq!(fresh.ask_bytes(&probe), first);
}

#[test]
fn corpus_legality() {
    let mut runtime = Runtime::new(&module_bytes());
    let mut record = Record::default();
    for vector in corpus("legality.json")["vectors"].as_array().unwrap() {
        let id = vector["id"].as_str().unwrap();
        let request = json!({
            "op": "apply",
            "position": vector["position"],
            "move": content(&vector["move"]),
        });
        let bytes = runtime.ask_bytes(&request);
        record.add(id, &bytes);
        let response: Value = serde_json::from_slice(&bytes).unwrap();
        if vector["legal"].as_bool().unwrap() {
            assert_eq!(response["position"], vector["result"], "{id}: position");
            assert_eq!(response["status"], vector["status"], "{id}: status");
            // `classify` on the result agrees with `apply`'s status.
            let classified =
                runtime.ask(&json!({ "op": "classify", "position": vector["result"] }));
            assert_eq!(classified["status"], vector["status"], "{id}: classify");
        } else {
            assert_eq!(
                response["error"]["code"], "illegal",
                "{id}: expected illegal"
            );
        }
    }
    record.write("legality.wasmi.json");
}

#[test]
fn corpus_selection() {
    let mut runtime = Runtime::new(&module_bytes());
    let mut record = Record::default();
    for vector in corpus("selection.json")["vectors"].as_array().unwrap() {
        let id = vector["id"].as_str().unwrap();
        let candidates: Vec<Value> = vector["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| json!({ "id": c["id"], "created_at": c["createdAt"], "legal": c["legal"] }))
            .collect();
        let request = json!({
            "op": "select",
            "boundary": vector["boundary"],
            "cap": vector["cap"],
            "candidates": candidates,
        });
        let bytes = runtime.ask_bytes(&request);
        record.add(id, &bytes);
        let response: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(response["result"], vector["expected"]["result"], "{id}");
        assert_eq!(response["selected"], vector["expected"]["selected"], "{id}");
    }
    record.write("selection.wasmi.json");
}

#[test]
fn corpus_time() {
    let mut runtime = Runtime::new(&module_bytes());
    let mut record = Record::default();
    for vector in corpus("time.json")["vectors"].as_array().unwrap() {
        let id = vector["id"].as_str().unwrap();
        if vector.get("timings").is_some() {
            let request =
                json!({ "op": "charges", "start": vector["t0"], "timings": vector["timings"] });
            let bytes = runtime.ask_bytes(&request);
            record.add(id, &bytes);
            let response: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(response["first"], vector["expected"]["first"], "{id}");
            assert_eq!(response["second"], vector["expected"]["second"], "{id}");
            assert_eq!(response["anchor"], vector["expected"]["anchor"], "{id}");
            assert_eq!(response["on_move"], vector["expected"]["onMove"], "{id}");
        } else {
            let request =
                json!({ "op": "elapsed", "anchor": vector["anchor"], "timing": vector["timing"] });
            let bytes = runtime.ask_bytes(&request);
            record.add(id, &bytes);
            let response: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(response["elapsed"], vector["expectedElapsed"], "{id}");
        }
    }
    record.write("time.wasmi.json");
}

#[test]
fn corpus_clock() {
    let mut runtime = Runtime::new(&module_bytes());
    let mut record = Record::default();
    for vector in corpus("clock.json")["vectors"].as_array().unwrap() {
        let id = vector["id"].as_str().unwrap();
        let clock = &vector["clock"];
        let request = json!({
            "op": "clock",
            "time_control": vector["timeControl"],
            "clock": {
                "period": clock["period"],
                "plies_in_period": clock["pliesInPeriod"],
                "remaining": clock["remaining"],
            },
            "elapsed": vector["elapsed"],
        });
        let bytes = runtime.ask_bytes(&request);
        record.add(id, &bytes);
        let response: Value = serde_json::from_slice(&bytes).unwrap();
        let expected = &vector["expected"];
        assert_eq!(response["kind"], expected["kind"], "{id}");
        if expected["kind"] == "continued" {
            let after = &expected["clock"];
            assert_eq!(response["clock"]["remaining"], after["remaining"], "{id}");
            assert_eq!(response["clock"]["period"], after["period"], "{id}");
            assert_eq!(
                response["clock"]["plies_in_period"], after["pliesInPeriod"],
                "{id}"
            );
        }
    }
    record.write("clock.wasmi.json");
}

/// A scenario in one mode: the chain and the termination through
/// `natural_state`, the verdict through `verdict_at`.
fn run_scenario(runtime: &mut Runtime, record: &mut Record, vector: &Value, attested: bool) {
    let id = vector["id"].as_str().unwrap();
    let mode = if attested { "attested" } else { "self-timed" };
    let (plies, attestations) = events(vector, attested);
    let session = session_terms(vector, attested);
    let request = json!({
        "op": "natural_state",
        "session": session,
        "plies": plies,
        "attestations": attestations,
        "cutoff": vector["cutoff"],
    });
    let bytes = runtime.ask_bytes(&request);
    record.add(&format!("{id}#{mode}#natural_state"), &bytes);
    let response: Value = serde_json::from_slice(&bytes).unwrap();
    let chain: Vec<Value> = response["chain"]
        .as_array()
        .unwrap_or_else(|| panic!("{id} ({mode}): {response}"))
        .iter()
        .map(|entry| {
            let hex = entry["id"].as_str().unwrap();
            let raw: Vec<u8> = (0..32)
                .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
                .take_while(|b| *b != 0)
                .collect();
            Value::from(String::from_utf8(raw).unwrap())
        })
        .collect();
    assert_eq!(
        Value::from(chain),
        vector["expectedChain"],
        "{id} ({mode}): chain"
    );
    match &vector["expectedTermination"] {
        Value::Null => assert!(
            response["end"].get("ongoing").is_some(),
            "{id} ({mode}): ongoing"
        ),
        termination => assert_eq!(
            response["end"]["terminal"]["verdict"]["status"], termination["status"],
            "{id} ({mode}): termination"
        ),
    }
    if let Some(invoker) = vector.get("invoker") {
        let (plies, attestations) = events(vector, attested);
        let request = json!({
            "op": "verdict_at",
            "session": session_terms(vector, attested),
            "plies": plies,
            "attestations": attestations,
            "invoker": invoker,
            "cutoff": vector["cutoff"],
        });
        let bytes = runtime.ask_bytes(&request);
        record.add(&format!("{id}#{mode}#verdict_at"), &bytes);
        let response: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            response["verdict"], vector["expectedVerdict"],
            "{id} ({mode}): verdict"
        );
    }
}

#[test]
fn corpus_scenarios() {
    let mut runtime = Runtime::new(&module_bytes());
    let mut record = Record::default();
    for vector in corpus("scenarios.json")["vectors"].as_array().unwrap() {
        run_scenario(&mut runtime, &mut record, vector, false);
        run_scenario(&mut runtime, &mut record, vector, true);
    }
    record.write("scenarios.wasmi.json");
}

#[test]
fn describe_and_ggn_are_recorded() {
    let mut runtime = Runtime::new(&module_bytes());
    let mut record = Record::default();
    record.add("describe", &runtime.ask_bytes(&json!({ "op": "describe" })));
    for variant in ["chess", "ogi", "xiongqi"] {
        let bytes = runtime.ask_bytes(&json!({ "op": "ggn", "variant": variant }));
        assert!(bytes.len() < 4_194_304, "the ggn response is within bound");
        record.add(&format!("ggn/{variant}"), &bytes);
    }
    record.write("rule-system.wasmi.json");
}
