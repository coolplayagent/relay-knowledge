use std::collections::BTreeSet;

pub(crate) fn normalized_terms(text: &str, min_len: usize) -> BTreeSet<String> {
    let mut terms = BTreeSet::new();
    extend_normalized_terms(text, min_len, &mut terms);

    terms
}

pub(crate) fn extend_normalized_terms(text: &str, min_len: usize, terms: &mut BTreeSet<String>) {
    let mut token = String::new();
    for character in text.chars() {
        if character.is_alphanumeric() || character == '_' {
            token.push(character);
        } else {
            insert_identifier_terms(&token, min_len, terms);
            token.clear();
        }
    }
    insert_identifier_terms(&token, min_len, terms);
}

fn insert_identifier_terms(token: &str, min_len: usize, terms: &mut BTreeSet<String>) {
    if token.is_empty() {
        return;
    }
    insert_term(&token.to_lowercase(), min_len, terms);

    let mut parts = Vec::new();
    for chunk in token.split('_').filter(|part| !part.is_empty()) {
        split_identifier_chunk(chunk, &mut parts);
    }
    for part in &parts {
        insert_term(part, min_len, terms);
    }
    if let Some(acronym) = acronym(&parts) {
        insert_term(&acronym, min_len, terms);
    }
}

fn split_identifier_chunk(chunk: &str, parts: &mut Vec<String>) {
    let mut current = String::new();
    let mut previous = CharacterClass::Boundary;
    let characters = chunk.chars().collect::<Vec<_>>();
    for (index, character) in characters.iter().enumerate() {
        let class = CharacterClass::from(*character);
        let next = characters
            .get(index + 1)
            .map(|character| CharacterClass::from(*character))
            .unwrap_or(CharacterClass::Boundary);
        if class == CharacterClass::Boundary {
            push_part(parts, &mut current);
            previous = CharacterClass::Boundary;
            continue;
        }
        if should_split(previous, class, next) {
            push_part(parts, &mut current);
        }
        current.extend(character.to_lowercase());
        previous = class;
    }
    push_part(parts, &mut current);
}

fn push_part(parts: &mut Vec<String>, current: &mut String) {
    if !current.is_empty() {
        parts.push(std::mem::take(current));
    }
}

fn acronym(parts: &[String]) -> Option<String> {
    if parts.len() < 2 {
        return None;
    }
    let acronym = parts
        .iter()
        .filter_map(|part| part.chars().next())
        .collect::<String>();
    (!acronym.is_empty()).then_some(acronym)
}

fn insert_term(term: &str, min_len: usize, terms: &mut BTreeSet<String>) {
    if term.chars().count() >= min_len {
        terms.insert(term.to_owned());
    }
}

fn should_split(previous: CharacterClass, current: CharacterClass, next: CharacterClass) -> bool {
    matches!(
        (previous, current),
        (CharacterClass::Lower, CharacterClass::Upper)
            | (CharacterClass::Digit, CharacterClass::Upper)
            | (CharacterClass::Digit, CharacterClass::Lower)
            | (CharacterClass::Lower, CharacterClass::Digit)
            | (CharacterClass::Upper, CharacterClass::Digit)
    ) || matches!(
        (previous, current, next),
        (
            CharacterClass::Upper,
            CharacterClass::Upper,
            CharacterClass::Lower
        )
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharacterClass {
    Upper,
    Lower,
    Digit,
    Boundary,
}

impl From<char> for CharacterClass {
    fn from(value: char) -> Self {
        if value.is_ascii_uppercase() {
            Self::Upper
        } else if value.is_ascii_lowercase() {
            Self::Lower
        } else if value.is_ascii_digit() {
            Self::Digit
        } else if value.is_alphanumeric() {
            Self::Lower
        } else {
            Self::Boundary
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_terms_split_mixed_identifiers_and_preserve_whole_tokens() {
        let terms = normalized_terms("GraphRAGContextPack retry_policy RESTClient W3Connector", 2);

        for term in [
            "graphragcontextpack",
            "graph",
            "rag",
            "context",
            "pack",
            "grcp",
            "retry_policy",
            "retry",
            "policy",
            "rest",
            "client",
            "w3connector",
            "connector",
            "w3c",
        ] {
            assert!(terms.contains(term), "missing term {term}");
        }
        assert!(!terms.contains("w"));
        assert!(!terms.contains("3"));
    }

    #[test]
    fn normalized_terms_can_keep_single_character_terms_for_rerank() {
        let terms = normalized_terms("C API W3", 1);

        assert!(terms.contains("c"));
        assert!(terms.contains("api"));
        assert!(terms.contains("w"));
        assert!(terms.contains("3"));
    }

    #[test]
    fn extend_normalized_terms_matches_owned_collection() {
        let mut extended = BTreeSet::from(["existing".to_owned()]);
        extend_normalized_terms("GraphRAGContextPack retry_policy", 2, &mut extended);

        let mut expected = normalized_terms("GraphRAGContextPack retry_policy", 2);
        expected.insert("existing".to_owned());

        assert_eq!(extended, expected);
    }
}
