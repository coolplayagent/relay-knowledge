//! A bounded configuration scope larger than a full-analysis usage budget.
use std::path::Path;

pub(super) fn write(root: &Path) -> Result<(), String> {
    for shard in 0..11 {
        let mut content = String::new();
        for key in 0..1000 {
            content.push_str(&format!("unrelated_{shard:02}_{key:04}=true\n"));
        }
        super::write_fixture_file(
            &root.join(format!("config/noise_{shard:02}.properties")),
            &content,
        )?;
    }
    super::write_fixture_file(
        &root.join("config/selected.properties"),
        "selected_switch=true\n",
    )?;
    super::write_fixture_file(
        &root.join("src/Settings.java"),
        "public class Settings { boolean enabled() { return Boolean.getBoolean(\"selected_switch\"); } }\n",
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn generated_configuration_scope_exceeds_analysis_budget_without_large_files() {
        let root = std::env::temp_dir().join(format!(
            "config-width-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        super::write(&root).unwrap();
        let mut count = 0;
        for entry in std::fs::read_dir(root.join("config")).unwrap() {
            let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
            assert!(text.len() < 32 * 1024);
            count += text.lines().count();
        }
        assert_eq!(count, 11_001);
        std::fs::remove_dir_all(root).unwrap();
    }
}
