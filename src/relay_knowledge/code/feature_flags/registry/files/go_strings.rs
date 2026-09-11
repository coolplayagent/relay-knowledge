//! Decode bounded UTF-8 Go string arguments; byte escapes retain their byte semantics.
pub(super) fn decode(raw: &str) -> Option<String> {
    if raw.len() > 65_536 {
        return None;
    }
    let mut bytes = Vec::new();
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            if matches!(ch, '\n' | '\r' | '"') {
                return None;
            }
            let mut buffer = [0; 4];
            bytes.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
            continue;
        }
        let escape = chars.next()?;
        let value = match escape {
            'a' => 7,
            'b' => 8,
            'f' => 12,
            'n' => 10,
            'r' => 13,
            't' => 9,
            'v' => 11,
            '\\' => 92,
            '"' => 34,
            'x' | 'u' | 'U' => {
                let mut value = 0u32;
                for _ in 0..match escape {
                    'x' => 2,
                    'u' => 4,
                    _ => 8,
                } {
                    value = value
                        .checked_mul(16)?
                        .checked_add(chars.next()?.to_digit(16)?)?;
                }
                if escape != 'x' {
                    let mut buffer = [0; 4];
                    bytes.extend_from_slice(
                        char::from_u32(value)?.encode_utf8(&mut buffer).as_bytes(),
                    );
                    continue;
                }
                value
            }
            '0'..='7' => {
                let mut value = escape.to_digit(8)?;
                for _ in 0..2 {
                    value = value * 8 + chars.next()?.to_digit(8)?;
                }
                value
            }
            _ => return None,
        };
        bytes.push(u8::try_from(value).ok()?);
    }
    String::from_utf8(bytes).ok()
}
#[cfg(test)]
#[path = "go_strings_tests.rs"]
mod tests;
