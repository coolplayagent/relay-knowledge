use super::*;

#[test]
fn default_graph_inspection_is_an_empty_non_sqlite_snapshot() {
    let inspection = GraphInspection::default();

    assert_eq!(inspection.graph_version, GraphVersion::ZERO);
    assert_eq!(inspection.entity_count, 0);
    assert_eq!(inspection.code_file_count, 0);
    assert_eq!(
        inspection.code_parse_status_counts,
        CodeParseStatusCounts::default()
    );
    assert_eq!(inspection.sqlite, SqliteStorageDiagnostics::default());
}
