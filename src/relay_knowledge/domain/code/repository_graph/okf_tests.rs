use super::{
    MAX_CONCEPT_LINKS, MAX_CONCEPT_SOURCES, MAX_LABEL_CHARS, MAX_RESOURCE_BYTES, parse_concept,
};
use crate::domain::IndexedRepositoryDocument;

#[test]
fn parses_bom_crlf_yaml_and_all_resource_bounded_sources() {
    let document = document(
        "knowledge/research/rates.md",
        "\u{feff}---\r\ntype: Research Claim\r\ntitle: \"Policy: rates\"\r\ndescription: >\r\n  A folded YAML description.\r\nsources:\r\n  - resource: https://example.com/uncited\r\n    title: Uncited source\r\n  - id: cited\r\n    resource: https://example.com/cited\r\n---\r\n\r\nNo source footnotes are required.\r\n",
    );

    let concept = parse_concept(&document, "knowledge/research").expect("valid OKF concept");

    assert_eq!(concept.title, "Policy: rates");
    assert_eq!(concept.sources.len(), 2);
    assert_eq!(concept.sources[0].id, None);
    assert_eq!(concept.sources[0].resource, "https://example.com/uncited");
    assert_eq!(concept.sources[1].id.as_deref(), Some("cited"));
    assert_eq!(
        concept.details.get("description").map(String::as_str),
        Some("A folded YAML description.")
    );
}

#[test]
fn requires_valid_yaml_non_empty_string_type_and_exact_delimiters() {
    for content in [
        "---\ntitle: Missing type\n---\n",
        "---\ntype: \"\"\n---\n",
        "---\ntype: [Research Claim]\n---\n",
        "---\ntype: {kind: Research Claim}\n---\n",
        "---\ntype: 42\n---\n",
        "---\ntype: true\n---\n",
        "---\ntype: [unterminated\n---\n",
        "---\ntype: Research Claim\n--- trailing\n",
        "--- \ntype: Research Claim\n---\n",
    ] {
        assert!(
            parse_concept(&document("knowledge/rates.md", content), "knowledge").is_none(),
            "unexpectedly accepted {content:?}"
        );
    }
}

#[test]
fn keeps_sources_with_missing_or_oversized_ids_but_bounds_resources() {
    let oversized_id = "s".repeat(MAX_LABEL_CHARS + 1);
    let oversized_resource = "r".repeat(MAX_RESOURCE_BYTES + 1);
    let oversized_title = "知".repeat(MAX_LABEL_CHARS + 1);
    let document = document(
        "knowledge/research/rates.md",
        &format!(
            "---\ntype: Research Claim\ntitle: {oversized_title}\nsources:\n  - resource: https://example.com/no-id\n  - id: {oversized_id}\n    resource: https://example.com/oversized-id\n  - id: bounded\n    resource: {oversized_resource}\n---\n"
        ),
    );

    let concept = parse_concept(&document, "knowledge/research").expect("valid OKF concept");

    assert_eq!(concept.sources.len(), 2);
    assert!(concept.sources.iter().all(|source| source.id.is_none()));
    assert_eq!(concept.node().label, "知".repeat(MAX_LABEL_CHARS));
}

#[test]
fn bounds_source_entries_and_exposes_truncation() {
    let sources = (0..=MAX_CONCEPT_SOURCES)
        .map(|index| format!("  - resource: https://example.com/source-{index}\n"))
        .collect::<String>();
    let document = document(
        "knowledge/focus.md",
        &format!("---\ntype: Research Claim\nsources:\n{sources}---\n"),
    );

    let concept = parse_concept(&document, "knowledge").expect("valid OKF concept");

    assert_eq!(concept.sources.len(), MAX_CONCEPT_SOURCES);
    assert!(concept.truncated);
    assert_eq!(
        concept
            .details
            .get("source_extraction_truncated")
            .map(String::as_str),
        Some("true")
    );
}

#[test]
fn resolves_relative_and_bundle_relative_concept_resources() {
    let document = document(
        "knowledge/bundle/focus.md",
        r#"---
type: Research Claim
sources:
  - resource: ./relative.md#details
  - resource: /bundle.md
  - resource: https://example.com/evidence
  - resource: all tables in BigQuery project alpha
  - resource: projects/acme/queries
  - resource: references/source.pdf
  - resource: ../../outside.md
---

See [another concept](linked.md?view=full#part).
"#,
    );

    let concept = parse_concept(&document, "knowledge/bundle").expect("valid OKF concept");

    assert_eq!(
        concept.sources[0].candidate_path.as_deref(),
        Some("knowledge/bundle/relative.md")
    );
    assert_eq!(
        concept.sources[1].candidate_path.as_deref(),
        Some("knowledge/bundle/bundle.md")
    );
    assert_eq!(concept.sources[2].candidate_path, None);
    assert!(!concept.sources[3].bundle_path_hint);
    assert!(!concept.sources[4].bundle_path_hint);
    assert!(concept.sources[5].bundle_path_hint);
    assert!(concept.sources[6].bundle_path_hint);
    assert_eq!(concept.links, ["knowledge/bundle/linked.md"]);
}

#[test]
fn extracts_inline_and_used_reference_links_but_not_code_or_images() {
    let document = document(
        "knowledge/bundle/focus.md",
        r#"---
type: Research Claim
---

[inline](inline.md "title"), [angle](<angle.md>), [encoded](space%20name.md),
[escaped](escaped\(name\).md), [encoded punctuation](literal%23hash%3Fquery.md),
and [query](query.md?view=full#part).
[full reference][Full Ref], [collapsed][], and [shortcut].

`[inline code](ignored-inline-code.md)`

![inline image](ignored-inline-image.md)
![reference image][image-ref]

```markdown
[fenced code](ignored-fenced-code.md)
```

[full ref]: references/full%20claim.md
[collapsed]: references/collapsed.md
[shortcut]: references/shortcut.md
[image-ref]: references/ignored-image.md
[unused]: references/unused.md
"#,
    );

    let concept = parse_concept(&document, "knowledge/bundle").expect("valid OKF concept");

    assert_eq!(
        concept.links,
        [
            "knowledge/bundle/angle.md",
            "knowledge/bundle/escaped(name).md",
            "knowledge/bundle/inline.md",
            "knowledge/bundle/literal#hash?query.md",
            "knowledge/bundle/query.md",
            "knowledge/bundle/references/collapsed.md",
            "knowledge/bundle/references/full claim.md",
            "knowledge/bundle/references/shortcut.md",
            "knowledge/bundle/space name.md",
        ]
    );
    assert!(!concept.details.contains_key("link_extraction_truncated"));
}

#[test]
fn bounds_unique_concept_links_and_exposes_truncation() {
    let body = (0..=MAX_CONCEPT_LINKS)
        .map(|index| format!("[concept {index}](concept-{index}.md)\n"))
        .collect::<String>();
    let document = document(
        "knowledge/focus.md",
        &format!("---\ntype: Research Claim\n---\n\n{body}"),
    );

    let concept = parse_concept(&document, "knowledge").expect("valid OKF concept");

    assert_eq!(concept.links.len(), MAX_CONCEPT_LINKS);
    assert_eq!(
        concept
            .details
            .get("link_extraction_truncated")
            .map(String::as_str),
        Some("true")
    );
}

#[test]
fn external_markdown_links_do_not_consume_the_concept_link_budget() {
    let external_links = (0..=MAX_CONCEPT_LINKS)
        .map(|index| format!("[external {index}](https://example.com/{index})\n"))
        .collect::<String>();
    let document = document(
        "knowledge/focus.md",
        &format!("---\ntype: Research Claim\n---\n\n{external_links}[local](local.md)\n"),
    );

    let concept = parse_concept(&document, "knowledge").expect("valid OKF concept");

    assert_eq!(concept.links, ["knowledge/local.md"]);
    assert!(!concept.truncated);
}

fn document(path: &str, content: &str) -> IndexedRepositoryDocument {
    IndexedRepositoryDocument {
        path: path.to_owned(),
        language_id: "markdown".to_owned(),
        content: content.to_owned(),
    }
}
