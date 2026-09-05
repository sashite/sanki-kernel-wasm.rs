//! The thirteen operations (Kernel ABI — Sanki §Operations), each a thin
//! mapping from the wire model onto the engine and the session kernel. The
//! module computes nothing of its own here: every rule answer is the crates'.

use crate::encode;
use crate::fault::Fault;
use crate::request::{
    self, ApplyReq, ChargesReq, CheckReq, ClockReq, ConclusionOffered, ElapsedReq, GgnReq,
    NaturalStateReq, PositionReq, SelectConclusionReq, SelectReq, VerdictAtReq,
};
use sashite_sanki_engine::clock::{tick, Tick};
use sashite_sanki_engine::domain::half_move::Move;
use sashite_sanki_engine::domain::side::Side;
use sashite_sanki_engine::domain::time::{Duration, Timestamp};
use sashite_sanki_engine::domain::variant::Variant;
use sashite_sanki_engine::engine::{apply_ply, legal_moves, status};
use sashite_sanki_engine::terminal::move_cap::HALF_MOVE_CAP;
use sashite_sanki_engine::{ggn, rules};
use sashite_sanki_session::natural_state::{natural_state, ChainEnd};
use sashite_sanki_session::selection::{select_candidate, Candidate};
use sashite_sanki_session::verdict::{
    check, cutoff_of, select_conclusion, verdict_at, Check, NoVerdict,
};
use serde_json::{json, Map, Value};

/// The ABI identifier this module states.
pub const ABI: &str = "sashite.sanki.kernel-abi/1";
/// The game identifier a Rule System event naming this module carries.
pub const GAME: &str = "sanki";
/// The kernel specification the module was built to (documentary).
pub const KERNEL: &str = "sashite.sanki.kernel/1";
/// The largest `step` a Ply can enter a chain with: half the half-move cap,
/// under the suite's alternation of seats.
pub const MAX_STEP: u32 = HALF_MOVE_CAP / 2;

const VARIANTS: [Variant; 3] = [Variant::Chess, Variant::Ogi, Variant::Xiongqi];

fn variant(name: &str) -> Option<Variant> {
    VARIANTS
        .into_iter()
        .find(|candidate| rules::variant_name(*candidate) == name)
}

// ---- rule system ------------------------------------------------------------

/// `describe`.
#[must_use]
pub fn describe() -> Value {
    // The engine's own statement of its parameters, minus the members of the
    // retired manifest (format, name, kernel digests, GGN digests).
    let digests: Vec<(Variant, String)> = VARIANTS.iter().map(|v| (*v, String::new())).collect();
    let mut manifest = match rules::manifest("", "", &digests) {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    let mut variants_out = Map::new();
    let mut variants_params = Map::new();
    if let Some(Value::Object(entries)) = manifest.remove("variants") {
        for (name, entry) in entries {
            let Value::Object(mut fields) = entry else {
                continue;
            };
            fields.remove("ggn");
            let mut summary = Map::new();
            for key in ["initial", "pieces", "style"] {
                if let Some(value) = fields.get(key) {
                    summary.insert(key.to_owned(), value.clone());
                }
            }
            variants_out.insert(name.clone(), Value::Object(summary));
            variants_params.insert(name, Value::Object(fields));
        }
    }
    let parameters = json!({
        "pairings": manifest.remove("pairings").unwrap_or(Value::Null),
        "session": manifest.remove("session").unwrap_or(Value::Null),
        "variants": Value::Object(variants_params),
    });
    let mut positions = Map::new();
    for first in VARIANTS {
        for second in VARIANTS {
            let key = format!(
                "{}/{}",
                rules::variant_name(first),
                rules::variant_name(second)
            );
            positions.insert(key, Value::from(rules::initial_feen(first, second)));
        }
    }
    json!({
        "abi": ABI,
        "game": GAME,
        "kernel": KERNEL,
        "max_step": MAX_STEP,
        "parameters": parameters,
        "positions": Value::Object(positions),
        "variants": Value::Object(variants_out),
    })
}

/// `ggn`.
pub fn ggn_table(req: &GgnReq) -> Result<Value, Fault> {
    let variant =
        variant(&req.variant).ok_or_else(|| Fault::UnknownVariant(req.variant.clone()))?;
    let document = ggn::to_json(&ggn::document(variant));
    Ok(json!({ "ggn": document, "variant": req.variant }))
}

// ---- position ---------------------------------------------------------------

/// `legal_moves`.
pub fn legal_moves_of(req: &PositionReq) -> Result<Value, Fault> {
    let position = request::position(&req.position)?;
    let mut moves: Vec<String> = legal_moves(&position).iter().map(encode::content).collect();
    moves.sort_unstable();
    Ok(json!({ "moves": moves }))
}

/// `apply`.
pub fn apply(req: &ApplyReq) -> Result<Value, Fault> {
    let position = request::position(&req.position)?;
    let mv = Move::parse(&req.mv)
        .map_err(|error| Fault::Illegal(format!("unparseable content: {error}")))?;
    let applied = apply_ply(&position, &mv).map_err(|reason| Fault::Illegal(reason.to_string()))?;
    let verdict = status(&applied.position);
    Ok(json!({
        "irreversible": applied.irreversible,
        "position": applied.position.to_feen(),
        "status": encode::intrinsic(&verdict),
    }))
}

/// `classify`.
pub fn classify(req: &PositionReq) -> Result<Value, Fault> {
    let position = request::position(&req.position)?;
    Ok(json!({ "status": encode::intrinsic(&status(&position)) }))
}

// ---- session ----------------------------------------------------------------

/// The natural state, encoded; `None` when the replay is inconsistent.
fn natural_state_value(
    params: &sashite_sanki_session::session::SessionParams,
    plies: &[sashite_sanki_session::event::Ply],
    attestations: &[sashite_sanki_session::event::Attestation],
    cutoff: Timestamp,
) -> Result<Value, Fault> {
    let natural = natural_state(params, plies, attestations, cutoff);
    let chain: Vec<Value> = natural
        .chain
        .iter()
        .map(|selected| json!({ "at": encode::timing(selected.at), "id": selected.ply.id.to_string() }))
        .collect();
    let end = match &natural.end {
        ChainEnd::Terminal { verdict, at } => json!({
            "terminal": { "at": encode::timing(*at), "verdict": encode::verdict(verdict) }
        }),
        ChainEnd::Ongoing(state) => json!({
            "ongoing": {
                "anchor": encode::timing(state.last_attestation()),
                "clocks": {
                    "first": encode::clock(state.clocks().get(Side::First)),
                    "second": encode::clock(state.clocks().get(Side::Second)),
                },
                "half_move": state.half_move(),
                "position": state.position().to_feen(),
            }
        }),
        ChainEnd::Inconsistent => return Err(Fault::Inconsistent),
    };
    Ok(json!({ "chain": chain, "end": end }))
}

/// `natural_state`.
pub fn natural_state_op(req: NaturalStateReq) -> Result<Value, Fault> {
    let params = request::session_params(&req.session)?;
    let cutoff = request::timing(req.cutoff, "cutoff")?;
    let attestations = request::attestations(&req.attestations)?;
    let plies = request::plies(req.plies, MAX_STEP)?;
    natural_state_value(&params, &plies, &attestations, cutoff)
}

/// A `no_verdict` reason, or the inconsistency fault.
fn no_verdict(reason: NoVerdict) -> Result<Value, Fault> {
    let token = match reason {
        NoVerdict::OtherSession => "other_session",
        NoVerdict::NotAPlayer => "not_a_player",
        NoVerdict::Pending => "pending",
        NoVerdict::BeforeStart => "before_start",
        NoVerdict::Inconsistent => return Err(Fault::Inconsistent),
    };
    Ok(json!({ "no_verdict": token }))
}

/// `verdict_at`.
pub fn verdict_at_op(req: VerdictAtReq) -> Result<Value, Fault> {
    let params = request::session_params(&req.session)?;
    let invoker = request::side(&req.invoker, "invoker")?;
    let cutoff = request::timing(req.cutoff, "cutoff")?;
    let attestations = request::attestations(&req.attestations)?;
    let plies = request::plies(req.plies, MAX_STEP)?;
    match verdict_at(&params, &plies, &attestations, invoker, cutoff) {
        Ok(verdict) => Ok(json!({ "verdict": encode::verdict(&verdict) })),
        Err(reason) => no_verdict(reason),
    }
}

/// The claim as offered, echoed.
fn claimed(offered: &ConclusionOffered) -> Value {
    json!({
        "result": { "first": offered.claim.scores.0, "second": offered.claim.scores.1 },
        "status": offered.claim.status,
    })
}

/// `check`.
pub fn check_op(req: CheckReq) -> Result<Value, Fault> {
    let params = request::session_params(&req.session)?;
    let attestations = request::attestations(&req.attestations)?;
    let plies = request::plies(req.plies, MAX_STEP)?;
    let offered = request::conclusion(&req.conclusion, "conclusion")?;
    match offered.typed() {
        // A claim the kernel can yield: the session kernel's check.
        Some(conclusion) => match check(&params, &plies, &attestations, &conclusion) {
            Check::Conforming(verdict) => Ok(json!({ "conforming": encode::verdict(&verdict) })),
            Check::Wrong {
                claimed: _,
                expected,
            } => Ok(json!({
                "wrong": { "claimed": claimed(&offered), "expected": encode::verdict(&expected) }
            })),
            Check::NoVerdict(reason) => no_verdict(reason),
        },
        // A claim the kernel never yields: in reach it is wrong, out of reach
        // it is out of reach — the same order as for any Conclusion.
        None => {
            let (invoker, cutoff) = match cutoff_of_offered(&params, &attestations, &offered) {
                Ok(reach) => reach,
                Err(reason) => return no_verdict(reason),
            };
            match verdict_at(&params, &plies, &attestations, invoker, cutoff) {
                Ok(expected) => Ok(json!({
                    "wrong": { "claimed": claimed(&offered), "expected": encode::verdict(&expected) }
                })),
                Err(reason) => no_verdict(reason),
            }
        }
    }
}

/// The reach of a Conclusion whose claim may not be a verdict: the kernel's
/// `cutoff_of` reads only the members every Conclusion carries, so it is
/// invoked on a stand-in carrying the agreement draw as its claim.
fn cutoff_of_offered(
    params: &sashite_sanki_session::session::SessionParams,
    attestations: &[sashite_sanki_session::event::Attestation],
    offered: &ConclusionOffered,
) -> Result<(Side, Timestamp), NoVerdict> {
    use sashite_sanki_engine::domain::status::{Outcome3, Status};
    use sashite_sanki_session::event::Conclusion;
    use sashite_sanki_session::verdict::Verdict;
    let stand_in = Verdict::new(Status::Agreement, Outcome3::Draw).map(|claim| {
        Conclusion::new(
            offered.id,
            offered.signer,
            offered.session,
            claim,
            offered.created_at,
        )
    });
    match stand_in {
        Some(conclusion) => cutoff_of(params, attestations, &conclusion),
        None => Err(NoVerdict::Inconsistent),
    }
}

/// `select_conclusion`.
pub fn select_conclusion_op(req: SelectConclusionReq) -> Result<Value, Fault> {
    let params = request::session_params(&req.session)?;
    let attestations = request::attestations(&req.attestations)?;
    let plies = request::plies(req.plies, MAX_STEP)?;
    let mut typed = Vec::new();
    for (index, conclusion) in req.conclusions.iter().enumerate() {
        let offered = request::conclusion(conclusion, &format!("conclusions[{index}]"))?;
        match offered.typed() {
            Some(conclusion) => typed.push(conclusion),
            // A claim the kernel never yields is never canonical; its replay
            // is still performed when it is in reach, so that an inconsistency
            // is reported rather than routed around.
            None => {
                if let Ok((invoker, cutoff)) = cutoff_of_offered(&params, &attestations, &offered) {
                    if let Err(NoVerdict::Inconsistent) =
                        verdict_at(&params, &plies, &attestations, invoker, cutoff)
                    {
                        return Err(Fault::Inconsistent);
                    }
                }
            }
        }
    }
    match select_conclusion(&params, &plies, &attestations, &typed) {
        Ok(Some(canonical)) => Ok(json!({
            "canonical": {
                "cutoff": encode::timing(canonical.cutoff),
                "id": canonical.conclusion.id.to_string(),
                "verdict": encode::verdict(&canonical.verdict),
            }
        })),
        Ok(None) => Ok(json!({ "canonical": Value::Null })),
        Err(_) => Err(Fault::Inconsistent),
    }
}

// ---- corpus primitives (test-only, non-normative) ----------------------------

/// `select`.
pub fn select(req: &SelectReq) -> Result<Value, Fault> {
    let boundary = request::timing(req.boundary, "boundary")?;
    let cap = request::cap(req.cap)?;
    let mut candidates = Vec::with_capacity(req.candidates.len());
    let mut legality: Vec<(String, bool)> = Vec::with_capacity(req.candidates.len());
    for (index, candidate) in req.candidates.iter().enumerate() {
        let created_at = request::timing(
            candidate.created_at,
            &format!("candidates[{index}].created_at"),
        )?;
        candidates.push(Candidate {
            id: candidate.id.clone(),
            created_at,
        });
        legality.push((candidate.id.clone(), candidate.legal));
    }
    let probe = |id: &String| legality.iter().any(|(known, legal)| known == id && *legal);
    let selection = select_candidate(boundary, &candidates, cap, probe);
    let (result, selected) = match selection.selected() {
        Some(candidate) => ("applied", Value::from(candidate.id.clone())),
        None => ("unfilled", Value::Null),
    };
    Ok(json!({ "result": result, "selected": selected }))
}

/// `max(0, timing − anchor)` — the clamp the kernel's step applies
/// (Time Accounting — Sanki §Elapsed time).
fn elapsed_between(anchor: Timestamp, timing: Timestamp) -> Duration {
    timing.duration_since(anchor).unwrap_or(Duration::ZERO)
}

/// `elapsed`.
pub fn elapsed(req: &ElapsedReq) -> Result<Value, Fault> {
    let anchor = request::timing(req.anchor, "anchor")?;
    let timing = request::timing(req.timing, "timing")?;
    Ok(json!({ "elapsed": encode::duration(elapsed_between(anchor, timing)) }))
}

/// `charges`.
pub fn charges(req: &ChargesReq) -> Result<Value, Fault> {
    let limit = usize::try_from(MAX_STEP.saturating_mul(2)).unwrap_or(usize::MAX);
    if req.timings.len() > limit {
        return Err(Fault::Malformed(format!(
            "timings holds more than {limit} entries"
        )));
    }
    let mut anchor = request::timing(req.start, "start")?;
    let mut first: Vec<Value> = Vec::new();
    let mut second: Vec<Value> = Vec::new();
    for (index, value) in req.timings.iter().enumerate() {
        let timing = request::timing(*value, &format!("timings[{index}]"))?;
        let charge = encode::duration(elapsed_between(anchor, timing));
        if index % 2 == 0 {
            first.push(charge);
        } else {
            second.push(charge);
        }
        anchor = anchor.max(timing);
    }
    let on_move = if req.timings.len() % 2 == 0 {
        Side::First
    } else {
        Side::Second
    };
    Ok(json!({
        "anchor": encode::timing(anchor),
        "first": first,
        "on_move": encode::side(on_move),
        "second": second,
    }))
}

/// `clock`.
pub fn clock(req: &ClockReq) -> Result<Value, Fault> {
    let control = request::time_control(&req.time_control)?;
    let before = request::clock(&req.clock)?;
    let spent = request::duration(req.elapsed, "elapsed")?;
    Ok(match tick(&control, before, spent) {
        Tick::Flagged => json!({ "kind": "flagged" }),
        Tick::Continued(after) => json!({ "clock": encode::clock(after), "kind": "continued" }),
    })
}
