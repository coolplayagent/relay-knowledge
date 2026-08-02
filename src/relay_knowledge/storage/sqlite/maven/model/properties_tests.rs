use std::collections::BTreeMap;

use super::{merge_profile_properties, resolved_profile_properties};
use crate::storage::sqlite::maven::model::{RawProfile, TaggedValue};

fn profile(properties: BTreeMap<String, TaggedValue>) -> RawProfile {
    RawProfile {
        id: TaggedValue {
            value: "release".to_owned(),
            line: 1,
        },
        active_by_default: false,
        properties,
        dependencies: Vec::new(),
        dependency_management: Vec::new(),
        plugins: Vec::new(),
        plugin_management: Vec::new(),
    }
}

#[test]
fn profile_properties_interpolate_against_the_complete_overlay() {
    let profile = profile(BTreeMap::from([
        (
            "major".to_owned(),
            TaggedValue {
                value: "2".to_owned(),
                line: 2,
            },
        ),
        (
            "version".to_owned(),
            TaggedValue {
                value: "${major}.1".to_owned(),
                line: 3,
            },
        ),
    ]));
    let base = BTreeMap::from([("major".to_owned(), "1".to_owned())]);

    let resolved = resolved_profile_properties(&base, &profile);

    assert_eq!(resolved.get("major").map(String::as_str), Some("2"));
    assert_eq!(resolved.get("version").map(String::as_str), Some("2.1"));
}

#[test]
fn profile_merge_preserves_unrelated_base_properties() {
    let profile = profile(BTreeMap::new());
    let mut properties = BTreeMap::from([("encoding".to_owned(), "UTF-8".to_owned())]);

    merge_profile_properties(&mut properties, &profile);

    assert_eq!(
        properties.get("encoding").map(String::as_str),
        Some("UTF-8")
    );
}
