//! Direct bounded Maven property interpolation invariants.

use super::*;

#[test]
fn interpolation_resolves_nested_values_and_preserves_unknown_properties() {
    let properties = BTreeMap::from([
        ("revision".to_owned(), "1.2.3".to_owned()),
        ("artifact".to_owned(), "core-${revision}".to_owned()),
    ]);

    assert_eq!(
        interpolate("${artifact}:${missing}", &properties),
        "core-1.2.3:${missing}"
    );
}

#[test]
fn cyclic_interpolation_stops_at_the_depth_budget() {
    let properties = BTreeMap::from([
        ("left".to_owned(), "${right}".to_owned()),
        ("right".to_owned(), "${left}".to_owned()),
    ]);
    let result = interpolate("${left}", &properties);

    assert!(result == "${left}" || result == "${right}");
}
