//! Exact qualified Java static targets backed by persisted AST ownership.
use crate::domain::CodeTypeOwner;

pub(crate) fn java_static_target(
    language: &str,
    name: &str,
    owner: Option<&CodeTypeOwner>,
) -> Option<String> {
    let owner = owner?;
    if language != "java"
        || owner.static_dispatch != Some(true)
        || owner.relation != "direct_member"
        || owner.resolution_state.as_deref() != Some("resolved")
    {
        return None;
    }
    let (package, ty) = owner
        .lookup_identity
        .as_deref()?
        .strip_prefix("java|")?
        .split_once('|')?;
    if !java_name_path(ty)
        || !java_name_path(name)
        || (!package.is_empty() && !java_name_path(package))
    {
        return None;
    }
    Some(if package.is_empty() {
        format!("{ty}.{name}")
    } else {
        format!("{package}.{ty}.{name}")
    })
}

pub(crate) fn java_name_path(value: &str) -> bool {
    value.len() <= 1024
        && value.split('.').all(|part| {
            let mut chars = part.bytes();
            chars
                .next()
                .is_some_and(|b| b.is_ascii_alphabetic() || matches!(b, b'_' | b'$'))
                && chars.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$'))
        })
}

pub(crate) fn java_source_path(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("java"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_alias_requires_optional_ast_proof_and_safe_lookup_parts() {
        let old = r#"{"identity":"type","relation":"direct_member","target_hint":"B","lookup_identity":"java|demo|B","resolution_state":"resolved"}"#;
        let mut owner: CodeTypeOwner = serde_json::from_str(old).unwrap();
        assert!(java_static_target("java", "run", Some(&owner)).is_none());
        owner.static_dispatch = Some(true);
        assert_eq!(
            java_static_target("java", "run", Some(&owner)).as_deref(),
            Some("demo.B.run")
        );
        for lookup in ["java||B", "java|demo|Outer.B"] {
            owner.lookup_identity = Some(lookup.into());
            assert!(java_static_target("java", "run", Some(&owner)).is_some());
        }
        for lookup in [
            "java|demo|local@1.B",
            "java|bad package|B",
            "rust|demo|B",
            "java|demo",
            "java|demo|",
        ] {
            owner.lookup_identity = Some(lookup.into());
            assert!(java_static_target("java", "run", Some(&owner)).is_none());
        }
        assert!(java_static_target("python", "run", Some(&owner)).is_none());
        assert!(!java_name_path(&"x".repeat(1025)));
        assert!(!java_name_path("1B"));
        assert!(java_source_path("B.JAVA"));
        assert!(!java_source_path("B.java.js"));
    }
}
