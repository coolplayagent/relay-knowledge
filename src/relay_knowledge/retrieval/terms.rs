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

    #[test]
    fn extract_identifiers_pascal_case() {
        let ids = extract_identifiers("UserService");
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0].original, "UserService");
        assert_eq!(ids[0].kind, IdentifierKind::PascalCase);
        assert!(ids[0].parts.contains(&"user".to_owned()));
        assert!(ids[0].parts.contains(&"service".to_owned()));
        assert!(ids[0].weight > 1.0);
    }

    #[test]
    fn extract_identifiers_camel_case() {
        let ids = extract_identifiers("signInWithGoogle");
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0].original, "signInWithGoogle");
        assert_eq!(ids[0].kind, IdentifierKind::CamelCase);
        assert!(ids[0].parts.contains(&"sign".to_owned()));
        assert!(ids[0].parts.contains(&"google".to_owned()));
        assert!(!ids[0].parts.contains(&"in".to_owned()));
        assert!(!ids[0].parts.contains(&"with".to_owned()));
    }

    #[test]
    fn extract_identifiers_snake_case() {
        let ids = extract_identifiers("max_retries");
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0].original, "max_retries");
        assert_eq!(ids[0].kind, IdentifierKind::SnakeCase);
        assert_eq!(ids[0].parts, vec!["max", "retries"]);
    }

    #[test]
    fn extract_identifiers_screaming_snake_case() {
        let ids = extract_identifiers("MAX_RETRIES");
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0].original, "MAX_RETRIES");
        assert_eq!(ids[0].kind, IdentifierKind::ScreamingSnakeCase);
        assert_eq!(ids[0].parts, vec!["max", "retries"]);
    }

    #[test]
    fn extract_identifiers_dot_notation() {
        let ids = extract_identifiers("app.isPackaged");
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0].original, "app.isPackaged");
        assert_eq!(ids[0].kind, IdentifierKind::DotNotation);
        assert!(ids[0].parts.contains(&"app".to_owned()));
        assert!(ids[0].parts.contains(&"ispackaged".to_owned()));
    }

    #[test]
    fn extract_identifiers_all_caps() {
        let ids = extract_identifiers("HTTP");
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0].original, "HTTP");
        assert_eq!(ids[0].kind, IdentifierKind::AllCaps);
    }

    #[test]
    fn extract_identifiers_lowercase() {
        let ids = extract_identifiers("render");
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0].original, "render");
        assert_eq!(ids[0].kind, IdentifierKind::Lowercase);
    }

    #[test]
    fn extract_identifiers_natural_language_query() {
        let ids =
            extract_identifiers("how does UserService handle signInWithGoogle authentication");
        let originals: Vec<&str> = ids.iter().map(|id| id.original.as_str()).collect();
        assert!(originals.contains(&"UserService"));
        assert!(originals.contains(&"signInWithGoogle"));
        assert!(originals.contains(&"authentication"));
        assert!(!originals.contains(&"how"));
        assert!(!originals.contains(&"does"));
    }

    #[test]
    fn stop_word_filtering() {
        assert!(is_stop_word("the"));
        assert!(is_stop_word("and"));
        assert!(is_stop_word("for"));
        assert!(is_stop_word("with"));
        assert!(is_stop_word("how"));
        assert!(is_stop_word("what"));
        assert!(!is_stop_word("handle"));
        assert!(!is_stop_word("service"));
    }

    #[test]
    fn stop_word_count_covers_at_least_80() {
        assert!(STOP_WORDS.len() >= 80);
    }

    #[test]
    fn stem_variants_connecting() {
        let variants = stem_variants("connecting");
        assert!(variants.contains(&"connect".to_owned()));
        assert!(!variants.contains(&"connecte".to_owned()));
    }

    #[test]
    fn stem_variants_connected() {
        let variants = stem_variants("connected");
        assert!(variants.contains(&"connect".to_owned()));
    }

    #[test]
    fn stem_variants_renderer() {
        let variants = stem_variants("renderer");
        assert!(variants.contains(&"render".to_owned()));
        assert!(variants.contains(&"rendere".to_owned()));
    }

    #[test]
    fn stem_variants_running() {
        let variants = stem_variants("running");
        assert!(variants.contains(&"run".to_owned()));
        assert!(variants.contains(&"runn".to_owned()));
    }

    #[test]
    fn stem_variants_parsed() {
        let variants = stem_variants("parsed");
        assert!(variants.contains(&"parse".to_owned()));
        assert!(variants.contains(&"pars".to_owned()));
    }

    #[test]
    fn identifier_parts_handles_embedded_acronyms() {
        let ids = extract_identifiers("parseXMLFile");
        assert_eq!(ids.len(), 1);
        assert!(ids[0].parts.contains(&"parse".to_owned()));
        assert!(ids[0].parts.contains(&"xml".to_owned()));
        assert!(ids[0].parts.contains(&"file".to_owned()));
    }

    #[test]
    fn stem_variants_collections() {
        let variants = stem_variants("collections");
        assert!(variants.contains(&"collection".to_owned()));
    }

    #[test]
    fn stem_variants_short_words_ignored() {
        assert!(stem_variants("go").is_empty());
        assert!(stem_variants("do").is_empty());
    }

    #[test]
    fn identifier_weight_pascal_higher_than_lowercase() {
        let pascal = extract_identifiers("UserService");
        let lower = extract_identifiers("service");
        assert!(!pascal.is_empty());
        assert!(!lower.is_empty());
        assert!(pascal[0].weight > lower[0].weight);
    }

    #[test]
    fn extract_query_identifiers_includes_stem_variants() {
        let ids = extract_query_identifiers("connecting renderer");
        let connecting = ids.iter().find(|id| id.original == "connecting").unwrap();
        assert!(connecting.parts.contains(&"connect".to_owned()));
        assert!(!connecting.parts.contains(&"connecte".to_owned()));
        let renderer = ids.iter().find(|id| id.original == "renderer").unwrap();
        assert!(renderer.parts.contains(&"render".to_owned()));
    }

    #[test]
    fn classify_token_patterns() {
        assert_eq!(
            classify_token("UserService"),
            Some(IdentifierKind::PascalCase)
        );
        assert_eq!(classify_token("signIn"), Some(IdentifierKind::CamelCase));
        assert_eq!(
            classify_token("max_retries"),
            Some(IdentifierKind::SnakeCase)
        );
        assert_eq!(
            classify_token("MAX_RETRIES"),
            Some(IdentifierKind::ScreamingSnakeCase)
        );
        assert_eq!(classify_token("REST"), Some(IdentifierKind::AllCaps));
        assert_eq!(
            classify_token("app.init"),
            Some(IdentifierKind::DotNotation)
        );
        assert_eq!(classify_token("render"), Some(IdentifierKind::Lowercase));
        assert_eq!(classify_token(""), None);
    }

    #[test]
    fn extract_identifiers_mixed_query() {
        let ids = extract_identifiers(
            "UserService retry_policy REST MAX_RETRIES API_KEY app.isPackaged render parse",
        );
        let originals: Vec<&str> = ids.iter().map(|id| id.original.as_str()).collect();
        assert!(originals.contains(&"UserService"));
        assert!(originals.contains(&"retry_policy"));
        assert!(originals.contains(&"REST"));
        assert!(originals.contains(&"MAX_RETRIES"));
        assert!(originals.contains(&"API_KEY"));
        assert!(originals.contains(&"app.isPackaged"));
        assert!(originals.contains(&"render"));
        assert!(originals.contains(&"parse"));
        assert_eq!(ids.len(), 8);
    }
}
