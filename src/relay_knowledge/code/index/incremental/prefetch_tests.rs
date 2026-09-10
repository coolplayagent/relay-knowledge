use super::*;
#[test]
fn origin_plan_caps_all_files_before_prefetch_including_expanded_gitlinks() {
    let source = crate::code::test_fixtures::TempSourceDir::create("origin-plan-budget");
    let registration = source.registration();
    let selector = source.selector();
    for (count, bytes, accepted) in [
        (64, 262144, true),
        (65, 262144, false),
        (512, 0, true),
        (513, 0, false),
    ] {
        let entries = (0..count)
            .map(|i| changes::GitTreeEntry {
                path: format!("vendor/app{i}.py"),
                byte_count: bytes,
            })
            .collect::<Vec<_>>();
        let changes = entries
            .iter()
            .map(|entry| GitChange::AddedOrModified {
                path: entry.path.clone(),
            })
            .collect::<Vec<_>>();
        let layout = discover_source_layout(&entries);
        let request = ChangedPathPrefetchRequest {
            reparse_python: true,
            registration: &registration,
            selector: &selector,
            root: &source.path,
            commit: "unused-before-admission",
            changes: &changes,
            head_entries: &entries,
            source_layout: &layout,
            previous_source_layout: &layout,
        };
        assert_eq!(
            validate_origin_plan(&request).is_ok(),
            accepted,
            "{count}/{bytes}"
        );
        if !accepted {
            let error = prefetch_changed_path_bytes(request).unwrap_err();
            assert!(error.to_string().contains("bounded file/byte budget"));
        }
    }
}

#[test]
fn exact_origin_plan_does_not_charge_unmodified_gitlink_descendants() {
    let source = crate::code::test_fixtures::TempSourceDir::create("origin-exact-work");
    let registration = source.registration();
    let selector = source.selector();
    let mut entries = (0..512)
        .map(|i| changes::GitTreeEntry {
            path: format!("vendor/unchanged{i}.rs"),
            byte_count: 16 * 1024 * 1024,
        })
        .collect::<Vec<_>>();
    entries.extend(
        ["typing.py", "vendor/app.py"].map(|path| changes::GitTreeEntry {
            path: path.into(),
            byte_count: 10,
        }),
    );
    let layout = discover_source_layout(&entries);
    let changes =
        ["typing.py", "vendor/app.py"].map(|path| GitChange::AddedOrModified { path: path.into() });
    let request = ChangedPathPrefetchRequest {
        reparse_python: true,
        registration: &registration,
        selector: &selector,
        root: &source.path,
        commit: "no-source-read",
        changes: &changes,
        head_entries: &entries,
        source_layout: &layout,
        previous_source_layout: &layout,
    };
    assert!(validate_origin_plan(&request).is_ok());
}
