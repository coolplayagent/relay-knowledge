use std::io::{self, Cursor, Write};

use super::*;

struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn reader_worker_collects_utf8_and_missing_reader_is_empty() {
    let reader = read_pipe(Cursor::new(b"captured output".to_vec()));

    assert_eq!(join_reader(Some(reader)), "captured output");
    assert!(join_reader(None).is_empty());
}

#[test]
fn stdin_writer_errors_append_without_erasing_existing_stderr() {
    let writer = write_pipe(FailingWriter, "input".to_owned());
    let mut stderr = "process error".to_owned();

    append_stdin_error(&mut stderr, Some(writer));

    assert!(stderr.starts_with("process error\n"));
    assert!(stderr.contains("stdin write failed: closed"));
}

#[test]
fn absent_stdin_writer_leaves_stderr_unchanged() {
    let mut stderr = "process error".to_owned();

    append_stdin_error(&mut stderr, None);

    assert_eq!(stderr, "process error");
}
