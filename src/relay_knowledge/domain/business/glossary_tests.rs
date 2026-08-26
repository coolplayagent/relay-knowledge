use super::*;

fn glossary_with_homonyms() -> BusinessGlossary {
    BusinessGlossary {
        schema_version: 1,
        domains: vec![
            BusinessDomainDefinition {
                id: "sales".to_owned(),
                name: "Sales".to_owned(),
                description: None,
            },
            BusinessDomainDefinition {
                id: "sports".to_owned(),
                name: "Sports".to_owned(),
                description: None,
            },
        ],
        terms: vec![
            term("sales", "arr", "Annual Recurring Revenue"),
            term("sports", "arr", "Average Run Rate"),
        ],
    }
}

fn term(domain: &str, id: &str, name: &str) -> BusinessTermDefinition {
    BusinessTermDefinition {
        id: id.to_owned(),
        domain: domain.to_owned(),
        canonical_name: name.to_owned(),
        definition: format!("Definition of {name}."),
        language: "en".to_owned(),
        status: BusinessTermStatus::Active,
        aliases: vec![BusinessAlias {
            value: "ARR".to_owned(),
            kind: BusinessAliasKind::Abbreviation,
            language: Some("en".to_owned()),
        }],
        semantics: None,
        mappings: Vec::new(),
    }
}

#[test]
fn schema_allows_same_term_id_in_different_domains() {
    glossary_with_homonyms().validate().unwrap();
}

#[test]
fn schema_rejects_duplicate_term_id_within_domain_and_unknown_domain() {
    let mut glossary = glossary_with_homonyms();
    glossary.terms.push(term("sales", "arr", "Other"));
    assert!(glossary.validate().is_err());

    let mut glossary = glossary_with_homonyms();
    glossary.terms[0].domain = "missing".to_owned();
    assert!(glossary.validate().is_err());
}

#[test]
fn parser_enforces_file_and_per_term_bounds() {
    assert!(BusinessGlossary::parse(&vec![b'x'; BUSINESS_GLOSSARY_MAX_BYTES + 1]).is_err());
    let mut glossary = glossary_with_homonyms();
    glossary.terms[0].aliases = (0..=BUSINESS_TERM_MAX_ALIASES)
        .map(|index| BusinessAlias {
            value: format!("alias-{index}"),
            kind: BusinessAliasKind::Synonym,
            language: None,
        })
        .collect();
    assert!(glossary.validate().is_err());
}

#[test]
fn empty_v1_round_trips_as_valid_yaml() {
    let yaml = serde_norway::to_string(&BusinessGlossary::empty_v1()).unwrap();
    assert_eq!(
        BusinessGlossary::parse(yaml.as_bytes()).unwrap(),
        BusinessGlossary::empty_v1()
    );
}
