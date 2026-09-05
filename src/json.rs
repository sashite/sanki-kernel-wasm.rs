//! The request's **strictness** (Kernel ABI — Sanki §JSON), checked on the raw
//! bytes before any typed parsing.
//!
//! JSON's grammar (RFC 8259) admits more than the ABI does — duplicate member
//! names, fractions and exponents, negative zero, a leading byte-order mark,
//! unbounded nesting — and a lenient parser would silently normalise them,
//! which two consumers could do differently. This scanner walks the request
//! once and refuses, as `malformed`, everything the ABI names: nesting deeper
//! than [`MAX_DEPTH`], a duplicate member name in one object, a number not
//! matching `^-?(0|[1-9][0-9]*)$` (negative zero included), an invalid or
//! unpaired escape, a control character in a string, invalid UTF-8, a BOM,
//! anything but whitespace after the top-level value, and a top-level value
//! that is not an object.
//!
//! It validates and never builds: the typed parse that follows
//! ([`crate::request`]) sees only what this scanner admitted, so it never meets
//! a fraction or a duplicate, and its own refusals — a missing or unknown
//! member, a wrong shape, an out-of-range value — are the remaining `malformed`
//! cases.
//!
//! The scanner is iterative: the nesting depth is a counter, not a call stack.

use std::collections::BTreeSet;

/// The nesting the ABI accepts: eight levels, the deepest schema having four.
pub const MAX_DEPTH: usize = 8;

/// Why a request is malformed at the lexical level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Malformed {
    /// The bytes are not valid UTF-8, or start with a byte-order mark.
    Encoding,
    /// The text is not well-formed JSON (unexpected byte, unterminated value).
    Syntax,
    /// The top-level value is not an object.
    NotAnObject,
    /// Nesting exceeds [`MAX_DEPTH`].
    TooDeep,
    /// An object carries the same member name twice.
    DuplicateMember,
    /// A number carries a fraction, an exponent, a leading zero or is `-0`.
    Number,
    /// A string carries an invalid escape, an unpaired surrogate, or a raw
    /// control character.
    Str,
    /// Bytes other than whitespace follow the top-level value.
    Trailing,
}

/// One frame of the iterative walk: the container being read and, for an
/// object, the member names seen so far.
enum Frame {
    Object {
        keys: BTreeSet<String>,
        expect_key: bool,
    },
    Array,
}

/// A cursor over the request bytes.
struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn bump(&mut self) {
        self.pos = self.pos.saturating_add(1);
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.bump();
        }
    }

    fn expect_byte(&mut self, byte: u8) -> Result<(), Malformed> {
        if self.peek() == Some(byte) {
            self.bump();
            Ok(())
        } else {
            Err(Malformed::Syntax)
        }
    }

    fn literal(&mut self, word: &[u8]) -> Result<(), Malformed> {
        let end = self.pos.saturating_add(word.len());
        if self.bytes.get(self.pos..end) == Some(word) {
            self.pos = end;
            Ok(())
        } else {
            Err(Malformed::Syntax)
        }
    }

    /// A number: `-?(0|[1-9][0-9]*)` and nothing more — no fraction, no
    /// exponent, no leading zero, no negative zero.
    fn number(&mut self) -> Result<(), Malformed> {
        let negative = self.peek() == Some(b'-');
        if negative {
            self.bump();
        }
        match self.peek() {
            Some(b'0') => {
                self.bump();
                if negative {
                    return Err(Malformed::Number);
                }
            }
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.bump();
                }
            }
            _ => return Err(Malformed::Number),
        }
        match self.peek() {
            Some(b'.' | b'e' | b'E' | b'0'..=b'9') => Err(Malformed::Number),
            _ => Ok(()),
        }
    }

    /// A string, after its opening quote; returns the decoded text.
    fn string(&mut self) -> Result<String, Malformed> {
        let mut out = String::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err(Malformed::Syntax);
            };
            self.bump();
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let Some(escaped) = self.peek() else {
                        return Err(Malformed::Str);
                    };
                    self.bump();
                    match escaped {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let first = self.hex4()?;
                            let ch = match first {
                                0xD800..=0xDBFF => {
                                    // A high surrogate must be followed by an
                                    // escaped low surrogate.
                                    self.expect_byte(b'\\').map_err(|_| Malformed::Str)?;
                                    self.expect_byte(b'u').map_err(|_| Malformed::Str)?;
                                    let second = self.hex4()?;
                                    if !(0xDC00..=0xDFFF).contains(&second) {
                                        return Err(Malformed::Str);
                                    }
                                    let high = u32::from(first).saturating_sub(0xD800) << 10;
                                    let low = u32::from(second).saturating_sub(0xDC00);
                                    char::from_u32(0x10000_u32.saturating_add(high | low))
                                        .ok_or(Malformed::Str)?
                                }
                                0xDC00..=0xDFFF => return Err(Malformed::Str),
                                code => char::from_u32(u32::from(code)).ok_or(Malformed::Str)?,
                            };
                            out.push(ch);
                        }
                        _ => return Err(Malformed::Str),
                    }
                }
                0x00..=0x1F => return Err(Malformed::Str),
                _ => {
                    // Raw UTF-8: the whole request was checked to be valid
                    // UTF-8, so continuation bytes follow; copy the scalar.
                    let start = self.pos.saturating_sub(1);
                    let width = utf8_width(byte).ok_or(Malformed::Encoding)?;
                    let end = start.saturating_add(width);
                    let chunk = self.bytes.get(start..end).ok_or(Malformed::Encoding)?;
                    let text = core::str::from_utf8(chunk).map_err(|_| Malformed::Encoding)?;
                    out.push_str(text);
                    self.pos = end;
                }
            }
        }
    }

    fn hex4(&mut self) -> Result<u16, Malformed> {
        let mut value: u16 = 0;
        for _ in 0..4 {
            let Some(byte) = self.peek() else {
                return Err(Malformed::Str);
            };
            self.bump();
            let digit = match byte {
                b'0'..=b'9' => byte.saturating_sub(b'0'),
                b'a'..=b'f' => byte.saturating_sub(b'a').saturating_add(10),
                b'A'..=b'F' => byte.saturating_sub(b'A').saturating_add(10),
                _ => return Err(Malformed::Str),
            };
            value = (value << 4) | u16::from(digit);
        }
        Ok(value)
    }
}

/// The byte width of a UTF-8 scalar from its first byte.
const fn utf8_width(first: u8) -> Option<usize> {
    match first {
        0x00..=0x7F => Some(1),
        0xC2..=0xDF => Some(2),
        0xE0..=0xEF => Some(3),
        0xF0..=0xF4 => Some(4),
        _ => None,
    }
}

/// Checks that `bytes` are a request the ABI admits at the lexical level.
///
/// # Errors
///
/// The first [`Malformed`] reason met, scanning left to right.
pub fn validate_request(bytes: &[u8]) -> Result<(), Malformed> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Err(Malformed::Encoding);
    }
    core::str::from_utf8(bytes).map_err(|_| Malformed::Encoding)?;

    let mut cursor = Cursor { bytes, pos: 0 };
    let mut stack: Vec<Frame> = Vec::new();

    cursor.skip_ws();
    if cursor.peek() != Some(b'{') {
        return Err(if cursor.peek().is_none() {
            Malformed::Syntax
        } else {
            Malformed::NotAnObject
        });
    }

    // The walk: `value` reads one value at the cursor; a container pushes a
    // frame and continues; a closing bracket pops one.
    let mut need_value = true;
    loop {
        cursor.skip_ws();
        if need_value {
            match cursor.peek() {
                Some(b'{') => {
                    cursor.bump();
                    if stack.len() >= MAX_DEPTH {
                        return Err(Malformed::TooDeep);
                    }
                    stack.push(Frame::Object {
                        keys: BTreeSet::new(),
                        expect_key: true,
                    });
                    cursor.skip_ws();
                    if cursor.peek() == Some(b'}') {
                        cursor.bump();
                        stack.pop();
                        need_value = false;
                    } else {
                        // Read the first member name.
                        read_key(&mut cursor, &mut stack)?;
                        need_value = true;
                    }
                }
                Some(b'[') => {
                    cursor.bump();
                    if stack.len() >= MAX_DEPTH {
                        return Err(Malformed::TooDeep);
                    }
                    stack.push(Frame::Array);
                    cursor.skip_ws();
                    if cursor.peek() == Some(b']') {
                        cursor.bump();
                        stack.pop();
                        need_value = false;
                    } else {
                        need_value = true;
                    }
                }
                Some(b'"') => {
                    cursor.bump();
                    cursor.string()?;
                    need_value = false;
                }
                Some(b'-' | b'0'..=b'9') => {
                    cursor.number()?;
                    need_value = false;
                }
                Some(b't') => {
                    cursor.literal(b"true")?;
                    need_value = false;
                }
                Some(b'f') => {
                    cursor.literal(b"false")?;
                    need_value = false;
                }
                Some(b'n') => {
                    cursor.literal(b"null")?;
                    need_value = false;
                }
                _ => return Err(Malformed::Syntax),
            }
            continue;
        }

        // After a value: a separator or the end of the container.
        let Some(frame) = stack.last_mut() else {
            // The top-level value is complete.
            cursor.skip_ws();
            return if cursor.peek().is_none() {
                Ok(())
            } else {
                Err(Malformed::Trailing)
            };
        };
        match (frame, cursor.peek()) {
            (Frame::Object { .. }, Some(b',')) => {
                cursor.bump();
                cursor.skip_ws();
                read_key(&mut cursor, &mut stack)?;
                need_value = true;
            }
            (Frame::Object { .. }, Some(b'}')) => {
                cursor.bump();
                stack.pop();
            }
            (Frame::Array, Some(b',')) => {
                cursor.bump();
                need_value = true;
            }
            (Frame::Array, Some(b']')) => {
                cursor.bump();
                stack.pop();
            }
            _ => return Err(Malformed::Syntax),
        }
    }
}

/// Reads a member name and its colon in the object on top of the stack,
/// refusing a duplicate.
fn read_key(cursor: &mut Cursor<'_>, stack: &mut [Frame]) -> Result<(), Malformed> {
    cursor.expect_byte(b'"')?;
    let key = cursor.string()?;
    cursor.skip_ws();
    cursor.expect_byte(b':')?;
    match stack.last_mut() {
        Some(Frame::Object { keys, expect_key }) => {
            *expect_key = false;
            if !keys.insert(key) {
                return Err(Malformed::DuplicateMember);
            }
            Ok(())
        }
        _ => Err(Malformed::Syntax),
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::{validate_request, Malformed};

    fn check(text: &str) -> Result<(), Malformed> {
        validate_request(text.as_bytes())
    }

    #[test]
    fn accepts_a_lenient_but_well_formed_request() {
        assert_eq!(
            check(" { \"op\" : \"describe\" , \"x\": [1, -2, {\"a\": null, \"b\": true}] }\n"),
            Ok(())
        );
    }

    #[test]
    fn refuses_what_the_abi_names() {
        assert_eq!(check("\u{feff}{}"), Err(Malformed::Encoding));
        assert_eq!(check("[]"), Err(Malformed::NotAnObject));
        assert_eq!(check("{\"a\":1,\"a\":2}"), Err(Malformed::DuplicateMember));
        assert_eq!(check("{\"a\":1.5}"), Err(Malformed::Number));
        assert_eq!(check("{\"a\":1e2}"), Err(Malformed::Number));
        assert_eq!(check("{\"a\":007}"), Err(Malformed::Number));
        assert_eq!(check("{\"a\":-0}"), Err(Malformed::Number));
        assert_eq!(check("{\"a\":\"\\ud800\"}"), Err(Malformed::Str));
        assert_eq!(check("{\"a\":\"\\x\"}"), Err(Malformed::Str));
        assert_eq!(check("{\"a\":\"\t\"}"), Err(Malformed::Str));
        assert_eq!(check("{} x"), Err(Malformed::Trailing));
        assert_eq!(check("{\"a\":[[[[[[[[1]]]]]]]]}"), Err(Malformed::TooDeep));
        assert_eq!(check("{\"a\":[[[[[[[1]]]]]]]}"), Ok(()));
        assert_eq!(check("{\"a\":}"), Err(Malformed::Syntax));
        assert_eq!(check(""), Err(Malformed::Syntax));
        assert_eq!(check("{\"a\":1,}"), Err(Malformed::Syntax));
    }

    #[test]
    fn duplicates_are_compared_decoded_and_per_object() {
        assert_eq!(
            check("{\"id\":1,\"\\u0069d\":2}"),
            Err(Malformed::DuplicateMember)
        );
        assert_eq!(check("{\"a\":{\"k\":1},\"b\":{\"k\":2}}"), Ok(()));
    }

    #[test]
    fn surrogate_pairs_and_raw_unicode_are_fine() {
        assert_eq!(check("{\"a\":\"\\ud83d\\ude00 é 😀\"}"), Ok(()));
    }
}
