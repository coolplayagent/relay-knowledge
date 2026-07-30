use super::*;

#[test]
fn fixture_writer_creates_parent_directories() {
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-fixture-writer-{}",
        std::process::id()
    ));
    let path = root.join("nested/source.rs");

    write_fixture_file(&path, "fn fixture() {}\n").expect("write fixture");

    assert_eq!(
        std::fs::read_to_string(&path).expect("read fixture"),
        "fn fixture() {}\n"
    );
    std::fs::remove_dir_all(root).expect("remove fixture");
}
