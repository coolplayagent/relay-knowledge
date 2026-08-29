use crate::identity::stable_hash64;

const CODE_SNAPSHOT_FACT_VERSION: &str = "code-facts-js-ts-import-edges-v1-sbom-dependencies-v2-python-type-refs-v1-scope-compat-v1-workspace-imports-v1-generated-files-v1-web-routes-v1-syntax-failure-chunks-v1-bounded-config-chunks-v1-dense-source-windows-v1-c-composite-tags-v1-doc-block-owner-anchor-v2-bounded-type-doc-summary-v1-search-owner-v2-reference-search-groups-v2";

/// Builds the stable source scope id for a Git snapshot partition.
pub fn code_snapshot_scope_id(
    repository_id: &str,
    tree_hash: &str,
    path_filters: &[String],
    language_filters: &[String],
) -> String {
    let mut input = Vec::new();
    append_hash_part(&mut input, "git_snapshot");
    append_hash_part(&mut input, repository_id);
    append_hash_part(&mut input, tree_hash);
    append_hash_list(&mut input, path_filters);
    append_hash_list(&mut input, language_filters);
    append_hash_part(&mut input, CODE_SNAPSHOT_FACT_VERSION);

    format!("git_snapshot:{:016x}", stable_hash64(&input))
}

/// Builds a scope identity that includes workspace-detection semantics when
/// those semantics can add persisted workspace graph facts.
pub fn code_snapshot_scope_id_with_workspace_detection(
    repository_id: &str,
    tree_hash: &str,
    path_filters: &[String],
    language_filters: &[String],
    config: &super::super::workspace::CodeWorkspaceDetectionConfig,
) -> String {
    let base = code_snapshot_scope_id(repository_id, tree_hash, path_filters, language_filters);
    workspace_detection_mask(config)
        .map_or(base.clone(), |mask| format!("{base}:workspace-v1:{mask}"))
}

/// Accepts every canonical supported workspace configuration while still
/// rejecting scopes from older code-fact versions or unrelated identities.
pub fn code_snapshot_scope_matches_identity(
    repository_id: &str,
    tree_hash: &str,
    path_filters: &[String],
    language_filters: &[String],
    source_scope: &str,
) -> bool {
    let base = code_snapshot_scope_id(repository_id, tree_hash, path_filters, language_filters);
    parse_scope_identity(source_scope).is_some_and(|identity| identity.base == base)
}

/// Returns the workspace semantic encoded by a valid scope identity.
/// `Some(None)` is the backward-compatible disabled identity; an enabled
/// configuration, including mask zero, is `Some(Some(mask))`.
pub fn code_snapshot_scope_workspace_semantic(
    repository_id: &str,
    tree_hash: &str,
    path_filters: &[String],
    language_filters: &[String],
    source_scope: &str,
) -> Option<Option<u8>> {
    let expected_base =
        code_snapshot_scope_id(repository_id, tree_hash, path_filters, language_filters);
    let identity = parse_scope_identity(source_scope)?;
    (identity.base == expected_base).then_some(identity.workspace_mask)
}

fn workspace_detection_mask(
    config: &super::super::workspace::CodeWorkspaceDetectionConfig,
) -> Option<u8> {
    if !config.enabled {
        return None;
    }
    let formats = [
        super::super::workspace::CodeMonorepoWorkspaceFormat::Pnpm,
        super::super::workspace::CodeMonorepoWorkspaceFormat::GoModules,
        super::super::workspace::CodeMonorepoWorkspaceFormat::CargoWorkspace,
    ];
    Some(
        formats
            .iter()
            .enumerate()
            .fold(0_u8, |mask, (index, format)| {
                mask | (u8::from(config.supported_formats.contains(format)) << index)
            }),
    )
}

pub fn code_snapshot_scope_is_fact_versioned(source_scope: &str) -> bool {
    parse_scope_identity(source_scope).is_some()
}

struct ParsedScopeIdentity {
    base: String,
    workspace_mask: Option<u8>,
}

fn parse_scope_identity(source_scope: &str) -> Option<ParsedScopeIdentity> {
    let mut parts = source_scope.split(':');
    if parts.next()? != "git_snapshot" {
        return None;
    }
    let hash = parts.next()?;
    if hash.len() != 16 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let base = format!("git_snapshot:{hash}");
    let Some(label) = parts.next() else {
        return Some(ParsedScopeIdentity {
            base,
            workspace_mask: None,
        });
    };
    if label != "workspace-v1" {
        return None;
    }
    let encoded = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let mask = encoded.parse::<u8>().ok()?;
    if mask >= 8 || encoded != mask.to_string() {
        return None;
    }
    Some(ParsedScopeIdentity {
        base,
        workspace_mask: Some(mask),
    })
}

/// Returns the clean Git commit carried by a persisted snapshot identity.
///
/// Clean snapshots already store the commit SHA directly. Worktree overlays use
/// `worktree:<base-commit>:<overlay-hash>` and deliberately resolve back to the
/// clean base so a later commit reconciliation never treats dirty files as an
/// incremental Git base. Filesystem identities are not Git commits.
pub fn clean_git_commit_from_snapshot_identity(identity: &str) -> Option<&str> {
    if identity.is_empty() || identity.starts_with("filesystem:") {
        return None;
    }
    let Some(rest) = identity.strip_prefix("worktree:") else {
        return Some(identity);
    };
    let (base_commit, overlay_hash) = rest.split_once(':')?;
    (!base_commit.is_empty() && !overlay_hash.is_empty()).then_some(base_commit)
}

fn append_hash_list(input: &mut Vec<u8>, values: &[String]) {
    input.extend_from_slice(&(values.len() as u64).to_le_bytes());
    for value in values {
        append_hash_part(input, value);
    }
}

fn append_hash_part(input: &mut Vec<u8>, value: &str) {
    input.extend_from_slice(&(value.len() as u64).to_le_bytes());
    input.extend_from_slice(value.as_bytes());
}

#[cfg(test)]
#[path = "scope_identity_tests.rs"]
mod tests;
