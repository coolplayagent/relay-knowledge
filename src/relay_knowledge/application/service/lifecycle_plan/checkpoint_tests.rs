//! Direct unit contract for checkpoint backup and temporary path identity.

use std::path::Path;

use super::*;

#[test]
fn checkpoint_paths_are_attempt_scoped_and_backup_kinds_do_not_collide() {
    let path = Path::new("/tmp/relay-knowledge.service");
    let definition = backup_path(path, CheckpointBackupKind::Definition, "attempt-1");
    let binary = backup_path(path, CheckpointBackupKind::Binary, "attempt-1");
    let temporary = checkpoint_temporary_path(path, "attempt-1");

    assert_ne!(definition, binary);
    assert!(definition.ends_with("relay-knowledge.service.definition.attempt-1.rollback"));
    assert!(binary.ends_with("relay-knowledge.service.binary.attempt-1.rollback"));
    assert!(temporary.ends_with("relay-knowledge.service.attempt-1.tmp"));
}
