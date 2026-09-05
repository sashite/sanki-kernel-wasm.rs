//! One request in, one response out — the module's whole behaviour as a pure
//! function of bytes, shared by the WebAssembly exports and by the native
//! tests.

use crate::fault::Fault;
use crate::json::{validate_request, Malformed};
use crate::ops;
use crate::request::{
    ApplyReq, ChargesReq, CheckReq, ClockReq, DescribeReq, ElapsedReq, GgnReq, NaturalStateReq,
    OpProbe, PositionReq, SelectConclusionReq, SelectReq, VerdictAtReq,
};
use serde::de::DeserializeOwned;
use serde_json::Value;

/// The request bound: 32 MiB.
pub const REQUEST_BOUND: usize = 33_554_432;
/// The response bound: 4 MiB.
pub const RESPONSE_BOUND: usize = 4_194_304;

fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Fault> {
    serde_json::from_slice(bytes).map_err(|error| Fault::Malformed(error.to_string()))
}

fn describe_malformed(reason: Malformed) -> Fault {
    let detail = match reason {
        Malformed::Encoding => "not UTF-8, or a byte-order mark",
        Malformed::Syntax => "not well-formed JSON",
        Malformed::NotAnObject => "the top-level value is not an object",
        Malformed::TooDeep => "nesting deeper than eight levels",
        Malformed::DuplicateMember => "a duplicate member name",
        Malformed::Number => "a number with a fraction, an exponent, a leading zero, or -0",
        Malformed::Str => {
            "an invalid escape, an unpaired surrogate, or a control character in a string"
        }
        Malformed::Trailing => "bytes after the top-level object",
    };
    Fault::Malformed(detail.to_owned())
}

/// Answers a request within bound: the operation's result, or an error value.
///
/// # Errors
///
/// The [`Fault`] the request earns; the caller encodes it.
pub fn evaluate(bytes: &[u8]) -> Result<Value, Fault> {
    if bytes.len() > REQUEST_BOUND {
        return Err(Fault::Oversized);
    }
    validate_request(bytes).map_err(describe_malformed)?;
    let probe: OpProbe = parse(bytes)?;
    match probe.op.as_str() {
        "describe" => {
            let _typed: DescribeReq = parse(bytes)?;
            Ok(ops::describe())
        }
        "ggn" => ops::ggn_table(&parse::<GgnReq>(bytes)?),
        "legal_moves" => ops::legal_moves_of(&parse::<PositionReq>(bytes)?),
        "apply" => ops::apply(&parse::<ApplyReq>(bytes)?),
        "classify" => ops::classify(&parse::<PositionReq>(bytes)?),
        "natural_state" => ops::natural_state_op(parse::<NaturalStateReq>(bytes)?),
        "verdict_at" => ops::verdict_at_op(parse::<VerdictAtReq>(bytes)?),
        "check" => ops::check_op(parse::<CheckReq>(bytes)?),
        "select_conclusion" => ops::select_conclusion_op(parse::<SelectConclusionReq>(bytes)?),
        "select" => ops::select(&parse::<SelectReq>(bytes)?),
        "elapsed" => ops::elapsed(&parse::<ElapsedReq>(bytes)?),
        "charges" => ops::charges(&parse::<ChargesReq>(bytes)?),
        "clock" => ops::clock(&parse::<ClockReq>(bytes)?),
        other => Err(Fault::UnsupportedOp(other.to_owned())),
    }
}

/// The `oversized` error response, for a `call` whose length exceeds the
/// bound before any request is read.
#[must_use]
pub fn oversized() -> Vec<u8> {
    serde_json::to_vec(&Fault::Oversized.to_value()).unwrap_or_default()
}

/// The response bytes for a request: canonical JSON of the result or of the
/// error — always a response, never a panic. `None` only when the response
/// could not be serialised or exceeds the bound, which is a build defect the
/// exports report as no answer.
#[must_use]
pub fn answer(bytes: &[u8]) -> Option<Vec<u8>> {
    let value = match evaluate(bytes) {
        Ok(value) => value,
        Err(fault) => fault.to_value(),
    };
    let out = serde_json::to_vec(&value).ok()?;
    (out.len() <= RESPONSE_BOUND).then_some(out)
}
