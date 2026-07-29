use super::super::code_query_identifiers::identifier_terms_equivalent;

pub(in crate::storage::sqlite::code::code_query) fn declaration_chunk_bonus(
    terms: &[String],
    content: &str,
) -> f64 {
    let abstract_interface = terms.iter().any(|term| term == "interface")
        && content.contains("virtual ")
        && (content.contains("= 0;") || content.contains("=0;"));
    let relationship_declaration = terms.iter().any(|term| type_relationship_intent(term))
        && content_has_type_relationship_declaration(content);
    let mixin_definition = terms.iter().any(|term| mixin_definition_intent(term))
        && content_has_mixin_definition_surface(content);
    let declaration_lines = if abstract_interface {
        0
    } else {
        content
            .lines()
            .map(str::trim)
            .filter(|line| declaration_line_is_prototype(line))
            .take(2)
            .count()
    };
    if !abstract_interface
        && declaration_lines < 2
        && !relationship_declaration
        && !mixin_definition
    {
        return 0.0;
    }

    let lower_content = content.to_lowercase();
    let matched_terms = terms
        .iter()
        .filter(|term| {
            term.len() >= 3
                && (identifier_field_matches_token(content, term)
                    || lower_content.contains(term.as_str())
                    || ((relationship_declaration || mixin_definition)
                        && declaration_intent_term_matches(content, term)))
        })
        .count();
    if matched_terms < 3 {
        return 0.0;
    }

    if abstract_interface {
        3.0
    } else if mixin_definition {
        4.75
    } else if relationship_declaration && relationship_override_member_surface(terms, content) {
        5.75
    } else if relationship_declaration {
        2.75
    } else if declaration_lines >= 2 {
        2.0
    } else {
        0.0
    }
}

fn mixin_definition_intent(term: &str) -> bool {
    matches!(
        term,
        "interface"
            | "interfaces"
            | "mixin"
            | "mixins"
            | "module"
            | "modules"
            | "protocol"
            | "protocols"
            | "trait"
            | "traits"
    )
}

fn type_relationship_intent(term: &str) -> bool {
    matches!(
        term,
        "derive"
            | "derived"
            | "extend"
            | "extends"
            | "implement"
            | "implements"
            | "inherit"
            | "inheritance"
            | "inherited"
            | "inherits"
            | "interface"
            | "interfaces"
            | "mixin"
            | "mixins"
            | "module"
            | "modules"
            | "override"
            | "overrides"
            | "overriding"
            | "protocol"
            | "protocols"
            | "subclass"
            | "subclasses"
            | "trait"
            | "traits"
    )
}

fn content_has_type_relationship_declaration(content: &str) -> bool {
    content.lines().map(str::trim).any(|line| {
        if line.starts_with("//") || line.starts_with('*') {
            return false;
        }
        let declaration_kind = declaration_kind_for_line(line);
        matches!(
            declaration_kind.as_deref(),
            Some("interface" | "module" | "protocol" | "trait")
        ) || (declaration_kind.is_some() && line_has_type_relationship_operator(line))
    })
}

fn relationship_override_member_surface(terms: &[String], content: &str) -> bool {
    let override_intent = terms.iter().any(|term| {
        matches!(
            term.as_str(),
            "override" | "overrides" | "overriding" | "implement" | "implements"
        )
    });
    override_intent
        && content.lines().map(str::trim).any(|line| {
            !line.starts_with("//")
                && !line.starts_with('*')
                && line.contains('(')
                && (line.starts_with("override ")
                    || line.contains(" override")
                    || line.contains(" implements "))
        })
}

fn declaration_kind_for_line(line: &str) -> Option<String> {
    line.split(|character: char| character.is_whitespace() || matches!(character, '{' | '(' | '<'))
        .filter(|word| !word.is_empty())
        .take(6)
        .find(|word| {
            matches!(
                *word,
                "class" | "enum" | "interface" | "module" | "protocol" | "struct" | "trait"
            )
        })
        .map(str::to_owned)
}

fn line_has_type_relationship_operator(line: &str) -> bool {
    line.contains(" : ")
        || line.contains(": public ")
        || line.contains(": protected ")
        || line.contains(": private ")
        || line.contains(" extends ")
        || line.contains(" implements ")
        || line.contains(" with ")
        || line.contains(" < ")
        || line_inherits_with_parenthesized_base(line)
}

fn line_inherits_with_parenthesized_base(line: &str) -> bool {
    let Some(class_index) = line.find("class ") else {
        return false;
    };
    let after_class = &line[class_index + "class ".len()..];
    let Some(open_index) = after_class.find('(') else {
        return false;
    };
    let before_open = after_class[..open_index].trim();
    let after_open = after_class[open_index + 1..].trim_start();
    !before_open.is_empty() && !after_open.starts_with(')')
}

fn declaration_intent_term_matches(content: &str, term: &str) -> bool {
    match term {
        "subclass" | "subclasses" | "inherit" | "inherits" | "inherited" | "inheritance" => {
            content_has_type_relationship_declaration(content)
        }
        "exception" | "exceptions" => content_has_exception_declaration(content),
        "mixin" | "mixins" => content_has_mixin_declaration(content),
        "module" | "modules" => content_has_declaration_kind(content, "module"),
        "protocol" | "protocols" => content_has_declaration_kind(content, "protocol"),
        "trait" | "traits" => content_has_declaration_kind(content, "trait"),
        "interface" | "interfaces" => content_has_declaration_kind(content, "interface"),
        _ => false,
    }
}

fn content_has_declaration_kind(content: &str, kind: &str) -> bool {
    content
        .lines()
        .map(str::trim)
        .any(|line| declaration_kind_for_line(line).as_deref() == Some(kind))
}

fn content_has_mixin_declaration(content: &str) -> bool {
    content_has_declaration_kind(content, "module")
        || content_has_declaration_kind(content, "trait")
}

fn content_has_mixin_definition_surface(content: &str) -> bool {
    (content_has_declaration_kind(content, "interface")
        || content_has_declaration_kind(content, "module")
        || content_has_declaration_kind(content, "protocol")
        || content_has_declaration_kind(content, "trait"))
        && !content_has_declaration_kind(content, "class")
        && !content_has_declaration_kind(content, "enum")
        && !content_has_declaration_kind(content, "struct")
}

fn content_has_exception_declaration(content: &str) -> bool {
    content.lines().map(str::trim).any(|line| {
        declaration_kind_for_line(line).as_deref() == Some("class")
            && (line.contains("Exception")
                || line.contains("Error")
                || line.contains("RuntimeError")
                || line.contains("Throwable"))
    })
}

fn declaration_line_is_prototype(line: &str) -> bool {
    line.ends_with(';')
        && line.contains('(')
        && !line.contains("->")
        && !line.contains('.')
        && !line.starts_with("return ")
}

fn identifier_field_matches_token(field: &str, token: &str) -> bool {
    identifier_tokens(field).any(|candidate| {
        identifier_terms_equivalent(candidate, token)
            || candidate
                .split('_')
                .filter(|part| !part.is_empty())
                .any(|part| identifier_terms_equivalent(part, token))
            || camel_case_terms(candidate)
                .iter()
                .any(|part| identifier_terms_equivalent(part, token))
    })
}

fn identifier_tokens(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|token| !token.is_empty())
}

fn camel_case_terms(token: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut start = 0;
    let mut previous: Option<char> = None;
    let chars = token.char_indices().collect::<Vec<_>>();
    for (index, (byte_index, character)) in chars.iter().enumerate() {
        let next = chars.get(index + 1).map(|(_, next)| *next);
        let starts_upper_word = character.is_ascii_uppercase()
            && previous.is_some_and(|previous| {
                previous.is_ascii_lowercase()
                    || previous.is_ascii_digit()
                    || next.is_some_and(|next| next.is_ascii_lowercase())
            });
        if *byte_index > start && starts_upper_word {
            terms.push(token[start..*byte_index].to_ascii_lowercase());
            start = *byte_index;
        }
        previous = Some(*character);
    }
    if start < token.len() {
        terms.push(token[start..].to_ascii_lowercase());
    }

    terms
}
