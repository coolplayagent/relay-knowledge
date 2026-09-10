use super::super::FilesystemWorkspaceSource;
use super::*;

#[test]
fn namespaced_pom_extracts_members_without_external_reads() {
    let pom = summary("<m:project xmlns:m='urn:maven'><m:parent><m:groupId>demo</m:groupId></m:parent><m:artifactId>a</m:artifactId><m:modules><m:module>child</m:module></m:modules></m:project>").unwrap();
    assert_eq!(pom.coordinate().as_deref(), Some("demo:a"));
    assert_eq!(pom.modules, ["child"]);
    assert!(summary("<project>").is_none());
    assert!(
        summary("<project><artifactId>${name}</artifactId></project>")
            .unwrap()
            .coordinate()
            .is_none()
    );
    assert_eq!(child_pom("a", "../b"), Some("b/pom.xml".into()));
    assert_eq!(child_pom("", "a/pom.xml"), Some("a/pom.xml".into()));
    for value in ["../outside", "/absolute", "C:/escape", "${module}"] {
        assert!(child_pom("", value).is_none());
    }
}

#[test]
fn detects_nested_modules_and_stops_cycles() {
    let root = std::env::temp_dir().join(format!("maven-workspace-{}", std::process::id()));
    std::fs::create_dir_all(root.join("child")).unwrap();
    std::fs::write(root.join("pom.xml"), "<project><groupId>x</groupId><artifactId>root</artifactId><modules><module>child</module></modules></project>").unwrap();
    std::fs::write(root.join("child/pom.xml"), "<project><groupId>x</groupId><artifactId>child</artifactId><modules><module>..</module></modules></project>").unwrap();
    let members = detect(&FilesystemWorkspaceSource::new(&root)).unwrap();
    assert_eq!(members.len(), 2);
    assert_eq!(members[1].relative_path, "child");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cdata_and_text_fragments_form_single_coordinates_and_module_paths() {
    let pom = summary("<project><groupId><![CDATA[ demo ]]></groupId><artifactId>ro<![CDATA[ot]]></artifactId><modules><module>chi<![CDATA[ld]]></module><module><![CDATA[other&literal]]></module></modules></project>").unwrap();
    assert_eq!(pom.coordinate().as_deref(), Some("demo:root"));
    assert_eq!(pom.modules, ["child", "other&literal"]);
    let root = std::env::temp_dir().join(format!("maven-cdata-workspace-{}", std::process::id()));
    std::fs::create_dir_all(root.join("child")).unwrap();
    std::fs::write(root.join("pom.xml"), "<project><groupId><![CDATA[x]]></groupId><artifactId><![CDATA[root]]></artifactId><modules><module><![CDATA[child]]></module></modules></project>").unwrap();
    std::fs::write(root.join("child/pom.xml"), "<project><parent><groupId><![CDATA[x]]></groupId></parent><artifactId><![CDATA[child]]></artifactId></project>").unwrap();
    let members = detect(&FilesystemWorkspaceSource::new(&root)).unwrap();
    assert_eq!(members.len(), 2);
    assert_eq!(members[1].package_name, "x:child");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn maven_workspace_discovers_only_default_profile_members_in_any_element_order() {
    let root = std::env::temp_dir().join(format!("maven-profile-workspace-{}", std::process::id()));
    std::fs::create_dir_all(root.join("child")).unwrap();
    std::fs::write(
        root.join("child/pom.xml"),
        "<project><groupId>x</groupId><artifactId>child</artifactId></project>",
    )
    .unwrap();
    for profile in [
        "<modules><module><![CDATA[child]]></module></modules><activation><activeByDefault>true</activeByDefault></activation>",
        "<activation><activeByDefault>true</activeByDefault></activation><modules><module>child</module></modules>",
    ] {
        let xml = format!(
            "<project><groupId>x</groupId><artifactId>root</artifactId><profiles><profile><id>default</id>{profile}</profile><profile><id>opt-in</id><modules><module>excluded</module></modules></profile></profiles></project>"
        );
        assert_eq!(summary(&xml).unwrap().modules, ["child"]);
        std::fs::write(root.join("pom.xml"), xml).unwrap();
        assert_eq!(
            detect(&FilesystemWorkspaceSource::new(&root))
                .unwrap()
                .len(),
            2
        );
    }
    std::fs::remove_dir_all(root).unwrap();
    let modules = "<module>x</module>".repeat(MAX_MEMBERS + 1);
    assert!(summary(&format!("<project><profiles><profile><modules>{modules}</modules></profile></profiles></project>")).is_none());
    let xml = format!(
        "<project><modules><module>base</module></modules><profiles><profile><modules>{}</modules><activation><activeByDefault>true</activeByDefault></activation></profile></profiles></project>",
        "<module>x</module>".repeat(MAX_MEMBERS)
    );
    assert!(summary(&xml).is_none());
}
