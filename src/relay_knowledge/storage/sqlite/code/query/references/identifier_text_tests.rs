use super::{identifier_ranges, normalized_identifier};

#[test]
fn identifier_normalization_keeps_only_folded_ascii_alphanumerics() {
    assert_eq!(
        normalized_identifier("Instance_Context.ts"),
        "instancecontextts"
    );
}

#[test]
fn identifier_ranges_require_boundaries_on_both_sides() {
    let line = "Thing ThingExtra extraThing _Thing Thing_ call(Thing)";
    let matches = identifier_ranges(line, "Thing")
        .map(|(start, end)| &line[start..end])
        .collect::<Vec<_>>();

    assert_eq!(matches, vec!["Thing", "Thing"]);
}
