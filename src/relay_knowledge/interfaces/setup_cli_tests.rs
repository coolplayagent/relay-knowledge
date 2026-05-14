use crate::{
    api::{ApiMetadata, InterfaceKind, RequestContext},
    domain::GraphVersion,
};

use super::*;

#[test]
fn parses_setup_doctor_and_profiles() {
    let doctor = parse_setup(&["doctor".to_owned()]).expect("doctor should parse");
    let profile = parse_setup(&["profile".to_owned(), "agent-readonly".to_owned()])
        .expect("profile should parse");

    assert_eq!(doctor, CliAction::SetupDoctor);
    assert_eq!(
        profile,
        CliAction::SetupProfile {
            profile: SetupProfile::AgentReadonly,
        }
    );
    assert!(matches!(
        parse_setup(&["profile".to_owned(), "unknown".to_owned()]),
        Err(CliError::UnexpectedArgument(_))
    ));
}

#[test]
fn setup_profiles_render_actionable_environment_and_commands() {
    let metadata = ApiMetadata::graph_only(
        &RequestContext::for_interface(InterfaceKind::Cli),
        GraphVersion::ZERO,
    );
    let profile = setup_profile(SetupProfile::ExternalEmbedding, metadata);

    assert_eq!(profile.profile, "external-embedding");
    assert!(profile.environment.iter().any(|variable| {
        variable.name == "RELAY_KNOWLEDGE_EMBEDDING_API_KEY" && variable.required
    }));
    assert!(
        profile
            .commands
            .contains(&"relay-knowledge provider probe --format json")
    );
    assert!(
        profile
            .notes
            .iter()
            .any(|note| note.contains("do not synchronously call"))
    );
}
