use super::{build_facts, model::PomDocument, model::resolve_effective_model_load};

#[test]
fn aggregator_module_build_facts_package_modules() {
    let models = resolve_effective_model_load(vec![document(
        "pom.xml",
        r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>root</artifactId>
  <version>1.0.0</version>
  <packaging>pom</packaging>
  <modules>
    <module>app</module>
  </modules>
</project>"#,
    )])
    .expect("effective POM should resolve")
    .models;

    let root = models.first().expect("model should exist");
    let facts = build_facts(root);
    assert!(
        facts.iter().any(|fact| {
            fact.kind == "module"
                && fact.name == "app"
                && fact.command.as_deref() == Some("mvn -pl app package")
        }),
        "aggregator module commands should build selected modules: {facts:?}"
    );
}

#[test]
fn execution_inherited_false_does_not_reach_children() {
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
            <id>parent-only</id>
            <inherited>false</inherited>
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
    assert!(
        !facts
            .iter()
            .any(|fact| fact.kind == "goal" && fact.name == "surefire:test"),
        "execution inherited=false goals must not reach child build facts: {facts:?}"
    );
}

#[test]
fn explicit_profile_ignores_active_by_default_management() {
    let models = resolve_effective_model_load(vec![document(
        "pom.xml",
        r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>service</artifactId>
  <version>1.0.0</version>
  <profiles>
    <profile>
      <id>default</id>
      <activation><activeByDefault>true</activeByDefault></activation>
      <dependencyManagement>
        <dependencies>
          <dependency>
            <groupId>org.example</groupId>
            <artifactId>api</artifactId>
            <version>1.0.0</version>
          </dependency>
        </dependencies>
      </dependencyManagement>
      <build>
        <pluginManagement>
          <plugins>
            <plugin>
              <artifactId>maven-failsafe-plugin</artifactId>
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
    </profile>
    <profile>
      <id>ci</id>
      <dependencies>
        <dependency>
          <groupId>org.example</groupId>
          <artifactId>api</artifactId>
        </dependency>
      </dependencies>
      <build>
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
        model.dependencies.iter().any(|dependency| {
            dependency.coordinate() == "org.example:api"
                && dependency.dependency_group() == "profile:ci:compile"
                && dependency.version.is_none()
        }),
        "explicit profiles should not inherit activeByDefault management: {:?}",
        model.dependencies
    );
    let facts = build_facts(model);
    assert!(
        !facts.iter().any(|fact| {
            fact.kind == "goal" && fact.name == "profile:ci:failsafe:integration-test"
        }),
        "explicit profile plugins must not inherit activeByDefault pluginManagement: {facts:?}"
    );
}

#[test]
fn child_does_not_inherit_parent_profile_dependencies() {
    let models = resolve_effective_model_load(vec![
        document(
            "pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>platform</artifactId>
  <version>1.0.0</version>
  <profiles>
    <profile>
      <id>release</id>
      <dependencies>
        <dependency>
          <groupId>org.example</groupId>
          <artifactId>profile-only</artifactId>
          <version>1.0.0</version>
        </dependency>
      </dependencies>
    </profile>
  </profiles>
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
        !app.dependencies
            .iter()
            .any(|dependency| dependency.coordinate() == "org.example:profile-only"),
        "children must not inherit parent profile-only dependency rows: {:?}",
        app.dependencies
    );
}

#[test]
fn profile_properties_interpolate_after_profile_merge() {
    let models = resolve_effective_model_load(vec![document(
        "pom.xml",
        r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>service</artifactId>
  <version>1.0.0</version>
  <properties>
    <revision>1.0.0</revision>
  </properties>
  <profiles>
    <profile>
      <id>new-deps</id>
      <properties>
        <revision>2.0.0</revision>
        <api.version>${revision}</api.version>
      </properties>
      <dependencies>
        <dependency>
          <groupId>org.example</groupId>
          <artifactId>api</artifactId>
          <version>${api.version}</version>
        </dependency>
      </dependencies>
    </profile>
  </profiles>
</project>"#,
    )])
    .expect("effective POM should resolve")
    .models;

    let model = models.first().expect("model should exist");
    assert!(
        model.dependencies.iter().any(|dependency| {
            dependency.coordinate() == "org.example:api"
                && dependency.dependency_group() == "profile:new-deps:compile"
                && dependency.version.as_deref() == Some("2.0.0")
        }),
        "profile properties should resolve against same-profile overrides: {:?}",
        model.dependencies
    );
}

#[test]
fn explicit_profile_reresolves_base_dependencies() {
    let models = resolve_effective_model_load(vec![document(
        "pom.xml",
        r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>service</artifactId>
  <version>1.0.0</version>
  <properties>
    <api.version>1.0.0</api.version>
  </properties>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>org.example</groupId>
        <artifactId>managed-api</artifactId>
        <version>1.0.0</version>
      </dependency>
    </dependencies>
  </dependencyManagement>
  <dependencies>
    <dependency>
      <groupId>org.example</groupId>
      <artifactId>api</artifactId>
      <version>${api.version}</version>
    </dependency>
    <dependency>
      <groupId>org.example</groupId>
      <artifactId>managed-api</artifactId>
    </dependency>
  </dependencies>
  <profiles>
    <profile>
      <id>ci</id>
      <properties>
        <api.version>2.0.0</api.version>
      </properties>
      <dependencyManagement>
        <dependencies>
          <dependency>
            <groupId>org.example</groupId>
            <artifactId>managed-api</artifactId>
            <version>2.0.0</version>
          </dependency>
        </dependencies>
      </dependencyManagement>
    </profile>
  </profiles>
</project>"#,
    )])
    .expect("effective POM should resolve")
    .models;

    let model = models.first().expect("model should exist");
    for artifact in ["api", "managed-api"] {
        assert!(
            model.dependencies.iter().any(|dependency| {
                dependency.coordinate() == format!("org.example:{artifact}")
                    && dependency.dependency_group() == "compile"
                    && dependency.version.as_deref() == Some("1.0.0")
            }),
            "default dependency should keep base version for {artifact}: {:?}",
            model.dependencies
        );
        assert!(
            model.dependencies.iter().any(|dependency| {
                dependency.coordinate() == format!("org.example:{artifact}")
                    && dependency.dependency_group() == "profile:ci:compile"
                    && dependency.version.as_deref() == Some("2.0.0")
            }),
            "explicit profile should re-resolve base dependency for {artifact}: {:?}",
            model.dependencies
        );
    }
}

#[test]
fn active_default_profile_properties_resolve_project_coordinates() {
    let models = resolve_effective_model_load(vec![
        document(
            "pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>platform</artifactId>
  <version>${revision}</version>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>org.example</groupId>
        <artifactId>api</artifactId>
        <version>1.2.3</version>
      </dependency>
    </dependencies>
  </dependencyManagement>
  <profiles>
    <profile>
      <id>default</id>
      <activation><activeByDefault>true</activeByDefault></activation>
      <properties>
        <revision>2.0.0</revision>
      </properties>
    </profile>
  </profiles>
</project>"#,
        ),
        document(
            "app/pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.acme</groupId>
    <artifactId>platform</artifactId>
    <version>2.0.0</version>
  </parent>
  <artifactId>app</artifactId>
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

    let platform = model_at(&models, "pom.xml");
    assert_eq!(platform.coordinate, "com.acme:platform:2.0.0");
    let app = model_at(&models, "app/pom.xml");
    assert!(
        app.dependencies.iter().any(|dependency| {
            dependency.coordinate() == "org.example:api"
                && dependency.version.as_deref() == Some("1.2.3")
        }),
        "child should match active-profile parent coordinates: {:?}",
        app.dependencies
    );
}

#[test]
fn explicit_profile_reresolves_top_level_plugins() {
    let models = resolve_effective_model_load(vec![document(
        "pom.xml",
        r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>service</artifactId>
  <version>1.0.0</version>
  <build>
    <plugins>
      <plugin>
        <artifactId>maven-surefire-plugin</artifactId>
      </plugin>
    </plugins>
  </build>
  <profiles>
    <profile>
      <id>ci</id>
      <build>
        <pluginManagement>
          <plugins>
            <plugin>
              <artifactId>maven-surefire-plugin</artifactId>
              <version>3.2.5</version>
              <executions>
                <execution>
                  <id>unit</id>
                  <phase>test</phase>
                  <goals><goal>test</goal></goals>
                </execution>
              </executions>
            </plugin>
          </plugins>
        </pluginManagement>
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
            fact.kind == "plugin"
                && fact.name == "profile:ci:org.apache.maven.plugins:maven-surefire-plugin"
                && fact.output_hint.as_deref() == Some("3.2.5")
        }),
        "profile pluginManagement should version top-level plugin variants: {facts:?}"
    );
    assert!(
        facts
            .iter()
            .any(|fact| fact.kind == "goal" && fact.name == "profile:ci:surefire:test"),
        "profile pluginManagement should add managed goals to top-level plugin variants: {facts:?}"
    );
}

#[test]
fn child_plugin_management_applies_to_inherited_plugins() {
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
    <pluginManagement>
      <plugins>
        <plugin>
          <artifactId>maven-surefire-plugin</artifactId>
          <version>3.2.5</version>
          <executions>
            <execution>
              <id>unit</id>
              <phase>test</phase>
              <goals><goal>test</goal></goals>
            </execution>
          </executions>
        </plugin>
      </plugins>
    </pluginManagement>
  </build>
</project>"#,
        ),
    ])
    .expect("effective POMs should resolve")
    .models;

    let app = model_at(&models, "app/pom.xml");
    let facts = build_facts(app);
    assert!(
        facts.iter().any(|fact| {
            fact.kind == "plugin"
                && fact.name == "org.apache.maven.plugins:maven-surefire-plugin"
                && fact.output_hint.as_deref() == Some("3.2.5")
        }),
        "child pluginManagement should version inherited plugins: {facts:?}"
    );
    assert!(
        facts
            .iter()
            .any(|fact| fact.kind == "goal" && fact.name == "surefire:test"),
        "child pluginManagement should add managed goals to inherited plugins: {facts:?}"
    );
}

#[test]
fn child_does_not_inherit_parent_profile_plugins() {
    let models = resolve_effective_model_load(vec![
        document(
            "pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>platform</artifactId>
  <version>1.0.0</version>
  <profiles>
    <profile>
      <id>release</id>
      <build>
        <plugins>
          <plugin>
            <artifactId>maven-failsafe-plugin</artifactId>
            <executions>
              <execution>
                <id>it</id>
                <phase>verify</phase>
                <goals><goal>integration-test</goal></goals>
              </execution>
            </executions>
          </plugin>
        </plugins>
      </build>
    </profile>
  </profiles>
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
    assert!(
        !facts
            .iter()
            .any(|fact| fact.kind == "goal" && fact.name.contains("failsafe")),
        "children must not inherit parent profile-only plugin facts: {facts:?}"
    );
}

#[test]
fn default_id_plugin_executions_merge() {
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
          <artifactId>maven-surefire-plugin</artifactId>
          <executions>
            <execution>
              <goals><goal>test</goal></goals>
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
        <artifactId>maven-surefire-plugin</artifactId>
        <executions>
          <execution>
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
        .find(|plugin| plugin.coordinate == "org.apache.maven.plugins:maven-surefire-plugin")
        .expect("surefire plugin should resolve");
    assert_eq!(
        surefire.executions.len(),
        1,
        "default-id executions should merge instead of duplicating"
    );
    assert_eq!(surefire.executions[0].phase.as_deref(), Some("verify"));
    assert!(
        surefire.executions[0]
            .goals
            .iter()
            .any(|goal| goal.value == "test"),
        "merged execution should retain managed goals: {:?}",
        surefire.executions
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
