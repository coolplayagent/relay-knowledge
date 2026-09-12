//! Bounded Java Unicode translation and string escape decoding.
pub(super) fn decode(raw: &str) -> Option<String> {
    if raw.len() > 65_536 {
        return None;
    }
    let mut translated = Vec::new();
    let mut chars = raw.chars().peekable();
    let mut slashes = 0;
    while let Some(ch) = chars.next() {
        if ch == '\\' && slashes % 2 == 0 && chars.peek() == Some(&'u') {
            while chars.peek() == Some(&'u') {
                chars.next();
            }
            let mut unit = 0u16;
            for _ in 0..4 {
                unit = unit
                    .checked_mul(16)?
                    .checked_add(chars.next()?.to_digit(16)? as u16)?;
            }
            translated.push(unit);
            slashes = 0;
        } else {
            translated.extend(ch.encode_utf16(&mut [0; 2]).iter().copied());
            slashes = if ch == '\\' { slashes + 1 } else { 0 };
        }
    }
    let translated = String::from_utf16(&translated).ok()?;
    let mut chars = translated.chars().peekable();
    let mut result = String::new();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            if matches!(ch, '\n' | '\r' | '"') {
                return None;
            }
            result.push(ch);
            continue;
        }
        let escaped = chars.next()?;
        result.push(match escaped {
            'b' => '\u{0008}',
            't' => '\t',
            'n' => '\n',
            'f' => '\u{000c}',
            'r' => '\r',
            's' => ' ',
            '\\' => '\\',
            '\'' => '\'',
            '"' => '"',
            '0'..='7' => {
                let mut value = escaped.to_digit(8)?;
                for _ in 0..if escaped <= '3' { 2 } else { 1 } {
                    let Some(next) = chars.peek().copied().and_then(|c| c.to_digit(8)) else {
                        break;
                    };
                    chars.next();
                    value = value * 8 + next;
                }
                char::from_u32(value)?
            }
            _ => return None,
        });
    }
    Some(result)
}

#[cfg(test)]
#[path = "strings_tests.rs"]
mod tests;
