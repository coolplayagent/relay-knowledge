//! Class-name query regression with a bounded wide Java call graph.
use super::writer::write_fixture_file;
use std::path::Path;

pub(super) fn write_java_class_calls(root: &Path) -> Result<(), String> {
    write_fixture_file(
        &root.join("src/Processor.java"),
        "public class Processor { public static void processItem() { System.out.println(\"processed\"); } }\n",
    )?;
    write_fixture_file(
        &root.join("src/Caller.java"),
        "public class Caller { public static void run() { Processor.processItem(); } }\n",
    )?;
    for index in 0..256 {
        let mut source = format!("public class Noise{index} {{ public void runNoise() {{\n");
        for _ in 0..128 {
            source.push_str("System.out.println(\"unrelated\");\n");
        }
        source.push_str("} }\n");
        write_fixture_file(&root.join(format!("src/Noise{index}.java")), &source)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "java_class_calls_tests.rs"]
mod tests;
