//! The typed errors of the ABI (Kernel ABI — Sanki §The request and the
//! response) and their wire form.

use serde_json::{json, Value};

/// An error response: one of the ABI's codes and a free-text message a
/// consumer never keys behaviour on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// The request exceeds the bound.
    Oversized,
    /// The request is not a JSON object of this ABI.
    Malformed(String),
    /// The `op` is not one of this ABI's.
    UnsupportedOp(String),
    /// A variant identifier the module does not define (`ggn` only).
    UnknownVariant(String),
    /// A FEEN string the module cannot parse, or a session position with
    /// `second` to move.
    InvalidPosition(String),
    /// The move is not legal in the position (`apply` only).
    Illegal(String),
    /// The replay hit a broken internal invariant: no answer is defined.
    Inconsistent,
}

impl Fault {
    /// The error code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Oversized => "oversized",
            Self::Malformed(_) => "malformed",
            Self::UnsupportedOp(_) => "unsupported_op",
            Self::UnknownVariant(_) => "unknown_variant",
            Self::InvalidPosition(_) => "invalid_position",
            Self::Illegal(_) => "illegal",
            Self::Inconsistent => "inconsistent",
        }
    }

    /// The message, documentary.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::Oversized => "the request exceeds the bound".to_owned(),
            Self::Malformed(detail) => format!("malformed request: {detail}"),
            Self::UnsupportedOp(op) => format!("unsupported operation: {op}"),
            Self::UnknownVariant(variant) => format!("unknown variant: {variant}"),
            Self::InvalidPosition(detail) => format!("invalid position: {detail}"),
            Self::Illegal(reason) => format!("illegal move: {reason}"),
            Self::Inconsistent => {
                "the replay hit a broken internal invariant: no answer is defined".to_owned()
            }
        }
    }

    /// The wire form, `{ "error": { "code": …, "message": … } }`.
    #[must_use]
    pub fn to_value(&self) -> Value {
        json!({ "error": { "code": self.code(), "message": self.message() } })
    }
}
