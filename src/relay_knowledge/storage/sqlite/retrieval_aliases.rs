use std::collections::BTreeSet;

pub(super) fn lexical_aliases(values: &[&str]) -> String {
    let mut aliases = BTreeSet::new();
    for value in values {
        for alias in aliases_for_value(value) {
            aliases.insert(alias);
        }
    }

    aliases.into_iter().collect::<Vec<_>>().join(" ")
}

fn aliases_for_value(value: &str) -> Vec<String> {
    let split = split_identifier(value);
    let mut aliases = Vec::new();
    if !split.is_empty() && split != value.to_lowercase() {
        aliases.push(split.clone());
    }
    if let Some(acronym) = acronym(&split) {
        aliases.push(acronym);
    }

    aliases
}

fn split_identifier(value: &str) -> String {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut previous = CharacterClass::Boundary;

    let characters = value.chars().collect::<Vec<_>>();
    for (index, character) in characters.iter().enumerate() {
        let class = CharacterClass::from(*character);
        let next = characters
            .get(index + 1)
            .map(|character| CharacterClass::from(*character))
            .unwrap_or(CharacterClass::Boundary);
        if class == CharacterClass::Boundary {
            push_word(&mut words, &mut current);
            previous = CharacterClass::Boundary;
            continue;
        }
        if should_split(previous, class, next) {
            push_word(&mut words, &mut current);
        }
        current.push((*character).to_ascii_lowercase());
        previous = class;
    }
    push_word(&mut words, &mut current);

    words.join(" ")
}

fn push_word(words: &mut Vec<String>, current: &mut String) {
    if !current.is_empty() {
        words.push(std::mem::take(current));
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

fn acronym(split: &str) -> Option<String> {
    let acronym = split
        .split_whitespace()
        .filter_map(|word| word.chars().next())
        .collect::<String>();
    (acronym.len() > 1).then_some(acronym)
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
        } else {
            Self::Boundary
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_searchable_identifier_aliases() {
        let aliases = lexical_aliases(&["GraphRAGContextPack", "relay-knowledge"]);

        assert!(aliases.contains("graph rag context pack"));
        assert!(aliases.contains("grcp"));
        assert!(aliases.contains("relay knowledge"));
        assert!(aliases.contains("rk"));
    }
}
