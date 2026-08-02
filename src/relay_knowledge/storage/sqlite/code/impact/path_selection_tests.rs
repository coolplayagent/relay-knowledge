//! Direct contracts for impact language inference and bounded path batching.

use std::collections::BTreeSet;

use super::path_selection::{SQLITE_BIND_BATCH_SIZE, batched_path_values, language_id_for_path};

#[test]
fn infers_languages_from_compound_suffixes_and_special_names() {
    assert_eq!(language_id_for_path("web/app.TSX").as_deref(), Some("tsx"));
    assert_eq!(
        language_id_for_path("scripts/.bash_profile").as_deref(),
        Some("bash")
    );
    assert_eq!(language_id_for_path("Gemfile").as_deref(), Some("ruby"));
    assert_eq!(language_id_for_path("assets/image.bin"), None);
}

#[test]
fn path_batches_never_exceed_the_sqlite_bind_budget() {
    let paths = (0..=SQLITE_BIND_BATCH_SIZE)
        .map(|index| format!("src/{index}.rs"))
        .collect::<BTreeSet<_>>();

    let batches = batched_path_values(&paths);

    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].len(), SQLITE_BIND_BATCH_SIZE);
    assert_eq!(batches[1].len(), 1);
}
