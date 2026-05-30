use crate::domain::CodeRepositoryRegistration;

use super::*;

#[test]
fn json_and_yaml_config_paths_preserve_array_shape() {
    let json = parse_source_snapshot(
        "config.json",
        br#"{"server":{"port":8080},"containers":[{"name":"app"},{"name":"sidecar"}]}"#,
    );
    assert_config_symbol(&json, "server.port");
    assert_config_symbol(&json, "containers[].name");
    assert_no_config_symbol(&json, "containers.name");

    let yaml = parse_source_snapshot(
        "config.yaml",
        b"containers:\n  - name: app\nserver:\n  port: 8080\n",
    );
    assert_config_symbol(&yaml, "containers[].name");
    assert_config_symbol(&yaml, "server.port");
    assert_no_config_symbol(&yaml, "containers.name");
}

#[test]
fn nested_sequence_config_paths_preserve_each_array_level() {
    let json = parse_source_snapshot(
        "matrix.json",
        br#"{"matrix":[[{"name":"app"}]],"plain":[{"name":"worker"}]}"#,
    );
    assert_config_symbol(&json, "matrix[][].name");
    assert_config_symbol(&json, "plain[].name");
    assert_no_config_symbol(&json, "matrix[].name");

    let yaml = parse_source_snapshot(
        "matrix.yaml",
        b"matrix:\n  - - name: app\nplain:\n  - name: worker\n",
    );
    assert_config_symbol(&yaml, "matrix[][].name");
    assert_config_symbol(&yaml, "plain[].name");
    assert_no_config_symbol(&yaml, "matrix[].name");
}

#[test]
fn heterogeneous_sequence_config_paths_follow_the_matched_branch() {
    let json = parse_source_snapshot(
        "heterogeneous.json",
        br#"{"items":[{"name":"flat"},[{"name":"nested"}]]}"#,
    );

    assert_config_symbol(&json, "items[].name");
    assert_config_symbol(&json, "items[][].name");

    let yaml = parse_source_snapshot(
        "heterogeneous.yaml",
        b"items:\n  - name: flat\n  - - name: nested\n",
    );
    assert_config_symbol(&yaml, "items[].name");
    assert_config_symbol(&yaml, "items[][].name");
}

#[test]
fn toml_and_ini_config_paths_include_sections() {
    let toml = parse_source_snapshot(
        "Cargo.toml",
        b"[package]\nname = \"relay-knowledge\"\n[package.metadata]\nowner = \"team\"\n[[bin]]\nname = \"relay-knowledge\"\n",
    );
    assert_section_symbol(&toml, "package");
    assert_config_symbol(&toml, "package.name");
    assert_section_symbol(&toml, "package.metadata");
    assert_config_symbol(&toml, "package.metadata.owner");
    assert_section_symbol(&toml, "bin[]");
    assert_config_symbol(&toml, "bin[].name");

    let ini = parse_source_snapshot(
        "settings.ini",
        b"[server]\nenabled=true\nport: 8080\n[server.tls]\ncert=server.pem\n",
    );
    assert_section_symbol(&ini, "server");
    assert_config_symbol(&ini, "server.enabled");
    assert_config_symbol(&ini, "server.port");
    assert_section_symbol(&ini, "server.tls");
    assert_config_symbol(&ini, "server.tls.cert");
}

#[test]
fn nested_toml_array_tables_preserve_parent_array_shape() {
    let toml = parse_source_snapshot(
        "nested.toml",
        b"[[fruits]]\nname = \"apple\"\n[[fruits.varieties]]\nname = \"gala\"\n",
    );

    assert_section_symbol(&toml, "fruits[]");
    assert_config_symbol(&toml, "fruits[].name");
    assert_section_symbol(&toml, "fruits[].varieties[]");
    assert_config_symbol(&toml, "fruits[].varieties[].name");
    assert_no_config_symbol(&toml, "fruits.varieties[].name");
}

fn assert_config_symbol(snapshot: &crate::domain::CodeIndexSnapshot, name: &str) {
    assert_symbol(snapshot, name, "config");
}

fn assert_section_symbol(snapshot: &crate::domain::CodeIndexSnapshot, name: &str) {
    assert_symbol(snapshot, name, "section");
}

fn assert_no_config_symbol(snapshot: &crate::domain::CodeIndexSnapshot, name: &str) {
    assert!(
        !snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == name && symbol.kind == "config"),
        "config symbol {name} should not be indexed: {:?}",
        snapshot.symbols
    );
}

fn assert_symbol(snapshot: &crate::domain::CodeIndexSnapshot, name: &str, kind: &str) {
    assert!(
        snapshot
            .symbols
            .iter()
            .any(|symbol| symbol.name == name && symbol.kind == kind),
        "{kind} symbol {name} should be indexed: {:?}",
        snapshot.symbols
    );
}

fn parse_source_snapshot(path: &str, source: &[u8]) -> crate::domain::CodeIndexSnapshot {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );

    parse_indexed_file(&mut build, path, source).expect("file should parse");

    build.finish()
}
