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
#[path = "canonical_calls_tests.rs"]
mod tests;
