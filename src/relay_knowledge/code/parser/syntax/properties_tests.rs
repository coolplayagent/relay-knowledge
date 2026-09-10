use super::*;
use std::{sync::Barrier, thread, time::Duration};

fn properties() -> LanguageSpec {
    LanguageSpec {
        id: "properties",
        language: || tree_sitter_properties::LANGUAGE.into(),
        tags_query: "",
    }
}

#[test]
fn concurrent_properties_parses_keep_every_entry_with_the_original_work_budget() {
    let start = Barrier::new(8);
    thread::scope(|scope| {
        for worker in 0..8 {
            let start = &start;
            scope.spawn(move || {
                start.wait();
                for round in 0..12 {
                    let entries = if round % 2 == 0 { 1000 } else { 100 };
                    let content = (0..entries)
                        .map(|key| format!("flag_{worker}_{key:04}=true\n"))
                        .collect::<String>();
                    let tree = parse_tree(properties(), &content).unwrap();
                    let root = tree.root_node();
                    assert!(!root.has_error());
                    assert_eq!(root.end_byte(), content.len());
                    let mut cursor = root.walk();
                    assert_eq!(
                        root.named_children(&mut cursor)
                            .filter(|node| node.kind() == "property")
                            .count(),
                        entries
                    );
                }
            });
        }
    });
}

#[test]
fn cancelled_properties_parse_resets_before_the_next_file() {
    let content = "flag=true\n".repeat(1000);
    let error = parse_tree_with_budget(properties(), &content, 0).unwrap_err();
    assert!(error.to_string().contains("bounded syntax budget"));
    let tree = parse_tree(properties(), "next=true").unwrap();
    assert!(!tree.root_node().has_error());
    assert_eq!(tree.root_node().named_child_count(), 1);
}

#[test]
fn poisoned_properties_guard_recovers_by_resetting_the_cached_parser() {
    let panic = panic::catch_unwind(AssertUnwindSafe(|| {
        with_syntax_parser(properties(), |parser| {
            assert!(parser.parse("before=true", None).is_some());
            panic!("simulated parser callback panic");
        })
    }));
    assert!(panic.is_err());
    let tree = parse_tree(properties(), "after=true").unwrap();
    assert!(!tree.root_node().has_error());
    assert_eq!(tree.root_node().named_child_count(), 1);
}

#[test]
fn properties_scanner_ownership_does_not_block_other_languages() {
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    thread::scope(|scope| {
        let owner = scope.spawn(move || {
            with_syntax_parser(properties(), |_| {
                ready_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            })
            .unwrap();
        });
        ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let rust = LanguageSpec {
            id: "rust",
            language: || tree_sitter_rust::LANGUAGE.into(),
            tags_query: "",
        };
        let tree = parse_tree(rust, "fn unrelated() {}");
        release_tx.send(()).unwrap();
        owner.join().unwrap();
        assert!(!tree.unwrap().root_node().has_error());
    });
}
