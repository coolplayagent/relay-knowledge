//! Bounded Java string values: Unicode translation precedes string escape decoding.
const MAX_LITERAL_SOURCE_BYTES: usize = 64 * 1024;

pub(super) fn decode(source: &str) -> Option<String> {
    if source.len() > MAX_LITERAL_SOURCE_BYTES || source.starts_with("\"\"\"") {
        return None;
    }
    let body = source.strip_prefix('"')?.strip_suffix('"')?;
    let units = translate_unicode(body)?;
    let mut value = Vec::with_capacity(units.len());
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        index += 1;
        if unit != u16::from(b'\\') {
            if matches!(unit, 10 | 13 | 34) {
                return None;
            }
            value.push(unit);
            continue;
        }
        let escaped = *units.get(index)?;
        index += 1;
        let decoded = match escaped {
            98 => 8,   // b
            116 => 9,  // t
            110 => 10, // n
            102 => 12, // f
            114 => 13, // r
            115 => 32, // s
            34 | 39 | 92 => escaped,
            48..=55 => {
                let mut octal = escaped - 48;
                let extra = if escaped <= 51 { 2 } else { 1 };
                for _ in 0..extra {
                    let Some(digit @ 48..=55) = units.get(index).copied() else {
                        break;
                    };
                    octal = octal * 8 + digit - 48;
                    index += 1;
                }
                octal
            }
            _ => return None,
        };
        value.push(decoded);
    }
    // Java permits isolated UTF-16 surrogates, but they cannot be represented
    // faithfully in metadata's UTF-8 String. Preserve those defaults as unknown.
    String::from_utf16(&value).ok()
}

fn translate_unicode(source: &str) -> Option<Vec<u16>> {
    let mut chars = source.chars().peekable();
    let mut units = Vec::with_capacity(source.len());
    let mut backslashes = 0usize;
    let mut previous_unicode = false;
    while let Some(ch) = chars.next() {
        let eligible = previous_unicode || backslashes % 2 == 0;
        if ch == '\\' && eligible && chars.peek() == Some(&'u') {
            while chars.peek() == Some(&'u') {
                chars.next();
            }
            let mut unit = 0u16;
            for _ in 0..4 {
                unit = unit * 16 + u16::try_from(chars.next()?.to_digit(16)?).ok()?;
            }
            units.push(unit);
            previous_unicode = true;
            backslashes = if unit == u16::from(b'\\') {
                backslashes + 1
            } else {
                0
            };
        } else {
            let mut encoded = [0u16; 2];
            units.extend_from_slice(ch.encode_utf16(&mut encoded));
            previous_unicode = false;
            backslashes = if ch == '\\' { backslashes + 1 } else { 0 };
        }
    }
    Some(units)
}

#[cfg(test)]
#[path = "string_defaults_tests.rs"]
mod tests;
