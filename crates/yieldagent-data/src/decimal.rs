//! Exact decimals from JSON number text. A source's APY and TVL are read from the raw
//! token, never through `f64`: `4.1` stays `4.1`, and a value the 96-bit mantissa cannot
//! hold exactly is refused rather than rounded.

use rust_decimal::Decimal;

/// Longest number token accepted; a longer one is refused before any work.
pub const MAX_NUMBER_LEN: usize = 48; // LIMIT(number-length)
/// Largest exponent magnitude accepted (`1e28` is past any APY or TVL anyway).
const MAX_EXPONENT: i32 = 28;

/// Why a number token was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DecimalError {
    /// Not a JSON number: a string, `null`, `NaN`, a leading `+`, hex and so on.
    #[error("not a JSON number")]
    NotANumber,
    /// Longer than [`MAX_NUMBER_LEN`].
    #[error("number token too long")]
    TooLong,
    /// More digits or a larger exponent than an exact 96-bit decimal holds.
    #[error("not representable exactly")]
    Inexact,
}

/// Parses one JSON number token exactly. Grammar: RFC 8259 §6, so `-0.5`, `12`, `1.5e3`,
/// `4E-2`; anything else is [`DecimalError::NotANumber`].
pub fn parse_exact(token: &str) -> Result<Decimal, DecimalError> {
    if token.len() > MAX_NUMBER_LEN {
        return Err(DecimalError::TooLong);
    }
    let bytes = token.as_bytes();
    let mut i = 0;
    let negative = bytes.first() == Some(&b'-');
    if negative {
        i += 1;
    }
    let int_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let int = &token[int_start..i];
    if int.is_empty() || (int.len() > 1 && int.starts_with('0')) {
        return Err(DecimalError::NotANumber);
    }
    let mut frac = "";
    if bytes.get(i) == Some(&b'.') {
        let start = i + 1;
        i = start;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        frac = &token[start..i];
        if frac.is_empty() {
            return Err(DecimalError::NotANumber);
        }
    }
    let mut exponent: i32 = 0;
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        let exp_negative = match bytes.get(i) {
            Some(b'-') => {
                i += 1;
                true
            }
            Some(b'+') => {
                i += 1;
                false
            }
            _ => false,
        };
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        let digits = &token[start..i];
        if digits.is_empty() {
            return Err(DecimalError::NotANumber);
        }
        let magnitude: i32 = digits.parse().map_err(|_| DecimalError::Inexact)?;
        if magnitude > MAX_EXPONENT {
            return Err(DecimalError::Inexact);
        }
        exponent = if exp_negative { -magnitude } else { magnitude };
    }
    if i != bytes.len() {
        return Err(DecimalError::NotANumber);
    }
    // Move the point: digits = int ++ frac, scale = len(frac) - exponent.
    let mut digits = String::with_capacity(int.len() + frac.len() + MAX_EXPONENT as usize);
    digits.push_str(int);
    digits.push_str(frac);
    let mut scale = i32::try_from(frac.len()).map_err(|_| DecimalError::Inexact)? - exponent;
    while scale < 0 {
        digits.push('0');
        scale += 1;
    }
    let scale = u32::try_from(scale).map_err(|_| DecimalError::Inexact)?;
    let trimmed = digits.trim_start_matches('0');
    if trimmed.len() > 28 || scale > 28 {
        return Err(DecimalError::Inexact);
    }
    let mantissa: i128 = if trimmed.is_empty() {
        0
    } else {
        trimmed.parse().map_err(|_| DecimalError::Inexact)?
    };
    let value =
        Decimal::try_from_i128_with_scale(if negative { -mantissa } else { mantissa }, scale)
            .map_err(|_| DecimalError::Inexact)?;
    Ok(value.normalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn json_numbers_parse_exactly() {
        assert_eq!(parse_exact("4.1"), Ok(d("4.1")));
        assert_eq!(parse_exact("0"), Ok(d("0")));
        assert_eq!(parse_exact("-0.5"), Ok(d("-0.5")));
        assert_eq!(parse_exact("1.5e3"), Ok(d("1500")));
        assert_eq!(parse_exact("4E-2"), Ok(d("0.04")));
        assert_eq!(
            parse_exact("123456789.123456789"),
            Ok(d("123456789.123456789"))
        );
        // 0.1 + 0.2 is exact here; through f64 it is 0.30000000000000004.
        assert_eq!(
            parse_exact("0.1").unwrap() + parse_exact("0.2").unwrap(),
            d("0.3")
        );
    }

    #[test]
    fn what_is_not_a_json_number_is_refused() {
        for bad in [
            "", "-", "+1", "01", "1.", ".5", "1e", "1e+", "NaN", "Infinity", "0x10", "1 ", "\"1\"",
            "null", "1.2.3",
        ] {
            assert_eq!(parse_exact(bad), Err(DecimalError::NotANumber), "{bad:?}");
        }
    }

    #[test]
    fn what_cannot_be_held_exactly_is_refused_not_rounded() {
        assert_eq!(parse_exact(&"1".repeat(49)), Err(DecimalError::TooLong));
        assert_eq!(parse_exact("1e29"), Err(DecimalError::Inexact));
        assert_eq!(
            parse_exact("0.00000000000000000000000000001"),
            Err(DecimalError::Inexact)
        );
        assert_eq!(
            parse_exact("12345678901234567890123456789"),
            Err(DecimalError::Inexact)
        );
        assert_eq!(
            parse_exact(&"1".repeat(MAX_NUMBER_LEN + 1)),
            Err(DecimalError::TooLong)
        );
    }
}
