use std::collections::BTreeMap;

use serde_json::Value;

use super::fields::string_field;

pub fn objects_by_repository(cases: &[Value]) -> BTreeMap<String, Vec<Value>> {
    let mut grouped: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for case in cases {
        if let Some(repository) = string_field(case, "repository") {
            grouped
                .entry(repository.to_owned())
                .or_default()
                .push(case.clone());
        }
    }
    grouped
}

#[cfg(test)]
#[path = "grouping_tests.rs"]
mod grouping_tests;
