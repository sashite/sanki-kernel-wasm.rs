//! The typed side of a request (Kernel ABI — Sanki §Encodings, §Operations):
//! the shapes serde reads once the lexical scan admitted the bytes, the value
//! classes with their ranges, and the conversion of the wire model into the
//! kernel's own types.
//!
//! Every refusal here is `malformed` (§The request and the response): a
//! missing, unknown or wrongly-shaped member, an out-of-range integer, a
//! malformed id, an invalid time control, equal seats. What is *not* refused
//! here — a Ply whose content does not parse, a Conclusion whose claim the
//! kernel never yields — is the module's to judge later, as the ABI requires.

use crate::fault::Fault;
use sashite_sanki_engine::domain::side::Side;
use sashite_sanki_engine::domain::status::{Outcome3, Status};
use sashite_sanki_engine::domain::time::{Duration, Timestamp};
use sashite_sanki_engine::domain::time_control::{Clock, Period, TimeControl};
use sashite_sanki_engine::position::Position;
use sashite_sanki_session::event::{Attestation, Conclusion, EventId, Ply, PublicKey};
use sashite_sanki_session::session::{Seats, SessionParams};
use sashite_sanki_session::verdict::Verdict;
use serde::Deserialize;
use std::num::NonZeroUsize;

/// The exclusive upper bound of timings and durations: `2^40` seconds.
pub const TIME_BOUND: i64 = 1 << 40;
/// The exclusive upper bound of counts (`step`, `cap`, `period`, `plies`).
pub const COUNT_BOUND: i64 = 1 << 31;

/// The `op` of a request, read first to pick the operation's shape.
#[derive(Deserialize)]
pub struct OpProbe {
    /// The operation name.
    pub op: String,
}

/// A member that must be present but may be `null` — serde treats a missing
/// `Option` field as `None`, which this newtype does not.
#[derive(Deserialize)]
pub struct Nullable<T>(pub Option<T>);

/// A period triple `[duration, increment, plies]`, `null` for an absent option.
pub type Triple = (i64, Option<i64>, Option<i64>);

/// `{ "op": "describe" }`
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DescribeReq {
    /// `describe`
    pub op: String,
}

/// `{ "op": "ggn", "variant": … }`
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GgnReq {
    /// `ggn`
    pub op: String,
    /// The variant identifier.
    pub variant: String,
}

/// `{ "op": "legal_moves" | "classify", "position": … }`
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PositionReq {
    /// The operation.
    pub op: String,
    /// A FEEN string.
    pub position: String,
}

/// `{ "op": "apply", "position": …, "move": … }`
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyReq {
    /// `apply`
    pub op: String,
    /// A FEEN string.
    pub position: String,
    /// A Ply's `content` string.
    #[serde(rename = "move")]
    pub mv: String,
}

/// The session terms (§The session terms).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionTermsIn {
    /// The Game Session's event id.
    pub id: String,
    /// The player seated `first`.
    pub first: String,
    /// The player seated `second`.
    pub second: String,
    /// The designated timestamper, or `null` in self-timed mode.
    pub timestamper: Nullable<String>,
    /// The time control.
    pub time_control: Vec<Triple>,
    /// The initial position.
    pub position: String,
    /// t₀.
    pub start: i64,
}

/// A Ply in the abstract model (§The events).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlyIn {
    /// The event id.
    pub id: String,
    /// The signer.
    pub signer: String,
    /// The Game Session referenced.
    pub session: String,
    /// The signer's move ordinal.
    pub step: i64,
    /// The `draw` flag.
    pub draw: bool,
    /// The `content`, verbatim.
    pub content: String,
    /// The event's own `created_at`.
    pub created_at: i64,
}

/// An attestation in the abstract model.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttestationIn {
    /// The attestation's id.
    pub id: String,
    /// Its signer.
    pub signer: String,
    /// The event it attests.
    pub attests: String,
    /// Its `created_at`.
    pub created_at: i64,
}

/// A result on the seat axis.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultIn {
    /// The score of the player seated `first`.
    pub first: i64,
    /// The score of the player seated `second`.
    pub second: i64,
}

/// A Conclusion in the abstract model, its claim in kind `3425`'s own domain.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConclusionIn {
    /// The event id.
    pub id: String,
    /// The signer.
    pub signer: String,
    /// The Game Session referenced.
    pub session: String,
    /// The status claimed (the event's `content`).
    pub status: String,
    /// The result claimed, on the seat axis.
    pub result: ResultIn,
    /// The event's own `created_at`.
    pub created_at: i64,
}

/// `{ "op": "natural_state", … }`
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NaturalStateReq {
    /// `natural_state`
    pub op: String,
    /// The session terms.
    pub session: SessionTermsIn,
    /// The Plies offered.
    pub plies: Vec<PlyIn>,
    /// The attestations offered.
    pub attestations: Vec<AttestationIn>,
    /// The cutoff.
    pub cutoff: i64,
}

/// `{ "op": "verdict_at", … }`
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerdictAtReq {
    /// `verdict_at`
    pub op: String,
    /// The session terms.
    pub session: SessionTermsIn,
    /// The Plies offered.
    pub plies: Vec<PlyIn>,
    /// The attestations offered.
    pub attestations: Vec<AttestationIn>,
    /// The concluding side.
    pub invoker: String,
    /// The cutoff.
    pub cutoff: i64,
}

/// `{ "op": "check", … }`
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckReq {
    /// `check`
    pub op: String,
    /// The session terms.
    pub session: SessionTermsIn,
    /// The Plies offered.
    pub plies: Vec<PlyIn>,
    /// The attestations offered.
    pub attestations: Vec<AttestationIn>,
    /// The Conclusion examined.
    pub conclusion: ConclusionIn,
}

/// `{ "op": "select_conclusion", … }`
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectConclusionReq {
    /// `select_conclusion`
    pub op: String,
    /// The session terms.
    pub session: SessionTermsIn,
    /// The Plies offered.
    pub plies: Vec<PlyIn>,
    /// The attestations offered.
    pub attestations: Vec<AttestationIn>,
    /// The Conclusions offered.
    pub conclusions: Vec<ConclusionIn>,
}

/// A slot candidate for `select`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateIn {
    /// An arbitrary id.
    pub id: String,
    /// The candidate's canonical timing.
    pub created_at: i64,
    /// Its legality, taken as given.
    pub legal: bool,
}

/// `{ "op": "select", … }`
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectReq {
    /// `select`
    pub op: String,
    /// The boundary `T`.
    pub boundary: i64,
    /// The cap `K`.
    pub cap: i64,
    /// The slot's candidates.
    pub candidates: Vec<CandidateIn>,
}

/// `{ "op": "elapsed", … }`
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElapsedReq {
    /// `elapsed`
    pub op: String,
    /// The anchor.
    pub anchor: i64,
    /// The timing.
    pub timing: i64,
}

/// `{ "op": "charges", … }`
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChargesReq {
    /// `charges`
    pub op: String,
    /// t₀.
    pub start: i64,
    /// The chain's per-Ply timings in play order.
    pub timings: Vec<i64>,
}

/// A clock value.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClockIn {
    /// The 0-based period index.
    pub period: i64,
    /// The plies played in the period.
    pub plies_in_period: i64,
    /// The seconds remaining.
    pub remaining: i64,
}

/// `{ "op": "clock", … }`
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClockReq {
    /// `clock`
    pub op: String,
    /// The time control.
    pub time_control: Vec<Triple>,
    /// The mover's clock before the ply.
    pub clock: ClockIn,
    /// The seconds the ply consumed.
    pub elapsed: i64,
}

// ---- value classes ---------------------------------------------------------

/// A timing or a duration, in `[0, 2^40)`.
pub fn timing(value: i64, what: &str) -> Result<Timestamp, Fault> {
    if (0..TIME_BOUND).contains(&value) {
        Ok(Timestamp::from_unix(value))
    } else {
        Err(Fault::Malformed(format!("{what} is not in [0, 2^40)")))
    }
}

/// A duration, in `[0, 2^40)`.
pub fn duration(value: i64, what: &str) -> Result<Duration, Fault> {
    let seconds =
        u64::try_from(value).map_err(|_| Fault::Malformed(format!("{what} is negative")))?;
    if value < TIME_BOUND {
        Ok(Duration::from_secs(seconds))
    } else {
        Err(Fault::Malformed(format!("{what} is not in [0, 2^40)")))
    }
}

/// A count in `[low, 2^31)`.
pub fn count(value: i64, low: i64, what: &str) -> Result<u32, Fault> {
    if value >= low && value < COUNT_BOUND {
        u32::try_from(value).map_err(|_| Fault::Malformed(format!("{what} is out of range")))
    } else {
        Err(Fault::Malformed(format!("{what} is not in [{low}, 2^31)")))
    }
}

/// `^[0-9a-f]{64}$` — lowercase, as the ABI requires (the kernel's parsers
/// would accept uppercase).
fn is_lower_hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A 64-hex event id.
pub fn event_id(value: &str, what: &str) -> Result<EventId, Fault> {
    if !is_lower_hex64(value) {
        return Err(Fault::Malformed(format!("{what} is not a 64-hex event id")));
    }
    EventId::parse(value)
        .ok_or_else(|| Fault::Malformed(format!("{what} is not a 64-hex event id")))
}

/// A 64-hex pubkey.
pub fn pubkey(value: &str, what: &str) -> Result<PublicKey, Fault> {
    if !is_lower_hex64(value) {
        return Err(Fault::Malformed(format!("{what} is not a 64-hex pubkey")));
    }
    PublicKey::parse(value)
        .ok_or_else(|| Fault::Malformed(format!("{what} is not a 64-hex pubkey")))
}

/// A side name.
pub fn side(value: &str, what: &str) -> Result<Side, Fault> {
    match value {
        "first" => Ok(Side::First),
        "second" => Ok(Side::Second),
        _ => Err(Fault::Malformed(format!(
            "{what} is neither \"first\" nor \"second\""
        ))),
    }
}

/// A FEEN string the module can parse.
pub fn position(value: &str) -> Result<Position, Fault> {
    Position::parse(value).map_err(|error| Fault::InvalidPosition(format!("{error:?}")))
}

/// A time control valid per kind `3420` §Match-terms tags.
pub fn time_control(triples: &[Triple]) -> Result<TimeControl, Fault> {
    let mut periods = Vec::with_capacity(triples.len());
    for (index, (duration_secs, increment, plies)) in triples.iter().enumerate() {
        let what = format!("time_control[{index}]");
        let bank = duration(*duration_secs, &what)?;
        let increment = match increment {
            Some(seconds) => Some(duration(*seconds, &what)?),
            None => None,
        };
        let plies = match plies {
            Some(n) => Some(count(*n, 1, &what)?),
            None => None,
        };
        // Kind 3420 §Match-terms tags: a `duration` of 0 is valid only in the
        // three-element per-move form — a shape of the founding term, checked
        // here before the engine reads the period.
        if *duration_secs == 0 && (increment.is_none() || plies.is_none()) {
            return Err(Fault::Malformed(format!(
                "{what} has a duration of 0 outside the three-element form"
            )));
        }
        let period = Period::new(bank, increment, plies)
            .map_err(|error| Fault::Malformed(format!("{what} is invalid: {error}")))?;
        periods.push(period);
    }
    TimeControl::from_periods(periods)
        .map_err(|error| Fault::Malformed(format!("time_control is invalid: {error}")))
}

/// A clock value.
pub fn clock(value: &ClockIn) -> Result<Clock, Fault> {
    let remaining = duration(value.remaining, "clock.remaining")?;
    let period = count(value.period, 0, "clock.period")?;
    let plies = count(value.plies_in_period, 0, "clock.plies_in_period")?;
    let period = usize::try_from(period)
        .map_err(|_| Fault::Malformed("clock.period is out of range".into()))?;
    Ok(Clock::new(remaining, period, plies))
}

/// The cap `K` of `select`.
pub fn cap(value: i64) -> Result<NonZeroUsize, Fault> {
    let n = count(value, 1, "cap")?;
    let n = usize::try_from(n).map_err(|_| Fault::Malformed("cap is out of range".into()))?;
    NonZeroUsize::new(n).ok_or_else(|| Fault::Malformed("cap is zero".into()))
}

// ---- the session's model ----------------------------------------------------

/// A claim as offered, kept for echoing; and the verdict it is, if any.
#[derive(Debug, Clone)]
pub struct Claim {
    /// The status claimed, verbatim.
    pub status: String,
    /// The scores claimed, `first` then `second`.
    pub scores: (i64, i64),
    /// The verdict the claim is, when the kernel can yield it.
    pub verdict: Option<Verdict>,
}

/// A Conclusion offered: the kernel's model when its claim is a verdict, the
/// raw claim otherwise — with what every Conclusion carries for the reach test.
pub struct ConclusionOffered {
    /// The event id.
    pub id: EventId,
    /// The signer.
    pub signer: PublicKey,
    /// The Game Session referenced.
    pub session: EventId,
    /// The claim.
    pub claim: Claim,
    /// The event's own `created_at`.
    pub created_at: Timestamp,
}

impl ConclusionOffered {
    /// The kernel's Conclusion, when the claim is a verdict the kernel can
    /// yield.
    #[must_use]
    pub fn typed(&self) -> Option<Conclusion> {
        self.claim.verdict.map(|claim| {
            Conclusion::new(self.id, self.signer, self.session, claim, self.created_at)
        })
    }
}

/// The claim's domain is kind `3425`'s: a status matching `^[a-z]{1,32}$` and
/// two non-negative integers summing to `100`. Whether it is a verdict the
/// kernel can yield is a separate question, answered by [`Claim::verdict`].
pub fn claim(status: &str, result: &ResultIn) -> Result<Claim, Fault> {
    let well_formed =
        (1..=32).contains(&status.len()) && status.bytes().all(|b| b.is_ascii_lowercase());
    if !well_formed {
        return Err(Fault::Malformed(
            "conclusion.status does not match ^[a-z]{1,32}$".into(),
        ));
    }
    let (first, second) = (result.first, result.second);
    if first < 0 || second < 0 || first.saturating_add(second) != 100 {
        return Err(Fault::Malformed(
            "conclusion.result is not two non-negative integers summing to 100".into(),
        ));
    }
    let outcome = match (first, second) {
        (100, 0) => Some(Outcome3::FirstWins),
        (0, 100) => Some(Outcome3::SecondWins),
        (50, 50) => Some(Outcome3::Draw),
        _ => None,
    };
    let verdict = match (Status::parse(status), outcome) {
        (Ok(status), Some(outcome)) => Verdict::new(status, outcome),
        _ => None,
    };
    Ok(Claim {
        status: status.to_owned(),
        scores: (first, second),
        verdict,
    })
}

/// The session's terms as the kernel reads them.
pub fn session_params(terms: &SessionTermsIn) -> Result<SessionParams, Fault> {
    let id = event_id(&terms.id, "session.id")?;
    let first = pubkey(&terms.first, "session.first")?;
    let second = pubkey(&terms.second, "session.second")?;
    let timestamper = match &terms.timestamper.0 {
        Some(key) => Some(pubkey(key, "session.timestamper")?),
        None => None,
    };
    let seats = Seats::new(first, second).ok_or_else(|| {
        Fault::Malformed("session.first and session.second are the same pubkey".into())
    })?;
    let control = time_control(&terms.time_control)?;
    let initial = position(&terms.position)?;
    let start = timing(terms.start, "session.start")?;
    SessionParams::new(id, timestamper, seats, control, initial, start)
        .ok_or_else(|| Fault::InvalidPosition("the initial position has second to move".into()))
}

/// The Plies offered, as the kernel reads them; a `step` beyond `max_step`
/// is dropped here — it can enter no chain (§The events).
pub fn plies(offered: Vec<PlyIn>, max_step: u32) -> Result<Vec<Ply>, Fault> {
    let mut out = Vec::with_capacity(offered.len());
    for (index, ply) in offered.into_iter().enumerate() {
        let what = format!("plies[{index}]");
        let step = count(ply.step, 1, &format!("{what}.step"))?;
        let created_at = timing(ply.created_at, &format!("{what}.created_at"))?;
        let id = event_id(&ply.id, &format!("{what}.id"))?;
        let signer = pubkey(&ply.signer, &format!("{what}.signer"))?;
        let session = event_id(&ply.session, &format!("{what}.session"))?;
        if step > max_step {
            continue;
        }
        out.push(Ply::new(
            id,
            signer,
            session,
            step,
            ply.draw,
            ply.content,
            created_at,
        ));
    }
    Ok(out)
}

/// The attestations offered, as the kernel reads them.
pub fn attestations(offered: &[AttestationIn]) -> Result<Vec<Attestation>, Fault> {
    let mut out = Vec::with_capacity(offered.len());
    for (index, attestation) in offered.iter().enumerate() {
        let what = format!("attestations[{index}]");
        out.push(Attestation::new(
            event_id(&attestation.id, &format!("{what}.id"))?,
            pubkey(&attestation.signer, &format!("{what}.signer"))?,
            event_id(&attestation.attests, &format!("{what}.attests"))?,
            timing(attestation.created_at, &format!("{what}.created_at"))?,
        ));
    }
    Ok(out)
}

/// A Conclusion offered.
pub fn conclusion(offered: &ConclusionIn, what: &str) -> Result<ConclusionOffered, Fault> {
    Ok(ConclusionOffered {
        id: event_id(&offered.id, &format!("{what}.id"))?,
        signer: pubkey(&offered.signer, &format!("{what}.signer"))?,
        session: event_id(&offered.session, &format!("{what}.session"))?,
        claim: claim(&offered.status, &offered.result)?,
        created_at: timing(offered.created_at, &format!("{what}.created_at"))?,
    })
}
