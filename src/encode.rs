//! The response side (Kernel ABI — Sanki §Values): the kernel's values in
//! their wire form. Responses are built as [`serde_json::Value`] objects,
//! whose map is ordered by key, and serialised compactly — the canonical form
//! of §JSON (sorted members, no insignificant whitespace, minimal escaping with
//! lowercase `\u00xx`, non-ASCII raw), which `serde_json` produces as is.

use sashite_sanki_engine::domain::half_move::Move;
use sashite_sanki_engine::domain::outcome::Verdict as EngineVerdict;
use sashite_sanki_engine::domain::side::Side;
use sashite_sanki_engine::domain::status::Status;
use sashite_sanki_engine::domain::time::{Duration, Timestamp};
use sashite_sanki_engine::domain::time_control::Clock;
use sashite_sanki_session::verdict::Verdict;
use serde_json::{json, Value};

/// A timing.
#[must_use]
pub fn timing(at: Timestamp) -> Value {
    Value::from(at.as_unix())
}

/// A duration.
#[must_use]
pub fn duration(d: Duration) -> Value {
    Value::from(d.as_secs())
}

/// A side name.
#[must_use]
pub const fn side(side: Side) -> &'static str {
    match side {
        Side::First => "first",
        Side::Second => "second",
    }
}

/// A status token.
#[must_use]
pub fn status(status: Status) -> Value {
    Value::from(status.to_string())
}

/// A position's intrinsic status: `ongoing` or the terminal token.
#[must_use]
pub fn intrinsic(verdict: &EngineVerdict) -> Value {
    match verdict {
        EngineVerdict::Ongoing => Value::from("ongoing"),
        EngineVerdict::Terminated { status, .. } => self::status(*status),
    }
}

/// A verdict: `{ "result": { "first", "second" }, "status" }`.
#[must_use]
pub fn verdict(verdict: &Verdict) -> Value {
    json!({
        "result": {
            "first": verdict.score(Side::First),
            "second": verdict.score(Side::Second),
        },
        "status": verdict.status().to_string(),
    })
}

/// A clock: `{ "period", "plies_in_period", "remaining" }`.
#[must_use]
pub fn clock(clock: Clock) -> Value {
    json!({
        "period": clock.period_index(),
        "plies_in_period": clock.plies_in_period(),
        "remaining": clock.remaining().as_secs(),
    })
}

/// A move as the `content` string a Ply carrying it would have — the
/// `[source, destination, actor]` array in compact JSON.
#[must_use]
pub fn content(mv: &Move) -> String {
    let array = match mv {
        Move::Board { from, to, actor } => json!([
            from.to_string(),
            to.to_string(),
            actor.as_ref().map(ToString::to_string),
        ]),
        Move::Drop { piece, to } => json!([Value::Null, to.to_string(), piece.to_string()]),
    };
    array.to_string()
}

/// The canonical bytes of a response value.
///
/// # Errors
///
/// Never in practice: every value built here serialises; the error is kept
/// so that the caller decides what a failure means rather than a panic.
pub fn canonical(value: &Value) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(value)
}
