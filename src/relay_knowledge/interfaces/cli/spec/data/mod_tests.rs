use std::collections::BTreeSet;

use super::command_specs;

#[test]
fn aggregate_specs_preserve_stable_order_and_unique_paths() {
    let paths = command_specs()
        .into_iter()
        .map(|command| command.path.join(" "))
        .collect::<Vec<_>>();

    assert_eq!(paths.len(), 83);
    assert!(paths.iter().any(|path| path == "repo diagnostics"));
    assert_eq!(
        &paths[..7],
        [
            "status",
            "ingest",
            "query",
            "files index",
            "files query",
            "files content",
            "repo list"
        ]
    );
    assert_eq!(
        &paths[26..32],
        [
            "repo-set",
            "map init",
            "map plan",
            "map apply",
            "map show",
            "map history"
        ]
    );
    assert_eq!(
        &paths[49..53],
        [
            "graph inspect",
            "index refresh",
            "worker status",
            "worker run-once"
        ]
    );
    assert_eq!(
        &paths[78..],
        [
            "setup doctor",
            "setup profile",
            "version",
            "version check",
            "help"
        ]
    );
    assert_eq!(
        &paths[42..49],
        [
            "sources audit",
            "evidence validate",
            "evidence import",
            "evidence export",
            "evidence view",
            "evidence impact",
            "research status"
        ]
    );
    assert_eq!(
        &paths[59..68],
        [
            "feedback configure",
            "feedback report",
            "feedback status",
            "feedback preview",
            "feedback submit",
            "feedback retry",
            "feedback track",
            "feedback link-fix",
            "feedback validate"
        ]
    );
    assert_eq!(
        paths.iter().collect::<BTreeSet<_>>().len(),
        paths.len(),
        "machine-readable command paths must remain unique"
    );
}
