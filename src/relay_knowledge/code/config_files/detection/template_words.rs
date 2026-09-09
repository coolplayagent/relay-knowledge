//! Conservative Go identifier, field-chain, and numeric token validation.
pub(super) fn valid(word: &str) -> bool {
    if word == "." {
        return true;
    }
    if word.starts_with('.') && word.as_bytes().get(1).is_some_and(u8::is_ascii_digit) {
        return number(word);
    }
    if let Some(fields) = word.strip_prefix('.') {
        return fields.split('.').all(identifier);
    }
    if word
        .as_bytes()
        .first()
        .is_some_and(|ch| ch.is_ascii_digit() || matches!(ch, b'+' | b'-'))
    {
        return number(word);
    }
    identifier(word)
}
fn identifier(word: &str) -> bool {
    let mut chars = word.chars();
    chars
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_alphanumeric())
}
fn number(word: &str) -> bool {
    let imaginary = word.ends_with('i');
    let sign = match word.as_bytes().first() {
        Some(b'-') => Some(true),
        Some(b'+') => Some(false),
        _ => None,
    };
    let word = word.strip_suffix('i').unwrap_or(word);
    let word = word.strip_prefix(['+', '-']).unwrap_or(word);
    let lower = word.to_ascii_lowercase();
    if let Some(hex) = lower.strip_prefix("0x") {
        let (mantissa, exponent) = hex
            .split_once('p')
            .map_or((hex, None), |(a, b)| (a, Some(b)));
        return (!imaginary || exponent.is_some())
            && (!hex.starts_with('_') || hex.as_bytes().get(1).is_some_and(u8::is_ascii_hexdigit))
            && (!mantissa.contains('.') || exponent.is_some())
            && fraction(mantissa.strip_prefix('_').unwrap_or(mantissa), 16)
            && exponent.is_none_or(decimal_exponent)
            && if !imaginary && exponent.is_none() && !mantissa.contains('.') {
                integer(mantissa.strip_prefix('_').unwrap_or(mantissa), 16, sign)
            } else {
                finite_hex(mantissa, exponent)
            };
    }
    for (prefix, radix) in [("0b", 2), ("0o", 8)] {
        if let Some(digits) = lower.strip_prefix(prefix) {
            let digits = digits.strip_prefix('_').unwrap_or(digits);
            return !imaginary && digit_run(digits, radix) && integer(digits, radix, sign);
        }
    }
    let (mantissa, exponent) = lower
        .split_once('e')
        .map_or((lower.as_str(), None), |(a, b)| (a, Some(b)));
    if exponent.is_none() && !mantissa.contains('.') && !imaginary {
        let radix = if mantissa.starts_with('0') { 8 } else { 10 };
        return digit_run(mantissa, radix) && integer(mantissa, radix, sign);
    }
    fraction(mantissa, 10)
        && exponent.is_none_or(decimal_exponent)
        && lower
            .replace('_', "")
            .parse::<f64>()
            .is_ok_and(f64::is_finite)
}
fn integer(digits: &str, radix: u32, sign: Option<bool>) -> bool {
    let mut value = 0_u64;
    for ch in digits.chars().filter(|ch| *ch != '_') {
        let Some(digit) = ch.to_digit(radix) else {
            return false;
        };
        let Some(next) = value
            .checked_mul(u64::from(radix))
            .and_then(|v| v.checked_add(u64::from(digit)))
        else {
            return false;
        };
        value = next;
    }
    match sign {
        None => true,
        Some(false) => value <= i64::MAX as u64,
        Some(true) => value <= (i64::MAX as u64) + 1,
    }
}

fn finite_hex(mantissa: &str, exponent: Option<&str>) -> bool {
    let mut value = 0.0_f64;
    let mut fraction_digits = 0_i32;
    let mut fractional = false;
    for ch in mantissa.chars() {
        match ch {
            '_' => {}
            '.' => fractional = true,
            _ => {
                let Some(digit) = ch.to_digit(16) else {
                    return false;
                };
                value = value * 16.0 + f64::from(digit);
                if fractional {
                    fraction_digits += 1;
                }
            }
        }
    }
    let exponent = match exponent {
        Some(exponent) => match exponent.replace('_', "").parse::<i32>() {
            Ok(value) => value,
            Err(_) => return false,
        },
        None => 0,
    };
    let Some(exponent) = exponent.checked_sub(fraction_digits * 4) else {
        return false;
    };
    (value * 2.0_f64.powi(exponent)).is_finite()
}

fn decimal_exponent(exponent: &str) -> bool {
    digit_run(exponent.strip_prefix(['+', '-']).unwrap_or(exponent), 10)
}
fn fraction(value: &str, radix: u32) -> bool {
    if let Some((left, right)) = value.split_once('.') {
        (!left.is_empty() || !right.is_empty())
            && (left.is_empty() || digit_run(left, radix))
            && (right.is_empty() || digit_run(right, radix))
    } else {
        digit_run(value, radix)
    }
}
fn digit_run(value: &str, radix: u32) -> bool {
    let mut digit = false;
    for ch in value.chars() {
        if ch == '_' {
            if !digit {
                return false;
            }
            digit = false;
        } else if ch.is_ascii() && ch.is_digit(radix) {
            digit = true;
        } else {
            return false;
        }
    }
    digit
}
#[cfg(test)]
#[path = "template_words_tests.rs"]
mod tests;
