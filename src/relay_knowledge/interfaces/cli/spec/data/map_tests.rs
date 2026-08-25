use super::{command_specs, map_source_options};

#[test]
fn map_specs_keep_source_requirements_specific_to_add() {
    let paths = command_specs()
        .into_iter()
        .map(|command| command.path.join(" "))
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        [
            "map init",
            "map show",
            "map history",
            "map route",
            "map source add",
            "map source update",
            "map source remove",
            "map validate",
            "map agent-snippet",
        ]
    );

    let add_options = map_source_options(true);
    let update_options = map_source_options(false);
    for flag in ["--topic", "--kind", "--uri"] {
        let add = add_options
            .iter()
            .find(|option| option.flag == flag)
            .expect("add option should exist");
        let update = update_options
            .iter()
            .find(|option| option.flag == flag)
            .expect("update option should exist");
        assert!(add.required);
        assert!(!update.required);
    }
}
