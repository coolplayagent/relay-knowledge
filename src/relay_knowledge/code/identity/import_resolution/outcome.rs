use crate::domain::CodeImportRecord;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::code::identity) enum ImportResolution {
    Resolved,
    Ambiguous,
    Unresolved,
}

pub(in crate::code::identity) enum ModuleFileResolution {
    Resolved(String),
    Ambiguous,
    Unresolved,
}

pub(in crate::code::identity) fn apply_resolution(
    import: &mut CodeImportRecord,
    resolution: ImportResolution,
) {
    match resolution {
        ImportResolution::Resolved => {
            import.resolution_state = "resolved".to_owned();
            import.confidence_basis_points = 8_000;
            import.confidence_tier = "inferred".to_owned();
        }
        ImportResolution::Ambiguous => {
            import.resolution_state = "ambiguous".to_owned();
            import.confidence_basis_points = 5_000;
            import.confidence_tier = "ambiguous".to_owned();
        }
        ImportResolution::Unresolved => {
            import.resolution_state = "unresolved".to_owned();
            import.confidence_basis_points = 2_500;
            import.confidence_tier = "ambiguous".to_owned();
        }
    }
}

pub(in crate::code::identity) fn combined_resolution(
    results: impl IntoIterator<Item = ImportResolution>,
) -> ImportResolution {
    let mut total = 0usize;
    let mut resolved = 0usize;
    let mut ambiguous = false;
    for result in results {
        total += 1;
        match result {
            ImportResolution::Resolved => resolved += 1,
            ImportResolution::Ambiguous => ambiguous = true,
            ImportResolution::Unresolved => {}
        }
    }
    if total == 0 {
        return ImportResolution::Unresolved;
    }
    if ambiguous || (resolved > 0 && resolved < total) {
        return ImportResolution::Ambiguous;
    }
    if resolved == total {
        return ImportResolution::Resolved;
    }

    ImportResolution::Unresolved
}

pub(in crate::code::identity) fn module_file_resolution(
    resolution: ModuleFileResolution,
) -> (ImportResolution, Option<String>) {
    match resolution {
        ModuleFileResolution::Resolved(target_hint) => {
            (ImportResolution::Resolved, Some(target_hint))
        }
        ModuleFileResolution::Ambiguous => (ImportResolution::Ambiguous, None),
        ModuleFileResolution::Unresolved => (ImportResolution::Unresolved, None),
    }
}

pub(super) fn resolution_from_count(count: usize) -> ImportResolution {
    match count {
        0 => ImportResolution::Unresolved,
        1 => ImportResolution::Resolved,
        _ => ImportResolution::Ambiguous,
    }
}

#[cfg(test)]
#[path = "outcome_tests.rs"]
mod tests;
