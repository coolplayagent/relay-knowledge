use std::collections::BTreeSet;

use super::{DYNAMIC_EXPRESS_MOUNT_PREFIX, parse_express_router_mounts};

fn router_names() -> BTreeSet<String> {
    BTreeSet::from([
        "app".to_owned(),
        "authRouter".to_owned(),
        "usersRouter".to_owned(),
    ])
}

#[test]
fn parses_multiple_paths_and_nested_router_arrays() {
    let mounts = parse_express_router_mounts(
        "app.use(['/api', '/v1'], [authRouter, usersRouter]);",
        &router_names(),
    );

    let records = mounts
        .iter()
        .map(|mount| {
            (
                mount.receiver_name.as_str(),
                mount.router_name.as_str(),
                mount.local_prefix.as_str(),
            )
        })
        .collect::<BTreeSet<_>>();

    assert_eq!(records.len(), 4);
    assert!(records.contains(&("app", "authRouter", "/api")));
    assert!(records.contains(&("app", "usersRouter", "/v1")));
}

#[test]
fn marks_dynamic_mount_paths_without_guessing_a_prefix() {
    let mounts = parse_express_router_mounts("app.use(apiPrefix, usersRouter);", &router_names());

    assert_eq!(mounts.len(), 1);
    assert_eq!(mounts[0].local_prefix, DYNAMIC_EXPRESS_MOUNT_PREFIX);
}

#[test]
fn ignores_mount_calls_on_unknown_receivers() {
    let mounts = parse_express_router_mounts("client.use('/api', usersRouter);", &router_names());

    assert!(mounts.is_empty());
}
