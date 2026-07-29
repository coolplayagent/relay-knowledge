use super::*;

#[test]
fn exposes_numeric_graph_version() {
    assert_eq!(GraphVersion::ZERO.get(), 0);
    assert_eq!(GraphVersion::new(42).get(), 42);
}
