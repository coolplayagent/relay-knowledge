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

#[allow(dead_code)]
const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "not", "in", "on", "at", "to", "for", "of", "with", "by",
    "from", "as", "is", "was", "are", "were", "be", "been", "being", "have", "has", "had", "do",
    "does", "did", "will", "would", "could", "should", "may", "might", "shall", "can", "this",
    "that", "these", "those", "it", "its", "he", "she", "they", "them", "we", "us", "what",
    "which", "who", "whom", "how", "when", "where", "why", "if", "then", "else", "so", "no", "yes",
    "all", "each", "every", "both", "few", "more", "most", "other", "some", "such", "than", "too",
    "very", "just", "about", "above", "after", "again", "also", "any", "because", "before",
    "between", "during", "here", "into", "over", "there", "through", "under", "until", "up",
    "down", "out", "off", "only", "own", "same",
];

#[allow(dead_code)]
fn is_stop_word(word: &str) -> bool {
    STOP_WORDS.contains(&word)
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentifierKind {
    PascalCase,
    CamelCase,
    SnakeCase,
    ScreamingSnakeCase,
    DotNotation,
    AllCaps,
    Lowercase,
}

#[allow(dead_code)]
pub(crate) fn classify_token(token: &str) -> Option<IdentifierKind> {
    if token.is_empty() {
        return None;
    }
    if token.contains('.') && token.chars().next().is_some_and(|c| c.is_alphabetic()) {
        return Some(IdentifierKind::DotNotation);
    }
    if token.contains('_') {
        let all_upper = token
            .chars()
            .filter(|c| c.is_alphabetic())
            .all(|c| c.is_uppercase());
        return if all_upper {
            Some(IdentifierKind::ScreamingSnakeCase)
        } else {
            Some(IdentifierKind::SnakeCase)
        };
    }
    let first = token.chars().next()?;
    if !first.is_alphabetic() {
        return None;
    }
    let rest_has_upper = token.chars().skip(1).any(|c| c.is_uppercase());
    let all_upper = token
        .chars()
        .filter(|c| c.is_alphabetic())
        .all(|c| c.is_uppercase());
    if all_upper {
        return Some(IdentifierKind::AllCaps);
    }
    if first.is_uppercase() && rest_has_upper {
        return Some(IdentifierKind::PascalCase);
    }
    if first.is_uppercase() {
        return Some(IdentifierKind::PascalCase);
    }
    if rest_has_upper {
        return Some(IdentifierKind::CamelCase);
    }
    if token.len() >= 3 {
        return Some(IdentifierKind::Lowercase);
    }
    None
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ExtractedIdentifier {
    pub original: String,
    pub kind: IdentifierKind,
    pub parts: Vec<String>,
    pub weight: f64,
}

#[allow(dead_code)]
pub(crate) fn extract_identifiers(text: &str) -> Vec<ExtractedIdentifier> {
    let mut results = Vec::new();
    let mut token = String::new();
    for character in text.chars() {
        if character.is_alphanumeric() || character == '_' || character == '.' {
            token.push(character);
        } else if !token.is_empty() {
            try_extract_identifier(&token, &mut results);
            token.clear();
        }
    }
    if !token.is_empty() {
        try_extract_identifier(&token, &mut results);
    }
    results
}

#[allow(dead_code)]
fn try_extract_identifier(token: &str, results: &mut Vec<ExtractedIdentifier>) {
    let Some(kind) = classify_token(token) else {
        return;
    };
    let lower = token.to_ascii_lowercase();
    if matches!(kind, IdentifierKind::Lowercase | IdentifierKind::AllCaps) && lower.len() < 3 {
        return;
    }
    if matches!(kind, IdentifierKind::Lowercase) && is_stop_word(&lower) {
        return;
    }
    let mut parts = identifier_parts(token, &kind);
    parts.retain(|p| !is_stop_word(p));
    let weight = identifier_weight(&kind);
    results.push(ExtractedIdentifier {
        original: token.to_owned(),
        kind,
        parts,
        weight,
    });
}

#[allow(dead_code)]
fn identifier_parts(token: &str, kind: &IdentifierKind) -> Vec<String> {
    match kind {
        IdentifierKind::DotNotation => token.split('.').map(|s| s.to_ascii_lowercase()).collect(),
        IdentifierKind::ScreamingSnakeCase | IdentifierKind::SnakeCase => token
            .split('_')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_ascii_lowercase())
            .collect(),
        IdentifierKind::PascalCase | IdentifierKind::CamelCase => {
            let mut parts = Vec::new();
            split_identifier_chunk(token, &mut parts);
            parts
        }
        IdentifierKind::AllCaps | IdentifierKind::Lowercase => {
            vec![token.to_ascii_lowercase()]
        }
    }
}

#[allow(dead_code)]
fn identifier_weight(kind: &IdentifierKind) -> f64 {
    match kind {
        IdentifierKind::PascalCase | IdentifierKind::CamelCase => 1.5,
        IdentifierKind::SnakeCase => 1.3,
        IdentifierKind::ScreamingSnakeCase => 1.3,
        IdentifierKind::DotNotation => 1.2,
        IdentifierKind::AllCaps => 1.1,
        IdentifierKind::Lowercase => 0.8,
    }
}

#[allow(dead_code)]
pub(crate) fn stem_variants(word: &str) -> Vec<String> {
    let lower = word.to_ascii_lowercase();
    if lower.len() < 4 {
        return Vec::new();
    }
    let mut variants = Vec::new();
    if lower.ends_with("ing") && lower.len() > 5 {
        let base = &lower[..lower.len() - 3];
        if !base.is_empty() {
            variants.push(base.to_owned());
            let mut deduped_added = false;
            {
                let base_chars: Vec<char> = base.chars().collect();
                if base_chars.len() >= 2
                    && base_chars[base_chars.len() - 1] == base_chars[base_chars.len() - 2]
                {
                    let deduped: String = base_chars[..base_chars.len() - 1].iter().collect();
                    if !deduped.is_empty() {
                        variants.push(deduped);
                        deduped_added = true;
                    }
                }
            }
            if deduped_added {
                let mut e_base = base.to_owned();
                e_base.push('e');
                variants.push(e_base);
            }
        }
    } else if lower.ends_with("ed") && lower.len() > 4 {
        let base = &lower[..lower.len() - 2];
        if !base.is_empty() {
            let mut e_form = base.to_owned();
            e_form.push('e');
            if e_form.len() >= 4 {
                variants.push(e_form);
            }
            variants.push(base.to_owned());
        }
        {
            let chars: Vec<char> = lower.chars().collect();
            if chars.len() > 3 && chars[chars.len() - 3] == *chars.last().unwrap_or(&'_') {
                let deduped: String = chars[..chars.len() - 3].iter().collect();
                if !deduped.is_empty() {
                    variants.push(deduped);
                }
            }
        }
    } else if lower.ends_with("er") && lower.len() > 4 {
        let base = &lower[..lower.len() - 2];
        if !base.is_empty() {
            variants.push(base.to_owned());
        }
        let mut e_base = base.to_owned();
        e_base.push('e');
        variants.push(e_base);
    } else if lower.ends_with("tion") && lower.len() > 5 {
        let base = &lower[..lower.len() - 4];
        let mut te = base.to_owned();
        te.push_str("te");
        variants.push(te);
    } else if lower.ends_with("sion") && lower.len() > 5 {
        let base = &lower[..lower.len() - 4];
        let mut d = base.to_owned();
        d.push('d');
        variants.push(d);
    } else if lower.ends_with("ies") && lower.len() > 4 {
        let base = &lower[..lower.len() - 3];
        if !base.is_empty() {
            let mut y = base.to_owned();
            y.push('y');
            variants.push(y);
        }
    } else if lower.ends_with('s')
        && !lower.ends_with("ss")
        && !lower.ends_with("us")
        && !lower.ends_with("is")
        && lower.len() > 4
    {
        let base = &lower[..lower.len() - 1];
        variants.push(base.to_owned());
    }
    variants
}

#[allow(dead_code)]
pub(crate) fn extract_query_identifiers(text: &str) -> Vec<ExtractedIdentifier> {
    let mut identifiers = extract_identifiers(text);
    for ident in &mut identifiers {
        let mut stem_parts = Vec::new();
        for part in &ident.parts {
            stem_parts.extend(stem_variants(part));
        }
        ident.parts.extend(stem_parts);
    }
    identifiers
}

#[cfg(test)]
#[path = "terms_tests.rs"]
mod tests;
