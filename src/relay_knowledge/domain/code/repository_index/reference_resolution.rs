//! Owns the canonical durable token for paged ordinary-reference resolution.

use super::{
    CODE_QUERY_INDEX_PLAN_UNIT_COUNT, CODE_QUERY_INDEX_PLAN_VERSION,
    CODE_QUERY_INDEX_REPAIR_PREFIX, LEGACY_CODE_QUERY_INDEX_PLAN_V2,
};

const PREFIX: &str = "finalizing:resolve_references";
const VERSION: u32 = 1;

/// Durable stage within an unpublished ordinary-reference resolution pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodeReferenceResolutionStage {
    Resolve,
}

impl CodeReferenceResolutionStage {
    const fn code(self) -> &'static str {
        match self {
            Self::Resolve => "resolve",
        }
    }

    fn parse(code: &str) -> Option<Self> {
        (code == "resolve").then_some(Self::Resolve)
    }
}

/// Parsed canonical cursor for one durable ordinary-reference page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CodeReferenceResolution {
    pub(crate) protocol_version: u32,
    pub(crate) stage: CodeReferenceResolutionStage,
    pub(crate) completed_page_ordinal: usize,
    pub(crate) completed_reference_count: usize,
    pub(crate) cursor_digest: Option<u64>,
}

impl CodeReferenceResolution {
    pub(crate) fn checkpoint_state(self) -> Option<String> {
        (self.protocol_version == VERSION
            && ((self.completed_page_ordinal == 0 && self.completed_reference_count == 0)
                || (self.completed_page_ordinal > 0
                    && self.completed_reference_count >= self.completed_page_ordinal))
            && (self.completed_page_ordinal == 0) == self.cursor_digest.is_none())
        .then(|| {
            format!(
                "{PREFIX}:v{}:{}:{}:{}:{}",
                self.protocol_version,
                self.stage.code(),
                self.completed_page_ordinal,
                self.completed_reference_count,
                format_cursor_digest(self.cursor_digest),
            )
        })
    }
}

/// Query-index repair cursor that preserves an exact reference-resolution page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CodeReferenceResolutionQueryIndexRepair {
    pub(crate) plan_version: u32,
    pub(crate) completed_unit: usize,
    pub(crate) reference_resolution: CodeReferenceResolution,
}

impl CodeReferenceResolutionQueryIndexRepair {
    pub(crate) const fn requires_legacy_retired_prefix(self) -> bool {
        self.plan_version == LEGACY_CODE_QUERY_INDEX_PLAN_V2
    }

    pub(crate) fn next_state(self, completed_unit: usize) -> Option<String> {
        query_index_repair_state_for_version(
            self.plan_version,
            completed_unit,
            self.reference_resolution,
        )
    }
}

pub(crate) fn code_reference_resolution_state(
    completed_page_ordinal: usize,
    completed_reference_count: usize,
    cursor_reference_id: Option<&str>,
) -> Option<String> {
    CodeReferenceResolution {
        protocol_version: VERSION,
        stage: CodeReferenceResolutionStage::Resolve,
        completed_page_ordinal,
        completed_reference_count,
        cursor_digest: code_reference_resolution_cursor_digest(cursor_reference_id),
    }
    .checkpoint_state()
}

pub(crate) fn code_reference_resolution(state: &str) -> Option<CodeReferenceResolution> {
    let suffix = state.strip_prefix(&format!("{PREFIX}:v"))?;
    let mut parts = suffix.split(':');
    let protocol_version = parts.next()?.parse::<u32>().ok()?;
    let stage = CodeReferenceResolutionStage::parse(parts.next()?)?;
    let completed_page_ordinal = parts.next()?.parse::<usize>().ok()?;
    let completed_reference_count = parts.next()?.parse::<usize>().ok()?;
    let cursor_digest = parse_cursor_digest(parts.next()?)?;
    if protocol_version != VERSION || parts.next().is_some() {
        return None;
    }
    let cursor = CodeReferenceResolution {
        protocol_version,
        stage,
        completed_page_ordinal,
        completed_reference_count,
        cursor_digest,
    };
    (cursor.checkpoint_state().as_deref() == Some(state)).then_some(cursor)
}

pub(crate) fn code_reference_resolution_query_index_repair_state(
    unit: usize,
    reference_resolution: CodeReferenceResolution,
) -> Option<String> {
    query_index_repair_state_for_version(CODE_QUERY_INDEX_PLAN_VERSION, unit, reference_resolution)
}

fn query_index_repair_state_for_version(
    plan_version: u32,
    unit: usize,
    reference_resolution: CodeReferenceResolution,
) -> Option<String> {
    (matches!(
        plan_version,
        LEGACY_CODE_QUERY_INDEX_PLAN_V2 | CODE_QUERY_INDEX_PLAN_VERSION
    ) && unit < CODE_QUERY_INDEX_PLAN_UNIT_COUNT
        && reference_resolution.checkpoint_state().is_some())
        .then(|| {
            format!(
                "{CODE_QUERY_INDEX_REPAIR_PREFIX}:v{plan_version}:{unit}:resume:reference_resolution:v{}:{}:{}:{}:{}",
                reference_resolution.protocol_version,
                reference_resolution.stage.code(),
                reference_resolution.completed_page_ordinal,
                reference_resolution.completed_reference_count,
                format_cursor_digest(reference_resolution.cursor_digest),
            )
        })
}

pub(crate) fn code_reference_resolution_query_index_repair(
    state: &str,
) -> Option<CodeReferenceResolutionQueryIndexRepair> {
    let suffix = state.strip_prefix(&format!("{CODE_QUERY_INDEX_REPAIR_PREFIX}:v"))?;
    let (version_and_unit, resolution) = suffix.split_once(":resume:reference_resolution:v")?;
    let (plan_version, completed_unit) = version_and_unit.split_once(':')?;
    let plan_version = plan_version.parse::<u32>().ok()?;
    let completed_unit = completed_unit.parse::<usize>().ok()?;
    let mut resolution = resolution.split(':');
    let protocol_version = resolution.next()?.parse::<u32>().ok()?;
    let stage = CodeReferenceResolutionStage::parse(resolution.next()?)?;
    let completed_page_ordinal = resolution.next()?.parse::<usize>().ok()?;
    let completed_reference_count = resolution.next()?.parse::<usize>().ok()?;
    let cursor_digest = parse_cursor_digest(resolution.next()?)?;
    if !matches!(
        plan_version,
        CODE_QUERY_INDEX_PLAN_VERSION | LEGACY_CODE_QUERY_INDEX_PLAN_V2
    ) || completed_unit >= CODE_QUERY_INDEX_PLAN_UNIT_COUNT
        || protocol_version != VERSION
        || resolution.next().is_some()
    {
        return None;
    }
    let reference_resolution = CodeReferenceResolution {
        protocol_version,
        stage,
        completed_page_ordinal,
        completed_reference_count,
        cursor_digest,
    };
    let canonical =
        query_index_repair_state_for_version(plan_version, completed_unit, reference_resolution)?;
    (canonical == state).then_some(CodeReferenceResolutionQueryIndexRepair {
        plan_version,
        completed_unit,
        reference_resolution,
    })
}

pub(crate) fn code_reference_resolution_cursor_digest(cursor: Option<&str>) -> Option<u64> {
    cursor.map(|cursor| {
        cursor
            .as_bytes()
            .iter()
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
            })
    })
}

fn format_cursor_digest(digest: Option<u64>) -> String {
    digest.map_or_else(|| "none".to_owned(), |digest| format!("{digest:016x}"))
}

fn parse_cursor_digest(value: &str) -> Option<Option<u64>> {
    if value == "none" {
        return Some(None);
    }
    (value.len() == 16
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    .then(|| u64::from_str_radix(value, 16).ok().map(Some))
    .flatten()
}
