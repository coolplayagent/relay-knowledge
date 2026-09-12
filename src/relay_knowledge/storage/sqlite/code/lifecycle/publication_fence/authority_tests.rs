use super::*;
use crate::env::{EnvironmentConfig, PlatformKind};

#[tokio::test]
async fn authority_policy_is_checked_before_a_new_attachment_but_not_handle_reuse() {
    let root = std::env::temp_dir().join(format!(
        "relay-attach-policy-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let env = EnvironmentConfig::from_pairs(
        PlatformKind::current(),
        [("RELAY_KNOWLEDGE_HOME", root.to_str().unwrap())],
    )
    .unwrap();
    let mut paths = RuntimePaths::resolve(&env.platform, &env.paths).unwrap();
    std::fs::create_dir_all(&paths.data_dir).unwrap();
    paths.windows_data_sid = Some("S-1-5-21-1-2-3-1001".to_owned());
    tokio::task::spawn_blocking(move || {
        let connection = Connection::open_in_memory().unwrap();
        let mut authority = PublicationAuthority {
            path: paths.database_file(),
            paths,
        };
        let error = attach_authority(&connection, &authority).unwrap_err();
        assert!(error.to_string().contains("account policy"));
        assert!(
            !authority.path.exists(),
            "ATTACH must not create an unchecked control database"
        );
        authority.paths.windows_data_sid = None;
        attach_authority(&connection, &authority).unwrap();
        authority.paths.windows_data_sid = Some("S-1-5-21-1-2-3-1001".to_owned());
        attach_authority(&connection, &authority).unwrap();
        authority.path.set_extension("other.sqlite");
        assert!(
            attach_authority(&connection, &authority)
                .unwrap_err()
                .to_string()
                .contains("already attached")
        );
        assert!(!authority.path.exists());
    })
    .await
    .unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
