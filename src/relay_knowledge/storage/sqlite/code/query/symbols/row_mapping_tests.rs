use super::*;
use rusqlite::Connection;

#[test]
fn symbol_row_mapping_preserves_ranges_and_generated_state() {
    let connection = Connection::open_in_memory().expect("database should open");

    let symbol = connection
        .query_row(
            "
            SELECT 'snapshot', 'canonical', 'file', 'src/lib.rs', 'rust',
                   'fn run()', 'documentation', 10, 20, 3, 5, 'run',
                   'crate::run', 'function', 1, 2
            ",
            [],
            row_to_symbol,
        )
        .expect("symbol row should decode");

    assert_eq!(symbol.symbol_snapshot_id, "snapshot");
    assert_eq!(symbol.path, "src/lib.rs");
    assert_eq!(
        symbol.byte_range,
        RepositoryCodeRange { start: 10, end: 20 }
    );
    assert_eq!(symbol.line_range, RepositoryCodeRange { start: 3, end: 5 });
    assert!(symbol.is_generated);
    assert_eq!(symbol.previous_symbol_context_start, Some(2));
}
