use super::{diagnostic_commands, graph_commands, knowledge_commands, setup_and_meta_commands};

#[test]
fn core_groups_preserve_their_public_command_boundaries() {
    assert_eq!(paths(knowledge_commands()), ["status", "ingest", "query"]);
    assert_eq!(paths(graph_commands()), ["graph inspect", "index refresh"]);
    assert_eq!(paths(diagnostic_commands()), ["provider probe", "health"]);
    assert_eq!(
        paths(setup_and_meta_commands()),
        [
            "setup doctor",
            "setup profile",
            "version",
            "version check",
            "help"
        ]
    );
}

fn paths(commands: Vec<super::super::CliCommandSpec>) -> Vec<String> {
    commands
        .into_iter()
        .map(|command| command.path.join(" "))
        .collect()
}
