const CODE_SNAPSHOT_FACT_VERSION: &str = "code-facts-js-ts-import-edges-v1-sbom-dependencies-v2-python-type-refs-v1-scope-compat-v1-workspace-imports-v1-generated-files-v1-web-routes-v1";

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

pub fn code_snapshot_expected_scope_id(
    repository_id: &str,
    tree_hash: &str,
    path_filters: &[String],
    language_filters: &[String],
) -> Option<String> {
    Some(code_snapshot_scope_id(
        repository_id,
        tree_hash,
        path_filters,
        language_filters,
    ))
}

pub fn code_snapshot_scope_is_fact_versioned(source_scope: &str) -> bool {
    let Some(scope_hash) = source_scope.strip_prefix("git_snapshot:") else {
        return false;
    };
    scope_hash.len() == 16
        && scope_hash
            .chars()
            .all(|character| character.is_ascii_hexdigit())
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

fn stable_hash64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    hash
}

#[cfg(test)]
#[path = "scope_identity_tests.rs"]
mod tests;
