//! Expands bounded Maven property references in effective model values.

use std::collections::BTreeMap;

const MAX_INTERPOLATION_DEPTH: usize = 16;

pub(super) fn interpolate(value: &str, properties: &BTreeMap<String, String>) -> String {
    interpolate_with_depth(value, properties, 0)
}

fn interpolate_with_depth(
    value: &str,
    properties: &BTreeMap<String, String>,
    depth: usize,
) -> String {
    if depth >= MAX_INTERPOLATION_DEPTH {
        return value.to_owned();
    }
    let mut output = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        output.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            output.push_str(&rest[start..]);
            return output;
        };
        let key = &after[..end];
        if let Some(replacement) = properties.get(key) {
            output.push_str(&interpolate_with_depth(replacement, properties, depth + 1));
        } else {
            output.push_str("${");
            output.push_str(key);
            output.push('}');
        }
        rest = &after[end + 1..];
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
