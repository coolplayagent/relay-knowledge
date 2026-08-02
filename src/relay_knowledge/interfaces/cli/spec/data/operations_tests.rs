use super::command_specs;

#[test]
fn operation_specs_keep_worker_proposal_and_audit_order() {
    let paths = command_specs()
        .into_iter()
        .map(|command| command.path.join(" "))
        .collect::<Vec<_>>();

    assert_eq!(
        paths,
        [
            "worker status",
            "worker run-once",
            "proposal list",
            "proposal show",
            "proposal accept",
            "proposal reject",
            "proposal supersede",
            "audit query",
        ]
    );
}
