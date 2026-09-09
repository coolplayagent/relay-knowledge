use super::*;

#[test]
fn overlapping_origin_paths_parse_and_charge_once_in_either_visit_order() {
    let source = crate::code::test_fixtures::TempSourceDir::create("origin-path-once");
    let registration = source.registration();
    let selector = source.selector();
    let entries = ["pkg/app.py", "pkg2/app.py"].map(|path| changes::GitTreeEntry {
        path: path.into(),
        byte_count: 25,
    });
    let layout = discover_source_layout(&entries);
    let bytes = entries
        .iter()
        .map(|entry| (entry.path.clone(), b"def leaf(): return 1\n".to_vec()))
        .collect();
    let hashes = BTreeMap::new();
    for order in [
        ["pkg/app.py", "pkg2/app.py", "pkg/app.py"],
        ["pkg2/app.py", "pkg/app.py", "pkg2/app.py"],
    ] {
        let context = ChangedPathParseContext {
            reparse_python: true,
            origin_plan: Default::default(),
            visited_origin_paths: Default::default(),
            origin_budget: Default::default(),
            registration: &registration,
            selector: &selector,
            root: &source.path,
            base_commit: "base",
            previous_hashes: &hashes,
            source_layout: &layout,
            previous_source_layout: &layout,
            effective_path_filters: &[],
            prefetched_bytes: &bytes,
        };
        let mut build = SnapshotBuild::new(&registration, "head".into(), "tree".into(), true, 2, 0);
        for path in order {
            parse_changed_path(&mut build, &context, path).unwrap();
        }
        assert_eq!(build.files.len(), 2);
        assert_eq!(build.symbols.len(), 2);
        for _ in 0..510 {
            context.origin_budget.borrow_mut().charge(0).unwrap();
        }
        assert!(context.origin_budget.borrow_mut().charge(0).is_err());
    }
}

#[test]
fn origin_work_queue_is_bounded_before_any_source_reads() {
    let source = crate::code::test_fixtures::TempSourceDir::create("origin-queue-bound");
    let registration = source.registration();
    let selector = source.selector();
    let entries = (0..513)
        .map(|i| changes::GitTreeEntry {
            path: format!("app{i}.py"),
            byte_count: 0,
        })
        .collect::<Vec<_>>();
    let layout = discover_source_layout(&entries);
    let hashes = BTreeMap::new();
    let bytes = BTreeMap::new();
    let context = ChangedPathParseContext {
        reparse_python: true,
        origin_plan: std::cell::RefCell::new(Some(BTreeSet::new())),
        visited_origin_paths: Default::default(),
        origin_budget: Default::default(),
        registration: &registration,
        selector: &selector,
        root: &source.path,
        base_commit: "unused",
        previous_hashes: &hashes,
        source_layout: &layout,
        previous_source_layout: &layout,
        effective_path_filters: &[],
        prefetched_bytes: &bytes,
    };
    let mut build = SnapshotBuild::new(&registration, "head".into(), "tree".into(), true, 513, 0);
    for entry in entries.iter().take(512) {
        parse_changed_path(&mut build, &context, &entry.path).unwrap();
    }
    parse_changed_path(&mut build, &context, &entries[0].path).unwrap();
    assert!(build.files.is_empty());
    assert_eq!(context.origin_plan.borrow().as_ref().unwrap().len(), 512);
    assert!(
        parse_changed_path(&mut build, &context, &entries[512].path)
            .unwrap_err()
            .to_string()
            .contains("bounded file/byte budget")
    );
}
