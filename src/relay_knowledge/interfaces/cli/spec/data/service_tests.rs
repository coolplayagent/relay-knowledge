use std::collections::BTreeSet;

use super::{command_specs, service_lifecycle_options};

#[test]
fn service_specs_are_unique_and_execute_flags_are_lifecycle_only() {
    let commands = command_specs();
    let paths = commands
        .iter()
        .map(|command| command.path.join(" "))
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        [
            "service status",
            "service doctor",
            "service plan",
            "service lifecycle",
            "service definition write",
            "service operator",
            "service worker run",
            "service run",
        ]
    );
    assert_eq!(
        paths.iter().collect::<BTreeSet<_>>().len(),
        paths.len(),
        "service command paths must remain unique"
    );

    let plan_flags = service_lifecycle_options(false)
        .into_iter()
        .map(|option| option.flag)
        .collect::<Vec<_>>();
    let lifecycle_flags = service_lifecycle_options(true)
        .into_iter()
        .map(|option| option.flag)
        .collect::<Vec<_>>();
    assert_eq!(plan_flags, ["--target-version", "--install-dir"]);
    assert_eq!(
        lifecycle_flags,
        [
            "--dry-run",
            "--execute",
            "--target-version",
            "--install-dir"
        ]
    );
}
