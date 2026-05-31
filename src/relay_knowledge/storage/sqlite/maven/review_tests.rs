use super::{
    build_facts, dependency_records,
    model::{PomDocument, resolve_effective_model_load},
};

#[test]
fn direct_dependency_management_overrides_imported_bom() {
    let models = resolve_effective_model_load(vec![
        document(
            "bom/pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>platform-bom</artifactId>
  <version>1.0.0</version>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>org.slf4j</groupId>
        <artifactId>slf4j-api</artifactId>
        <version>2.0.9</version>
      </dependency>
    </dependencies>
  </dependencyManagement>
</project>"#,
        ),
        document(
            "app/pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>app</artifactId>
  <version>1.0.0</version>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>com.acme</groupId>
        <artifactId>platform-bom</artifactId>
        <version>1.0.0</version>
        <type>pom</type>
        <scope>import</scope>
      </dependency>
      <dependency>
        <groupId>org.slf4j</groupId>
        <artifactId>slf4j-api</artifactId>
        <version>2.1.0</version>
      </dependency>
    </dependencies>
  </dependencyManagement>
  <dependencies>
    <dependency>
      <groupId>org.slf4j</groupId>
      <artifactId>slf4j-api</artifactId>
    </dependency>
  </dependencies>
</project>"#,
        ),
    ])
    .expect("effective POMs should resolve")
    .models;

    let app = model_at(&models, "app/pom.xml");
    assert!(
        app.dependencies.iter().any(|dependency| {
            dependency.coordinate() == "org.slf4j:slf4j-api"
                && dependency.version.as_deref() == Some("2.1.0")
        }),
        "direct dependencyManagement should override imported BOM rows: {:?}",
        app.dependencies
    );
}

#[test]
fn same_id_plugin_execution_merges_managed_goals() {
    let models = resolve_effective_model_load(vec![
        document(
            "pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>platform</artifactId>
  <version>1.0.0</version>
  <build>
    <pluginManagement>
      <plugins>
        <plugin>
          <groupId>org.jacoco</groupId>
          <artifactId>jacoco-maven-plugin</artifactId>
          <version>0.8.12</version>
          <executions>
            <execution>
              <id>coverage</id>
              <goals><goal>prepare-agent</goal></goals>
            </execution>
          </executions>
        </plugin>
      </plugins>
    </pluginManagement>
  </build>
</project>"#,
        ),
        document(
            "app/pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.acme</groupId>
    <artifactId>platform</artifactId>
    <version>1.0.0</version>
  </parent>
  <artifactId>app</artifactId>
  <build>
    <plugins>
      <plugin>
        <groupId>org.jacoco</groupId>
        <artifactId>jacoco-maven-plugin</artifactId>
        <executions>
          <execution>
            <id>coverage</id>
            <phase>initialize</phase>
          </execution>
        </executions>
      </plugin>
    </plugins>
  </build>
</project>"#,
        ),
    ])
    .expect("effective POMs should resolve")
    .models;

    let app = model_at(&models, "app/pom.xml");
    let facts = build_facts(app);
    assert!(
        facts
            .iter()
            .any(|fact| fact.kind == "goal" && fact.name == "jacoco:prepare-agent"),
        "same-id child execution should retain managed goals: {facts:?}"
    );
}

#[test]
fn first_imported_bom_dependency_management_entry_wins() {
    let models = resolve_effective_model_load(vec![
        document(
            "bom-a/pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>bom-a</artifactId>
  <version>1.0.0</version>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>org.example</groupId>
        <artifactId>api</artifactId>
        <version>1.0.0</version>
      </dependency>
    </dependencies>
  </dependencyManagement>
</project>"#,
        ),
        document(
            "bom-b/pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>bom-b</artifactId>
  <version>1.0.0</version>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>org.example</groupId>
        <artifactId>api</artifactId>
        <version>2.0.0</version>
      </dependency>
    </dependencies>
  </dependencyManagement>
</project>"#,
        ),
        document(
            "app/pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>app</artifactId>
  <version>1.0.0</version>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>com.acme</groupId>
        <artifactId>bom-a</artifactId>
        <version>1.0.0</version>
        <type>pom</type>
        <scope>import</scope>
      </dependency>
      <dependency>
        <groupId>com.acme</groupId>
        <artifactId>bom-b</artifactId>
        <version>1.0.0</version>
        <type>pom</type>
        <scope>import</scope>
      </dependency>
    </dependencies>
  </dependencyManagement>
  <dependencies>
    <dependency>
      <groupId>org.example</groupId>
      <artifactId>api</artifactId>
    </dependency>
  </dependencies>
</project>"#,
        ),
    ])
    .expect("effective POMs should resolve")
    .models;

    let app = model_at(&models, "app/pom.xml");
    assert!(
        app.dependencies.iter().any(|dependency| {
            dependency.coordinate() == "org.example:api"
                && dependency.version.as_deref() == Some("1.0.0")
        }),
        "first imported BOM should keep management precedence: {:?}",
        app.dependencies
    );
}

#[test]
fn parent_plugins_respect_inherited_false() {
    let models = resolve_effective_model_load(vec![
        document(
            "pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>platform</artifactId>
  <version>1.0.0</version>
  <build>
    <plugins>
      <plugin>
        <artifactId>maven-deploy-plugin</artifactId>
        <inherited>false</inherited>
        <executions>
          <execution>
            <id>release-deploy</id>
            <goals><goal>deploy</goal></goals>
          </execution>
        </executions>
      </plugin>
    </plugins>
  </build>
</project>"#,
        ),
        document(
            "app/pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.acme</groupId>
    <artifactId>platform</artifactId>
    <version>1.0.0</version>
  </parent>
  <artifactId>app</artifactId>
</project>"#,
        ),
    ])
    .expect("effective POMs should resolve")
    .models;

    let app = model_at(&models, "app/pom.xml");
    assert!(
        !app.plugins
            .iter()
            .any(|plugin| plugin.coordinate == "org.apache.maven.plugins:maven-deploy-plugin"),
        "inherited=false parent plugins should not reach children: {:?}",
        app.plugins
    );
}

#[test]
fn profile_plugin_management_applies_to_profile_plugins() {
    let models = resolve_effective_model_load(vec![document(
        "pom.xml",
        r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>service</artifactId>
  <version>1.0.0</version>
  <profiles>
    <profile>
      <id>it</id>
      <build>
        <pluginManagement>
          <plugins>
            <plugin>
              <artifactId>maven-failsafe-plugin</artifactId>
              <version>3.2.5</version>
              <executions>
                <execution>
                  <id>integration-test</id>
                  <phase>integration-test</phase>
                  <goals><goal>integration-test</goal></goals>
                </execution>
              </executions>
            </plugin>
          </plugins>
        </pluginManagement>
        <plugins>
          <plugin>
            <artifactId>maven-failsafe-plugin</artifactId>
          </plugin>
        </plugins>
      </build>
    </profile>
  </profiles>
</project>"#,
    )])
    .expect("effective POM should resolve")
    .models;

    let model = models.first().expect("model should exist");
    assert!(
        model.plugins.iter().any(|plugin| {
            plugin.coordinate == "org.apache.maven.plugins:maven-failsafe-plugin"
                && plugin.version.as_deref() == Some("3.2.5")
                && plugin
                    .executions
                    .iter()
                    .any(|execution| execution.phase.as_deref() == Some("integration-test"))
        }),
        "profile plugins should inherit profile pluginManagement: {:?}",
        model.plugins
    );
}

#[test]
fn child_dependency_declaration_overrides_parent_dependency() {
    let models = resolve_effective_model_load(vec![
        document(
            "pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>platform</artifactId>
  <version>1.0.0</version>
  <dependencies>
    <dependency>
      <groupId>org.example</groupId>
      <artifactId>api</artifactId>
      <version>1.0.0</version>
    </dependency>
  </dependencies>
</project>"#,
        ),
        document(
            "app/pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.acme</groupId>
    <artifactId>platform</artifactId>
    <version>1.0.0</version>
  </parent>
  <artifactId>app</artifactId>
  <dependencies>
    <dependency>
      <groupId>org.example</groupId>
      <artifactId>api</artifactId>
      <version>2.0.0</version>
    </dependency>
  </dependencies>
</project>"#,
        ),
    ])
    .expect("effective POMs should resolve")
    .models;

    let app = model_at(&models, "app/pom.xml");
    let versions = app
        .dependencies
        .iter()
        .filter(|dependency| dependency.coordinate() == "org.example:api")
        .filter_map(|dependency| dependency.version.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(
        versions,
        vec!["2.0.0"],
        "child dependency declaration should replace inherited parent rows"
    );
}

#[test]
fn explicit_parent_relative_path_cannot_escape_indexed_root() {
    let models = resolve_effective_model_load(vec![
        document(
            "pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.external</groupId>
  <artifactId>parent</artifactId>
  <version>1.0.0</version>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>org.example</groupId>
        <artifactId>leak</artifactId>
        <version>9.9.9</version>
      </dependency>
    </dependencies>
  </dependencyManagement>
</project>"#,
        ),
        document(
            "app/pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.external</groupId>
    <artifactId>parent</artifactId>
    <version>1.0.0</version>
    <relativePath>../../pom.xml</relativePath>
  </parent>
  <artifactId>app</artifactId>
  <dependencies>
    <dependency>
      <groupId>org.example</groupId>
      <artifactId>leak</artifactId>
    </dependency>
  </dependencies>
</project>"#,
        ),
    ])
    .expect("effective POMs should resolve")
    .models;

    let app = model_at(&models, "app/pom.xml");
    assert!(
        app.dependencies.iter().any(|dependency| {
            dependency.coordinate() == "org.example:leak" && dependency.version.is_none()
        }),
        "escaped relativePath must not fold back to an indexed root POM: {:?}",
        app.dependencies
    );
}

#[test]
fn child_build_plugin_declaration_merges_inherited_parent_plugin() {
    let models = resolve_effective_model_load(vec![
        document(
            "pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>platform</artifactId>
  <version>1.0.0</version>
  <build>
    <plugins>
      <plugin>
        <artifactId>maven-surefire-plugin</artifactId>
        <version>3.2.5</version>
        <executions>
          <execution>
            <id>default-test</id>
            <phase>test</phase>
            <goals><goal>test</goal></goals>
          </execution>
        </executions>
      </plugin>
    </plugins>
  </build>
</project>"#,
        ),
        document(
            "app/pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.acme</groupId>
    <artifactId>platform</artifactId>
    <version>1.0.0</version>
  </parent>
  <artifactId>app</artifactId>
  <build>
    <plugins>
      <plugin>
        <artifactId>maven-surefire-plugin</artifactId>
        <executions>
          <execution>
            <id>default-test</id>
            <phase>verify</phase>
          </execution>
        </executions>
      </plugin>
    </plugins>
  </build>
</project>"#,
        ),
    ])
    .expect("effective POMs should resolve")
    .models;

    let app = model_at(&models, "app/pom.xml");
    let surefire = app
        .plugins
        .iter()
        .filter(|plugin| plugin.coordinate == "org.apache.maven.plugins:maven-surefire-plugin")
        .collect::<Vec<_>>();
    assert_eq!(surefire.len(), 1, "parent and child plugins should merge");
    let execution = surefire[0]
        .executions
        .iter()
        .find(|execution| execution.id.as_deref() == Some("default-test"))
        .expect("merged execution should exist");
    assert_eq!(surefire[0].version.as_deref(), Some("3.2.5"));
    assert_eq!(execution.phase.as_deref(), Some("verify"));
    assert!(
        execution.goals.iter().any(|goal| goal.value == "test"),
        "child execution should retain inherited parent goals: {:?}",
        execution
    );
}

#[test]
fn profile_build_plugin_facts_are_profile_scoped() {
    let models = resolve_effective_model_load(vec![document(
        "pom.xml",
        r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>service</artifactId>
  <version>1.0.0</version>
  <profiles>
    <profile>
      <id>it</id>
      <build>
        <plugins>
          <plugin>
            <artifactId>maven-failsafe-plugin</artifactId>
            <executions>
              <execution>
                <id>integration-test</id>
                <goals><goal>integration-test</goal></goals>
              </execution>
            </executions>
          </plugin>
        </plugins>
      </build>
    </profile>
  </profiles>
</project>"#,
    )])
    .expect("effective POM should resolve")
    .models;

    let model = models.first().expect("model should exist");
    let facts = build_facts(model);
    assert!(
        facts.iter().any(|fact| {
            fact.kind == "goal"
                && fact.name == "profile:it:failsafe:integration-test"
                && fact.command.as_deref() == Some("mvn -Pit failsafe:integration-test")
        }),
        "profile-only plugin goals should be profile-qualified: {facts:?}"
    );
    assert!(
        !facts
            .iter()
            .any(|fact| fact.kind == "goal" && fact.name == "failsafe:integration-test"),
        "profile-only plugin goals must not appear as default build facts: {facts:?}"
    );
}

#[test]
fn parent_version_aliases_resolve_child_imported_bom() {
    let models = resolve_effective_model_load(vec![
        document(
            "bom/pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>platform-bom</artifactId>
  <version>1.0.0</version>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>org.example</groupId>
        <artifactId>api</artifactId>
        <version>1.2.3</version>
      </dependency>
    </dependencies>
  </dependencyManagement>
</project>"#,
        ),
        document(
            "parent/pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>parent</artifactId>
  <version>1.0.0</version>
</project>"#,
        ),
        document(
            "app/pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.acme</groupId>
    <artifactId>parent</artifactId>
    <version>1.0.0</version>
    <relativePath>../parent/pom.xml</relativePath>
  </parent>
  <artifactId>app</artifactId>
  <version>2.0.0</version>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>${project.parent.groupId}</groupId>
        <artifactId>platform-bom</artifactId>
        <version>${project.parent.version}</version>
        <type>pom</type>
        <scope>import</scope>
      </dependency>
    </dependencies>
  </dependencyManagement>
  <dependencies>
    <dependency>
      <groupId>org.example</groupId>
      <artifactId>api</artifactId>
    </dependency>
  </dependencies>
</project>"#,
        ),
    ])
    .expect("effective POMs should resolve")
    .models;

    let app = model_at(&models, "app/pom.xml");
    assert!(
        app.dependencies.iter().any(|dependency| {
            dependency.coordinate() == "org.example:api"
                && dependency.version.as_deref() == Some("1.2.3")
        }),
        "project.parent aliases should resolve imported BOM coordinates: {:?}",
        app.dependencies
    );
}

#[test]
fn plugin_management_respects_inherited_false_in_children() {
    let models = resolve_effective_model_load(vec![
        document(
            "pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>platform</artifactId>
  <version>1.0.0</version>
  <build>
    <pluginManagement>
      <plugins>
        <plugin>
          <artifactId>maven-failsafe-plugin</artifactId>
          <version>3.2.5</version>
          <inherited>false</inherited>
          <executions>
            <execution>
              <id>it</id>
              <goals><goal>integration-test</goal></goals>
            </execution>
          </executions>
        </plugin>
      </plugins>
    </pluginManagement>
  </build>
</project>"#,
        ),
        document(
            "app/pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.acme</groupId>
    <artifactId>platform</artifactId>
    <version>1.0.0</version>
  </parent>
  <artifactId>app</artifactId>
  <build>
    <plugins>
      <plugin>
        <artifactId>maven-failsafe-plugin</artifactId>
      </plugin>
    </plugins>
  </build>
</project>"#,
        ),
    ])
    .expect("effective POMs should resolve")
    .models;

    let app = model_at(&models, "app/pom.xml");
    let failsafe = app
        .plugins
        .iter()
        .find(|plugin| plugin.coordinate == "org.apache.maven.plugins:maven-failsafe-plugin")
        .expect("child plugin declaration should remain");
    assert!(
        failsafe.executions.is_empty() && failsafe.version.is_none(),
        "non-inherited pluginManagement should not configure child plugins: {:?}",
        failsafe
    );
}

#[test]
fn inherited_dependency_records_keep_parent_evidence_path() {
    let models = resolve_effective_model_load(vec![
        document(
            "pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>platform</artifactId>
  <version>1.0.0</version>
  <dependencies>
    <dependency>
      <groupId>org.example</groupId>
      <artifactId>parent-only</artifactId>
      <version>1.0.0</version>
    </dependency>
  </dependencies>
</project>"#,
        ),
        document(
            "app/pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.acme</groupId>
    <artifactId>platform</artifactId>
    <version>1.0.0</version>
  </parent>
  <artifactId>app</artifactId>
</project>"#,
        ),
    ])
    .expect("effective POMs should resolve")
    .models;

    let records = dependency_records(&models);
    assert!(
        records
            .iter()
            .filter(|record| record.package_name == "org.example:parent-only")
            .all(|record| record.path == "pom.xml"),
        "inherited dependency evidence must stay on the parent POM: {records:?}"
    );
}

#[test]
fn repeated_dependency_tags_keep_node_specific_line_numbers() {
    let models = resolve_effective_model_load(vec![document(
        "pom.xml",
        r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>service</artifactId>
  <version>1.0.0</version>
  <dependencies>
    <dependency>
      <groupId>org.example</groupId>
      <artifactId>one</artifactId>
    </dependency>
    <dependency>
      <groupId>org.example</groupId>
      <artifactId>two</artifactId>
    </dependency>
  </dependencies>
</project>"#,
    )])
    .expect("effective POM should resolve")
    .models;

    let records = dependency_records(&models);
    let first_line = records
        .iter()
        .find(|record| record.package_name == "org.example:one")
        .map(|record| record.line_range.start)
        .expect("first dependency should be recorded");
    let second_line = records
        .iter()
        .find(|record| record.package_name == "org.example:two")
        .map(|record| record.line_range.start)
        .expect("second dependency should be recorded");
    assert!(
        second_line > first_line,
        "repeated groupId tags should keep per-node evidence lines: {records:?}"
    );
}

#[test]
fn active_by_default_profile_merges_into_default_build() {
    let models = resolve_effective_model_load(vec![document(
        "pom.xml",
        r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>service</artifactId>
  <version>1.0.0</version>
  <dependencies>
    <dependency>
      <groupId>org.example</groupId>
      <artifactId>core</artifactId>
      <version>${api.version}</version>
    </dependency>
  </dependencies>
  <profiles>
    <profile>
      <id>default</id>
      <activation>
        <activeByDefault>true</activeByDefault>
      </activation>
      <properties>
        <api.version>1.0.0</api.version>
      </properties>
      <dependencies>
        <dependency>
          <groupId>org.example</groupId>
          <artifactId>api</artifactId>
          <version>1.0.0</version>
        </dependency>
      </dependencies>
      <build>
        <plugins>
          <plugin>
            <artifactId>maven-failsafe-plugin</artifactId>
            <executions>
              <execution>
                <id>integration-test</id>
                <goals><goal>integration-test</goal></goals>
              </execution>
            </executions>
          </plugin>
        </plugins>
      </build>
    </profile>
  </profiles>
</project>"#,
    )])
    .expect("effective POM should resolve")
    .models;

    let model = models.first().expect("model should exist");
    assert!(
        model.dependencies.iter().any(|dependency| {
            dependency.coordinate() == "org.example:core"
                && dependency.version.as_deref() == Some("1.0.0")
        }),
        "activeByDefault properties should resolve default dependencies: {:?}",
        model.dependencies
    );
    let records = dependency_records(&models);
    assert!(
        records.iter().any(|record| {
            record.package_name == "org.example:api" && record.dependency_group == "compile"
        }),
        "activeByDefault dependencies should be default-scope records: {records:?}"
    );
    assert!(
        !records.iter().any(|record| {
            record.package_name == "org.example:api"
                && record.dependency_group.starts_with("profile:default")
        }),
        "activeByDefault dependencies must not be profile-scoped: {records:?}"
    );

    let facts = build_facts(model);
    assert!(
        facts.iter().any(|fact| {
            fact.kind == "goal"
                && fact.name == "failsafe:integration-test"
                && fact.command.as_deref() == Some("mvn failsafe:integration-test")
        }),
        "activeByDefault profile goals should be default build facts: {facts:?}"
    );
    assert!(
        !facts.iter().any(|fact| {
            fact.kind == "goal" && fact.name == "profile:default:failsafe:integration-test"
        }),
        "activeByDefault profile goals must not require -Pdefault: {facts:?}"
    );
}

#[test]
fn inherited_plugin_build_facts_keep_parent_evidence_path() {
    let models = resolve_effective_model_load(vec![
        document(
            "pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>platform</artifactId>
  <version>1.0.0</version>
  <build>
    <plugins>
      <plugin>
        <artifactId>maven-surefire-plugin</artifactId>
        <executions>
          <execution>
            <id>default-test</id>
            <goals><goal>test</goal></goals>
          </execution>
        </executions>
      </plugin>
    </plugins>
  </build>
</project>"#,
        ),
        document(
            "app/pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.acme</groupId>
    <artifactId>platform</artifactId>
    <version>1.0.0</version>
  </parent>
  <artifactId>app</artifactId>
</project>"#,
        ),
    ])
    .expect("effective POMs should resolve")
    .models;

    let app = model_at(&models, "app/pom.xml");
    let facts = build_facts(app);
    let surefire_goals = facts
        .iter()
        .filter(|fact| fact.kind == "goal" && fact.name == "surefire:test")
        .collect::<Vec<_>>();
    assert!(
        !surefire_goals.is_empty(),
        "inherited plugin goals should produce child build facts: {facts:?}"
    );
    assert!(
        surefire_goals.iter().all(|fact| fact.path == "pom.xml"),
        "inherited plugin goal evidence must stay on the parent POM: {surefire_goals:?}"
    );
}

fn model_at<'a>(
    models: &'a [super::model::EffectivePom],
    path: &str,
) -> &'a super::model::EffectivePom {
    models
        .iter()
        .find(|model| model.document.path == path)
        .expect("model should exist")
}

fn document(path: &str, content: &str) -> PomDocument {
    PomDocument {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        file_id: format!("{path}-file"),
        path: path.to_owned(),
        content: content.to_owned(),
        byte_start: 0,
        byte_end: content.len() as u64,
    }
}
