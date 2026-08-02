//! Direct unit contract for lifecycle step construction policy.

use std::path::Path;

use super::*;

#[test]
fn internal_and_command_steps_keep_effects_and_privilege_separate() {
    let internal = internal_step(
        "write-definition",
        "install",
        "write",
        Vec::new(),
        vec![Path::new("/tmp/definition")],
        vec![Path::new("/tmp/old-definition")],
    );
    let command = command_step(
        "install-service",
        "install",
        "register",
        vec!["systemctl".to_owned()],
        true,
    );

    assert_eq!(internal.writes_paths, ["/tmp/definition"]);
    assert_eq!(internal.removes_paths, ["/tmp/old-definition"]);
    assert!(!internal.requires_privilege);
    assert!(command.requires_privilege);
    assert!(command.writes_paths.is_empty());
}
