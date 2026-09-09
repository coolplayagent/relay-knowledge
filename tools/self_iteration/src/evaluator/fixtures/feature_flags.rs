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
#[path = "feature_flags_tests.rs"]
mod tests;
