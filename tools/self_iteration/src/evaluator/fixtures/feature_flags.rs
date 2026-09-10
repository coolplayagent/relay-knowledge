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
    // Each individual predicate has more than 1,000 candidates; only their
    // group intersection contains the target. Keep every generated file small.
    for shard in 0..11 {
        let mut query_noise = String::new();
        let mut metadata_noise = String::new();
        for key in 0..100 {
            query_noise.push_str(&format!("# @config domain=other hot-reload=false\naaa_metadata_needle_{shard:02}_{key:03}: true\n"));
            metadata_noise.push_str(&format!("# @config domain=selected hot-reload=true\naab_unrelated_{shard:02}_{key:03}=true\n"));
        }
        super::write_fixture_file(
            &root.join(format!("config/query_noise_{shard:02}.yaml")),
            &query_noise,
        )?;
        super::write_fixture_file(
            &root.join(format!("config/metadata_noise_{shard:02}.properties")),
            &metadata_noise,
        )?;
    }
    super::write_fixture_file(
        &root.join("config/metadata-target.properties"),
        "# @config domain=selected hot-reload=true\nzzz_metadata_needle=true\n",
    )?;
    super::write_fixture_file(
        &root.join("config/selected.properties"),
        "selected_switch=true\n",
    )?;
    super::write_fixture_file(
        &root.join("src/Settings.java"),
        "public class Settings { boolean enabled() { return Boolean.getBoolean(\"selected_switch\"); } }\n",
    )
}

pub(super) fn write_binding_groups(root: &Path) -> Result<(), String> {
    let mut noise = String::new();
    for key in 0..1100 {
        noise.push_str(&format!(
            "# @config domain=other hot-reload=false\naaa_needle_{key:04}: true\n"
        ));
        noise.push_str(&format!(
            "# @config domain=selected hot-reload=true\naab_unrelated_{key:04}: true\n"
        ));
    }
    super::write_fixture_file(&root.join("noise.yaml"), &noise)?;
    super::write_fixture_file(
        &root.join("settings.properties"),
        "# @config domain=selected hot-reload=true\nzzz_needle=true\n# @config domain=alias-selected hot-reload=true\nselected.key=true\n",
    )?;
    super::write_fixture_file(
        &root.join("NeedleReader.java"),
        "class Keys { static final String FIRST = \"selected.key\"; }\npublic class NeedleReader { static String read() { return System.getProperty(Keys.FIRST); } public static void main(String[] args) { System.setProperty(\"selected.key\", \"pass\"); if (!read().equals(\"pass\")) throw new AssertionError(); } }\n",
    )
}

#[cfg(test)]
#[path = "feature_flags_tests.rs"]
mod tests;
