use tree_sitter::{Node, Parser};

use super::super::nodes::{SyntaxRange, node_text, push_children_reverse, syntax_range};

pub(in crate::code::parser) fn imports(
    content: &str,
    node: Node<'_>,
) -> Vec<(String, SyntaxRange)> {
    match node.kind() {
        "inline" => inline_imports(content, node),
        "link_reference_definition" => reference_import(content, node).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn inline_imports(content: &str, inline: Node<'_>) -> Vec<(String, SyntaxRange)> {
    let Some(source) = content.get(inline.start_byte()..inline.end_byte()) else {
        return Vec::new();
    };
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_md::INLINE_LANGUAGE.into())
        .is_err()
    {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };
    let mut imports = Vec::new();
    let mut stack = Vec::new();
    push_children_reverse(tree.root_node(), &mut stack);
    while let Some(current) = stack.pop() {
        if matches!(current.kind(), "inline_link" | "image")
            && let Some(destination) = direct_named_child_of_kind(current, "link_destination")
            && let Some(module) = local_markdown_target(&node_text(source, destination))
        {
            imports.push((module, offset_range(inline, destination)));
        }
        push_children_reverse(current, &mut stack);
    }

    imports
}

fn reference_import(content: &str, node: Node<'_>) -> Option<(String, SyntaxRange)> {
    let destination = direct_named_child_of_kind(node, "link_destination")?;
    let module = local_markdown_target(&node_text(content, destination))?;

    Some((module, syntax_range(destination)))
}

fn direct_named_child_of_kind<'tree>(root: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    (0..root.named_child_count()).find_map(|index| {
        let child = root.named_child(u32::try_from(index).ok()?)?;
        (child.kind() == kind).then_some(child)
    })
}

fn offset_range(parent: Node<'_>, child: Node<'_>) -> SyntaxRange {
    SyntaxRange {
        byte_start: parent.start_byte() + child.start_byte(),
        byte_end: parent.start_byte() + child.end_byte(),
        line_start: parent.start_position().row + child.start_position().row + 1,
        line_end: parent.start_position().row + child.end_position().row + 1,
    }
}

fn local_markdown_target(value: &str) -> Option<String> {
    let target = value
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim();
    if target.is_empty() || target.starts_with('#') || target.starts_with("//") {
        return None;
    }
    if has_uri_scheme(target) {
        return None;
    }
    let path = markdown_path_without_query_or_fragment(target).trim();
    if path.is_empty() {
        return None;
    }
    let path = decode_markdown_escapes(path);

    Some(percent_decode_path(&path).unwrap_or(path))
}

fn has_uri_scheme(value: &str) -> bool {
    let Some((scheme, _)) = value.split_once(':') else {
        return false;
    };
    let mut bytes = scheme.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

fn markdown_path_without_query_or_fragment(target: &str) -> &str {
    let mut escaped = false;
    for (index, character) in target.char_indices() {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if matches!(character, '?' | '#') {
            return &target[..index];
        }
    }

    target
}

fn decode_markdown_escapes(path: &str) -> String {
    if !path.as_bytes().contains(&b'\\') {
        return path.to_owned();
    }

    let mut decoded = String::with_capacity(path.len());
    let mut characters = path.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\\'
            && let Some(next) = characters.peek()
            && next.is_ascii_punctuation()
        {
            decoded.push(*next);
            characters.next();
        } else {
            decoded.push(character);
        }
    }

    decoded
}

fn percent_decode_path(path: &str) -> Option<String> {
    if !path.as_bytes().contains(&b'%') {
        return Some(path.to_owned());
    }

    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(hex) = bytes.get(index + 1..index + 3)
            && let Some(byte) = decode_hex_pair(hex)
        {
            decoded.push(byte);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }

    String::from_utf8(decoded).ok()
}

fn decode_hex_pair(hex: &[u8]) -> Option<u8> {
    let [high, low] = hex else {
        return None;
    };
    Some(hex_value(*high)? << 4 | hex_value(*low)?)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
