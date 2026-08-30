use std::collections::BTreeSet;

use super::command_specs;

#[test]
fn aggregate_specs_preserve_stable_order_and_unique_paths() {
    let paths = command_specs()
        .into_iter()
        .map(|command| command.path.join(" "))
        .collect::<Vec<_>>();

    assert_eq!(paths.len(), 64);
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
        &paths[25..29],
        ["repo-set", "map init", "map show", "map history"]
    );
    assert_eq!(
        &paths[39..43],
        [
            "graph inspect",
            "index refresh",
            "worker status",
            "worker run-once"
        ]
    );
    assert_eq!(
        &paths[59..],
        [
            "setup doctor",
            "setup profile",
            "version",
            "version check",
            "help"
        ]
    );
    assert_eq!(
        paths.iter().collect::<BTreeSet<_>>().len(),
        paths.len(),
        "machine-readable command paths must remain unique"
    );
}
