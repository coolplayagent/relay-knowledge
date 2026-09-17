//! Thread-local deterministic fault injection, compiled only in unit tests.
use super::*;
use std::{cell::RefCell, path::PathBuf};
struct Fault {
    path: PathBuf,
    operation: FaultOperation,
    after: usize,
    error: io::Error,
}
#[derive(PartialEq, Eq)]
enum FaultOperation {
    Source(CodePathIoOperation),
    DirectoryEntryType,
}
thread_local! { static NEXT: RefCell<Option<Fault>> = const { RefCell::new(None) }; }
pub struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        NEXT.with(|next| *next.borrow_mut() = None);
    }
}
pub fn inject(
    path: PathBuf,
    operation: CodePathIoOperation,
    after: usize,
    error: io::Error,
) -> Guard {
    install(path, FaultOperation::Source(operation), after, error)
}

pub fn inject_entry_type(path: PathBuf, error: io::Error) -> Guard {
    install(path, FaultOperation::DirectoryEntryType, 0, error)
}

fn install(path: PathBuf, operation: FaultOperation, after: usize, error: io::Error) -> Guard {
    NEXT.with(|next| {
        assert!(
            next.borrow().is_none(),
            "only one local source fault per thread"
        );
        *next.borrow_mut() = Some(Fault {
            path,
            operation,
            after,
            error,
        });
    });
    Guard
}
pub(super) fn check(path: &Path, operation: CodePathIoOperation) -> io::Result<()> {
    check_operation(path, FaultOperation::Source(operation))
}

pub(super) fn check_entry_type(path: &Path) -> io::Result<()> {
    check_operation(path, FaultOperation::DirectoryEntryType)
}

fn check_operation(path: &Path, operation: FaultOperation) -> io::Result<()> {
    NEXT.with(|next| {
        let mut next = next.borrow_mut();
        if let Some(fault) = next.as_mut() {
            // Windows canonicalization adds a verbatim prefix to the same fixture path.
            let actual = path.to_string_lossy().replace('\\', "/");
            let expected = fault.path.to_string_lossy().replace('\\', "/");
            if actual.trim_start_matches("//?/") == expected.trim_start_matches("//?/")
                && fault.operation == operation
            {
                if fault.after > 0 {
                    fault.after -= 1;
                } else {
                    return Err(next.take().unwrap().error);
                }
            }
        }
        Ok(())
    })
}
