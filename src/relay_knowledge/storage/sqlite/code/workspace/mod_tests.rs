use super::{resolve_workspace_imports, test_support::workspace_schema_connection};

#[test]
fn empty_workspace_resolution_is_idempotent_without_prior_state() {
    let mut connection = workspace_schema_connection();
    let transaction = connection.transaction().expect("transaction");

    resolve_workspace_imports(&transaction, &[], "repo", "scope")
        .expect("empty workspace resolution should be a no-op");
}
