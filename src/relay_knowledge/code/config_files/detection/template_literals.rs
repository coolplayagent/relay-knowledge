//! Decode Go template string literals without replacing non-UTF-8 byte values.

pub(in crate::code) fn string(arguments: &str) -> Option<(String, &str)> {
    let (value, rest) = bytes(arguments)?;
    Some((String::from_utf8(value).ok()?, rest))
}

pub(super) fn bytes(arguments: &str) -> Option<(Vec<u8>, &str)> {
    let arguments = arguments.trim_start();
    if let Some(raw) = arguments.strip_prefix('`') {
        let (value, rest) = raw.split_once('`')?;
        return Some((value.replace('\r', "").into_bytes(), rest));
    }
    let bytes = arguments.as_bytes();
    if bytes.first() != Some(&b'"') {
        return None;
    }
    let mut decoded = Vec::new();
    let mut position = 1;
    while let Some(&byte) = bytes.get(position) {
        position += 1;
        match byte {
            b'"' => return Some((decoded, &arguments[position..])),
            b'\n' => return None,
            b'\\' => decode_escape(bytes, &mut position, &mut decoded)?,
            byte => decoded.push(byte),
        }
    }
    None
}

fn decode_escape(bytes: &[u8], position: &mut usize, output: &mut Vec<u8>) -> Option<()> {
    let escape = *bytes.get(*position)?;
    *position += 1;
    match escape {
        b'a' => output.push(7),
        b'b' => output.push(8),
        b'f' => output.push(12),
        b'n' => output.push(b'\n'),
        b'r' => output.push(b'\r'),
        b't' => output.push(b'\t'),
        b'v' => output.push(11),
        b'\\' | b'"' => output.push(escape),
        b'x' => output.push(digits(bytes, position, 2, 16)? as u8),
        b'0'..=b'7' => {
            *position -= 1;
            output.push(u8::try_from(digits(bytes, position, 3, 8)?).ok()?);
        }
        b'u' | b'U' => {
            let scalar = char::from_u32(digits(
                bytes,
                position,
                if escape == b'u' { 4 } else { 8 },
                16,
            )?)?;
            let mut encoded = [0; 4];
            output.extend_from_slice(scalar.encode_utf8(&mut encoded).as_bytes());
        }
        _ => return None,
    }
    Some(())
}

fn digits(bytes: &[u8], position: &mut usize, count: usize, radix: u32) -> Option<u32> {
    let end = position.checked_add(count)?;
    let mut value = 0u32;
    for byte in bytes.get(*position..end)? {
        value = value
            .checked_mul(radix)?
            .checked_add(char::from(*byte).to_digit(radix)?)?;
    }
    *position = end;
    Some(value)
}

#[cfg(test)]
#[path = "template_literals_tests.rs"]
mod tests;
