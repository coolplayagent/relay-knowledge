use super::*;

#[test]
fn compact_pom_keeps_each_dependency_separate_from_project_and_exclusions() {
    let content = "<project><groupId>project</groupId><artifactId>app</artifactId><version>9</version><dependencies><dependency><groupId>sample</groupId><artifactId>engine</artifactId><version>1</version><exclusions><exclusion><groupId>bad</groupId><artifactId>excluded</artifactId></exclusion></exclusions></dependency><dependency><groupId>sample</groupId><artifactId>api</artifactId><scope>test</scope></dependency></dependencies></project>";
    let mut records = Vec::new();
    parse(content, &mut records).unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].package_name, "sample:engine");
    assert_eq!(records[0].requirement.as_deref(), Some("1"));
    assert_eq!(records[1].package_name, "sample:api");
    assert_eq!(records[1].dependency_group, "test");
}

#[test]
fn pom_events_preserve_namespace_text_entities_comments_and_lines() {
    let content = "<m:dependencies xmlns:m='urn:maven'>\n<!-- <dependency><groupId>fake</groupId></dependency> -->\n<m:dependency>\n<m:groupId>sam<![CDATA[ple]]></m:groupId><m:artifactId>en&#103;ine</m:artifactId><m:version>\n1.2\n</m:version></m:dependency></m:dependencies>";
    let mut records = Vec::new();
    parse(content, &mut records).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].package_name, "sample:engine");
    assert_eq!(records[0].line, 3);
    assert_eq!(records[0].requirement.as_deref(), Some("1.2"));
}

#[test]
fn compact_pom_management_does_not_leak_to_following_dependencies() {
    let content = "<project><dependencyManagement><dependencies><dependency><groupId>a</groupId><artifactId>bom</artifactId><type>pom</type><scope>import</scope></dependency><dependency><groupId>a</groupId><artifactId>managed</artifactId></dependency></dependencies></dependencyManagement><dependencies><dependency><groupId>b</groupId><artifactId>used</artifactId></dependency></dependencies></project>";
    let mut records = Vec::new();
    parse(content, &mut records).unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].dependency_group, "bom");
    assert_eq!(records[1].package_name, "b:used");
    assert_eq!(records[1].dependency_group, "compile");
}

#[test]
fn pom_rejects_ambiguous_or_malformed_coordinates_explicitly() {
    for content in [
        "<dependencies><dependency><groupId>a</groupId><groupId>b</groupId></dependency></dependencies>",
        "<dependencies><dependency><groupId/><groupId>a</groupId></dependency></dependencies>",
        "<dependencies><dependency><groupId>a<part/>b</groupId></dependency></dependencies>",
        "<dependencies><dependency><groupId>&unknown;</groupId></dependency></dependencies>",
        "<dependencies><dependency><groupId>a</groupId></dependencies>",
        "<dependencies><dependency>",
        "<dependencies><dependency><dependencies><dependency/></dependencies></dependency></dependencies><bad>",
    ] {
        assert!(parse(content, &mut Vec::new()).is_err(), "{content}");
    }
}

#[test]
fn pom_enforces_depth_text_event_and_dependency_budgets() {
    for content in [
        "<a>".repeat(MAX_DEPTH + 1),
        format!(
            "{}<a/>{}",
            "<a>".repeat(MAX_DEPTH),
            "</a>".repeat(MAX_DEPTH)
        ),
        format!(
            "<dependencies><dependency><groupId>{}</groupId></dependency></dependencies>",
            "x".repeat(MAX_TEXT + 1)
        ),
        format!(
            "<dependencies>{}</dependencies>",
            "<dependency></dependency>".repeat(MAX_DEPENDENCIES + 1)
        ),
        "<!--x-->".repeat(MAX_EVENTS),
        format!(
            "<dependencies>{}</dependencies>",
            "<dependency/>".repeat(MAX_DEPENDENCIES + 1)
        ),
    ] {
        let error = parse(&content, &mut Vec::new()).unwrap_err().to_string();
        assert!(error.contains("budget exceeded"), "{error}");
    }
}

#[test]
fn pom_skips_incomplete_and_unrelated_records() {
    let content = "<project><dependency><groupId>outside</groupId><artifactId>dependencies</artifactId></dependency><dependencies><dependency/><dependency><groupId> </groupId><artifactId>missing</artifactId></dependency><dependency><groupId>only</groupId></dependency></dependencies></project>";
    let mut records = Vec::new();
    parse(content, &mut records).unwrap();
    assert!(records.is_empty());
}

#[test]
fn pom_custom_configuration_is_not_a_project_dependency() {
    let mut records = Vec::new();
    let dependency = "<dependencies><dependency><groupId>fake</groupId><artifactId>setting</artifactId></dependency></dependencies>";
    parse(&format!("<project><properties>{dependency}</properties><build><plugins><plugin><configuration>{dependency}</configuration></plugin></plugins></build><profiles><profile><dependencies><dependency><groupId>real</groupId><artifactId>profile</artifactId></dependency></dependencies></profile></profiles></project>"), &mut records).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].package_name, "real:profile");
}
