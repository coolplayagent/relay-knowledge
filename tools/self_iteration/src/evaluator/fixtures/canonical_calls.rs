//! Many unrelated Java calls protect exact-selector query admission and indexes.
use std::path::Path;

pub(super) fn write(root: &Path) -> Result<(), String> {
    for shard in 0..32 {
        let mut source = format!("public class Noise{shard} {{\n");
        for method in 0..64 {
            source.push_str(&format!(
                "  void worker{method}() {{ System.nanoTime(); }}\n"
            ));
        }
        source.push_str("}\n");
        super::write_fixture_file(&root.join(format!("src/Noise{shard}.java")), &source)?;
    }
    super::write_fixture_file(
        &root.join("src/Target.java"),
        "public class Target { public static void selectedOperation() {} }\n",
    )?;
    super::write_fixture_file(
        &root.join("src/Caller.java"),
        "public class Caller { void execute() { Target.selectedOperation(); } }\n",
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn fixture_contains_many_unrelated_calls_and_a_unique_target() {
        let root = std::env::temp_dir().join(format!(
            "canonical-call-fixture-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        super::write(&root).unwrap();
        assert_eq!(std::fs::read_dir(root.join("src")).unwrap().count(), 34);
        assert_eq!(
            std::fs::read_to_string(root.join("src/Noise0.java"))
                .unwrap()
                .matches("System.nanoTime()")
                .count(),
            64
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
