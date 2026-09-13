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

pub(super) fn signed_literal(kind: &str, source: &str, operator: &str) -> Option<String> {
    if !matches!(operator, "+" | "-") || source.len() > 256 {
        return None;
    }
    if operator == "-" && kind == "decimal_integer_literal" {
        let normalized = source.replace('_', "");
        if normalized == "2147483648" {
            return Some(i32::MIN.to_string());
        }
        if normalized.eq_ignore_ascii_case("9223372036854775808L") {
            return Some(i64::MIN.to_string());
        }
    }
    let value = literal(kind, source)?;
    if operator == "+" {
        return Some(value);
    }
    if kind == "decimal_floating_point_literal" {
        return Some((-value.parse::<f64>().ok()?).to_string());
    }
    if source.ends_with(['l', 'L']) {
        Some(value.parse::<i64>().ok()?.wrapping_neg().to_string())
    } else {
        Some(value.parse::<i32>().ok()?.wrapping_neg().to_string())
    }
}

/// Apply proven wrapper conversions to static property fallbacks, retaining raw evidence.
pub(super) fn convert_default(
    metadata: &mut crate::domain::CodeConfigMetadata,
    conversions: &[String],
    nullable: bool,
) {
    if metadata.default_value.is_none() && !nullable {
        return;
    }
    metadata.unconverted_default = metadata.default_value.clone();
    for conversion in conversions {
        if conversion == "Boolean" {
            metadata.boolean_converted_default = true;
            metadata.default_value = Some(
                metadata
                    .default_value
                    .as_ref()
                    .is_some_and(|v| v.eq_ignore_ascii_case("true"))
                    .to_string(),
            );
            metadata.value_type = Some("boolean".into());
            continue;
        }
        metadata.numeric_converted_default = true;
        metadata.value_type = Some("number".into());
        if metadata.default_value.is_none() {
            break;
        }
        metadata.default_value = metadata.default_value.as_deref().and_then(|raw| {
            if raw.len() > 256 {
                return None;
            }
            match conversion.as_str() {
                "Integer" => raw.parse::<i32>().ok().map(|v| v.to_string()),
                "Long" => raw.parse::<i64>().ok().map(|v| v.to_string()),
                "Double" => {
                    let raw = raw.trim_matches(|c| c <= '\u{20}');
                    let raw = if raw.ends_with(['d', 'D', 'f', 'F']) {
                        &raw[..raw.len() - 1]
                    } else {
                        raw
                    };
                    raw.parse::<f64>()
                        .ok()
                        .filter(|v| v.is_finite())
                        .map(|v| v.to_string())
                }
                _ => None,
            }
        });
        if metadata.default_value.is_none() {
            metadata.flow_incomplete = Some("invalid_or_unsupported_numeric_default".into());
            break;
        }
    }
}

#[cfg(test)]
#[path = "numbers_tests.rs"]
mod tests;
