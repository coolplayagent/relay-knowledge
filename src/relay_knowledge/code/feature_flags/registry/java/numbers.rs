//! Canonical values for bounded Java numeric literals, without evaluating expressions.
pub(super) fn literal(kind: &str, source: &str) -> Option<String> {
    if source.len() > 256 {
        return None;
    }
    let normalized = source.replace('_', "");
    if kind == "decimal_floating_point_literal" {
        let raw = normalized.trim_end_matches(['f', 'F', 'd', 'D']);
        if normalized.ends_with(['f', 'F']) {
            let value = raw.parse::<f32>().ok()?;
            return value.is_finite().then(|| value.to_string());
        }
        let value = raw.parse::<f64>().ok()?;
        return value.is_finite().then(|| value.to_string());
    }
    let long = normalized.ends_with(['l', 'L']);
    let raw = normalized.trim_end_matches(['l', 'L']);
    let (digits, radix) = match kind {
        "hex_integer_literal" => (raw.get(2..)?, 16),
        "binary_integer_literal" => (raw.get(2..)?, 2),
        "octal_integer_literal" => (raw, 8),
        "decimal_integer_literal" => (raw, 10),
        _ => return None,
    };
    let value = u64::from_str_radix(digits, radix).ok()?;
    if long {
        if radix == 10 && value > i64::MAX as u64 {
            return None;
        }
        Some((value as i64).to_string())
    } else {
        let value = u32::try_from(value).ok()?;
        if radix == 10 && value > i32::MAX as u32 {
            return None;
        }
        Some((value as i32).to_string())
    }
}

#[cfg(test)]
#[path = "numbers_tests.rs"]
mod tests;
