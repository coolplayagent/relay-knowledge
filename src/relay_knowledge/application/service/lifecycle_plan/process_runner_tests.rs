//! Direct unit contract for bounded external-command output collection.

use std::io::Cursor;

use super::*;

#[test]
fn pipe_drain_discards_excess_bytes_after_the_retention_budget() {
    let retained = drain_pipe_limited(Cursor::new(vec![b'x'; 32]), 7)
        .join()
        .expect("pipe reader should join");

    assert_eq!(retained, vec![b'x'; 7]);
}
