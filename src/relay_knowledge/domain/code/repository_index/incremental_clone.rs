//! Canonical durable checkpoint token for an incremental base clone.

const PREFIX: &str = "staging:incremental_clone";
const VERSION: u32 = 1;
const NO_CURSOR: &str = "none";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodeIncrementalClonePhase {
    Tables,
    Search,
    CloneComplete,
}

impl CodeIncrementalClonePhase {
    const fn code(self) -> u8 {
        match self {
            Self::Tables => 0,
            Self::Search => 1,
            Self::CloneComplete => 2,
        }
    }

    fn parse(code: &str) -> Option<Self> {
        match code {
            "0" => Some(Self::Tables),
            "1" => Some(Self::Search),
            "2" => Some(Self::CloneComplete),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodeIncrementalCloneCheckpoint {
    pub(crate) phase: CodeIncrementalClonePhase,
    pub(crate) table_ordinal: usize,
    pub(crate) completed_page_ordinal: usize,
    pub(crate) scanned_total_rows: usize,
    pub(crate) cursor_digest: String,
}

pub(crate) fn code_incremental_clone_state(
    phase: CodeIncrementalClonePhase,
    table_ordinal: usize,
    completed_page_ordinal: usize,
    scanned_total_rows: usize,
    cursor_digest: &str,
) -> Option<String> {
    valid_cursor_digest(cursor_digest).then(|| {
        format!(
            "{PREFIX}:v{VERSION}:{}:{table_ordinal}:{completed_page_ordinal}:{scanned_total_rows}:{cursor_digest}",
            phase.code()
        )
    })
}

pub(crate) fn code_incremental_clone(state: &str) -> Option<CodeIncrementalCloneCheckpoint> {
    let suffix = state.strip_prefix(&format!("{PREFIX}:v{VERSION}:"))?;
    let mut parts = suffix.split(':');
    let phase = CodeIncrementalClonePhase::parse(parts.next()?)?;
    let table_ordinal = parts.next()?.parse::<usize>().ok()?;
    let completed_page_ordinal = parts.next()?.parse::<usize>().ok()?;
    let scanned_total_rows = parts.next()?.parse::<usize>().ok()?;
    let cursor_digest = parts.next()?;
    if parts.next().is_some() || !valid_cursor_digest(cursor_digest) {
        return None;
    }
    let canonical = code_incremental_clone_state(
        phase,
        table_ordinal,
        completed_page_ordinal,
        scanned_total_rows,
        cursor_digest,
    )?;
    (canonical == state).then_some(CodeIncrementalCloneCheckpoint {
        phase,
        table_ordinal,
        completed_page_ordinal,
        scanned_total_rows,
        cursor_digest: cursor_digest.to_owned(),
    })
}

fn valid_cursor_digest(value: &str) -> bool {
    value == NO_CURSOR
        || (value.len() == 16
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
}

#[cfg(test)]
#[path = "incremental_clone_tests.rs"]
mod tests;
