use super::{
    build_facts, build_target_inputs, dependency_records,
    model::{PomDocument, resolve_effective_model_load},
    refresh_effective_dependencies, refresh_effective_dependencies_with_language_filters,
};
use crate::domain::GraphVersion;
use rusqlite::{Connection, params};

#[test]
fn effective_model_resolves_parent_properties_profiles_plugins_and_boms() {
    let models = resolve_effective_model_load(vec![
        document(
            "pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>platform</artifactId>
  <version>1.2.3</version>
  <packaging>pom</packaging>
  <properties>
    <slf4j.version>2.0.9</slf4j.version>
    <junit.version>5.10.1</junit.version>
  </properties>
  <modules>
    <module>app</module>
  </modules>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>org.slf4j</groupId>
        <artifactId>slf4j-api</artifactId>
        <version>${slf4j.version}</version>
      </dependency>
      <dependency>
        <groupId>org.junit</groupId>
        <artifactId>junit-bom</artifactId>
        <version>${junit.version}</version>
        <type>pom</type>
        <scope>import</scope>
      </dependency>
    </dependencies>
  </dependencyManagement>
  <build>
    <pluginManagement>
      <plugins>
        <plugin>
          <artifactId>maven-compiler-plugin</artifactId>
          <version>3.12.1</version>
        </plugin>
        <plugin>
          <artifactId>maven-surefire-plugin</artifactId>
          <version>3.2.5</version>
          <executions>
            <execution>
              <id>managed-test</id>
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
        document(
            "app/pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.acme</groupId>
    <artifactId>platform</artifactId>
    <version>1.2.3</version>
  </parent>
  <artifactId>app</artifactId>
  <packaging>war</packaging>
  <dependencies>
    <dependency>
      <groupId>org.slf4j</groupId>
      <artifactId>slf4j-api</artifactId>
      <scope>runtime</scope>
    </dependency>
  </dependencies>
  <profiles>
    <profile>
      <id>it</id>
      <dependencies>
        <dependency>
          <groupId>org.testcontainers</groupId>
          <artifactId>junit-jupiter</artifactId>
          <version>1.19.7</version>
        </dependency>
      </dependencies>
    </profile>
  </profiles>
  <build>
    <plugins>
      <plugin>
        <artifactId>maven-compiler-plugin</artifactId>
        <executions>
          <execution>
            <id>default-compile</id>
            <phase>compile</phase>
            <goals><goal>compile</goal></goals>
          </execution>
        </executions>
      </plugin>
      <plugin>
        <artifactId>maven-surefire-plugin</artifactId>
      </plugin>
    </plugins>
  </build>
</project>"#,
        ),
    ])
    .expect("effective POMs should resolve")
    .models;

    let app = models
        .iter()
        .find(|model| model.document.path == "app/pom.xml")
        .expect("child model should exist");
    assert_eq!(app.coordinate, "com.acme:app:1.2.3");
    assert_eq!(app.packaging.as_deref(), Some("war"));
    assert!(
        app.dependencies.iter().any(|dependency| {
            dependency.coordinate() == "org.slf4j:slf4j-api"
                && dependency.version.as_deref() == Some("2.0.9")
                && dependency.dependency_group() == "runtime"
        }),
        "child dependency should inherit dependencyManagement version: {:?}",
        app.dependencies
    );
    assert!(
        app.dependencies
            .iter()
            .any(|dependency| dependency.dependency_group() == "profile:it:compile"),
        "profile dependencies should be retained: {:?}",
        app.dependencies
    );
    assert!(
        app.plugins.iter().any(|plugin| {
            plugin.coordinate == "org.apache.maven.plugins:maven-compiler-plugin"
                && plugin.version.as_deref() == Some("3.12.1")
                && plugin
                    .executions
                    .iter()
                    .any(|execution| execution.phase.as_deref() == Some("compile"))
        }),
        "plugin execution should inherit pluginManagement version: {:?}",
        app.plugins
    );
    assert!(
        app.plugins.iter().any(|plugin| {
            plugin.coordinate == "org.apache.maven.plugins:maven-surefire-plugin"
                && plugin.version.as_deref() == Some("3.2.5")
                && plugin
                    .executions
                    .iter()
                    .any(|execution| execution.phase.as_deref() == Some("test"))
        }),
        "declared plugin should inherit pluginManagement executions: {:?}",
        app.plugins
    );

    let records = dependency_records(&models);
    assert!(
        records
            .iter()
            .any(|record| record.package_name == "org.junit:junit-bom"
                && record.dependency_group == "bom"
                && record.requirement.as_deref() == Some("5.10.1")),
        "imported BOM should become a dependency fact: {records:?}"
    );

    let facts = build_facts(app);
    assert!(
        facts
            .iter()
            .any(|fact| fact.kind == "profile" && fact.name == "it"),
        "profiles should become Maven build facts: {facts:?}"
    );
    assert!(
        facts
            .iter()
            .any(|fact| fact.kind == "goal" && fact.name == "compiler:compile"),
        "plugin goals should become Maven build facts: {facts:?}"
    );
    assert!(
        facts
            .iter()
            .any(|fact| fact.kind == "goal" && fact.name == "surefire:test"),
        "managed plugin goals should become Maven build facts: {facts:?}"
    );
}

#[test]
fn empty_parent_relative_path_disables_local_parent_resolution() {
    let models = resolve_effective_model_load(vec![
        document(
            "pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>platform</artifactId>
  <version>1.2.3</version>
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
  <parent>
    <groupId>com.acme</groupId>
    <artifactId>platform</artifactId>
    <version>1.2.3</version>
    <relativePath/>
  </parent>
  <artifactId>app</artifactId>
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

    let app = models
        .iter()
        .find(|model| model.document.path == "app/pom.xml")
        .expect("child model should exist");
    assert!(
        app.dependencies.iter().any(|dependency| {
            dependency.coordinate() == "org.slf4j:slf4j-api" && dependency.version.is_none()
        }),
        "empty relativePath should not inherit local parent management: {:?}",
        app.dependencies
    );
}

#[test]
fn profile_dependency_management_only_applies_to_profile_dependencies() {
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
      <artifactId>feature-api</artifactId>
    </dependency>
  </dependencies>
  <profiles>
    <profile>
      <id>feature</id>
      <dependencyManagement>
        <dependencies>
          <dependency>
            <groupId>org.example</groupId>
            <artifactId>feature-api</artifactId>
            <version>9.9.9</version>
          </dependency>
        </dependencies>
      </dependencyManagement>
      <dependencies>
        <dependency>
          <groupId>org.example</groupId>
          <artifactId>feature-api</artifactId>
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
            dependency.coordinate() == "org.example:feature-api"
                && dependency.dependency_group() == "compile"
                && dependency.version.is_none()
        }),
        "default dependency should not inherit profile management: {:?}",
        model.dependencies
    );
    assert!(
        model.dependencies.iter().any(|dependency| {
            dependency.coordinate() == "org.example:feature-api"
                && dependency.dependency_group() == "profile:feature:compile"
                && dependency.version.as_deref() == Some("9.9.9")
        }),
        "profile dependency should inherit profile management: {:?}",
        model.dependencies
    );
}

#[test]
fn relative_parent_path_requires_matching_coordinates() {
    let models = resolve_effective_model_load(vec![
        document(
            "pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>wrong-parent</artifactId>
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
  <parent>
    <groupId>com.acme</groupId>
    <artifactId>external-parent</artifactId>
    <version>1.0.0</version>
    <relativePath>../pom.xml</relativePath>
  </parent>
  <artifactId>app</artifactId>
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

    let app = models
        .iter()
        .find(|model| model.document.path == "app/pom.xml")
        .expect("child model should exist");
    assert!(
        app.dependencies.iter().any(|dependency| {
            dependency.coordinate() == "org.slf4j:slf4j-api" && dependency.version.is_none()
        }),
        "mismatched relative parent must not contribute management: {:?}",
        app.dependencies
    );
}

#[test]
fn imported_bom_management_resolves_local_versions() {
    let models = resolve_effective_model_load(vec![
        document(
            "bom/pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>platform-bom</artifactId>
  <version>1.0.0</version>
  <packaging>pom</packaging>
  <properties><slf4j.version>2.0.9</slf4j.version></properties>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>org.slf4j</groupId>
        <artifactId>slf4j-api</artifactId>
        <version>${slf4j.version}</version>
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

    let app = models
        .iter()
        .find(|model| model.document.path == "app/pom.xml")
        .expect("app model should exist");
    assert!(
        app.dependencies.iter().any(|dependency| {
            dependency.coordinate() == "org.slf4j:slf4j-api"
                && dependency.version.as_deref() == Some("2.0.9")
        }),
        "imported local BOM should supply managed versions: {:?}",
        app.dependencies
    );
}

#[test]
fn project_coordinates_interpolate_properties_for_bom_matching() {
    let models = resolve_effective_model_load(vec![
        document(
            "bom/pom.xml",
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>platform-bom</artifactId>
  <version>${revision}</version>
  <packaging>pom</packaging>
  <properties><revision>2.0.0</revision></properties>
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
  <version>${revision}</version>
  <properties><revision>2.0.0</revision></properties>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>com.acme</groupId>
        <artifactId>platform-bom</artifactId>
        <version>${revision}</version>
        <type>pom</type>
        <scope>import</scope>
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

    let bom = models
        .iter()
        .find(|model| model.document.path == "bom/pom.xml")
        .expect("BOM model should exist");
    assert_eq!(bom.coordinate, "com.acme:platform-bom:2.0.0");

    let app = models
        .iter()
        .find(|model| model.document.path == "app/pom.xml")
        .expect("app model should exist");
    assert_eq!(app.coordinate, "com.acme:app:2.0.0");
    assert!(
        app.dependencies.iter().any(|dependency| {
            dependency.coordinate() == "org.slf4j:slf4j-api"
                && dependency.version.as_deref() == Some("2.0.9")
        }),
        "property-backed BOM coordinates should resolve local management: {:?}",
        app.dependencies
    );
}

#[test]
fn dependency_management_keys_include_type_and_classifier() {
    let models = resolve_effective_model_load(vec![document(
        "pom.xml",
        r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.acme</groupId>
  <artifactId>service</artifactId>
  <version>1.0.0</version>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>org.example</groupId>
        <artifactId>fixture</artifactId>
        <version>1.0.0</version>
      </dependency>
      <dependency>
        <groupId>org.example</groupId>
        <artifactId>fixture</artifactId>
        <version>2.0.0</version>
        <type>test-jar</type>
        <classifier>tests</classifier>
      </dependency>
    </dependencies>
  </dependencyManagement>
  <dependencies>
    <dependency>
      <groupId>org.example</groupId>
      <artifactId>fixture</artifactId>
    </dependency>
    <dependency>
      <groupId>org.example</groupId>
      <artifactId>fixture</artifactId>
      <type>test-jar</type>
      <classifier>tests</classifier>
    </dependency>
  </dependencies>
</project>"#,
    )])
    .expect("effective POM should resolve")
    .models;

    let model = models.first().expect("model should exist");
    let versions = model
        .dependencies
        .iter()
        .filter(|dependency| dependency.coordinate() == "org.example:fixture")
        .filter_map(|dependency| dependency.version.as_deref())
        .collect::<Vec<_>>();
    assert!(
        versions.contains(&"1.0.0") && versions.contains(&"2.0.0"),
        "type/classifier management rows should not overwrite jar management: {:?}",
        model.dependencies
    );
}

#[test]
fn refresh_deletes_stale_maven_dependencies_when_poms_disappear() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_refresh_schema(&connection);
    seed_existing_dependency(&connection);

    let transaction = connection.transaction().expect("transaction should start");
    refresh_effective_dependencies(&transaction, "scope").expect("refresh should clear stale rows");
    transaction.commit().expect("refresh should commit");

    assert_eq!(
        count_rows(
            &connection,
            "SELECT COUNT(*) FROM code_repository_dependencies"
        ),
        0
    );
    assert_eq!(
        count_rows(&connection, "SELECT COUNT(*) FROM code_repository_search"),
        0
    );
}

#[test]
fn refresh_preserves_existing_maven_dependencies_when_pom_is_malformed() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_refresh_schema(&connection);
    seed_existing_dependency(&connection);
    let malformed = "<project><modelVersion>4.0.0</modelVersion>";
    connection
        .execute(
            "
            INSERT INTO code_repository_chunks (
                repository_id, source_scope, chunk_id, file_id, path, content,
                byte_start, byte_end, line_start, line_end, symbol_snapshot_id
            )
            VALUES ('repo', 'scope', 'pom-chunk', 'pom-file', 'pom.xml',
                ?1, 0, ?2, 1, 1, NULL)
            ",
            params![malformed, malformed.len() as u64],
        )
        .expect("malformed pom chunk should seed");

    let transaction = connection.transaction().expect("transaction should start");
    refresh_effective_dependencies(&transaction, "scope")
        .expect("refresh should preserve stale rows until POM XML is valid");
    transaction.commit().expect("refresh should commit");

    assert_eq!(
        count_rows(
            &connection,
            "SELECT COUNT(*) FROM code_repository_dependencies"
        ),
        1
    );
    assert_eq!(
        count_rows(&connection, "SELECT COUNT(*) FROM code_repository_search"),
        1
    );
}

#[test]
fn refresh_preserves_existing_maven_dependencies_when_pom_chunk_is_truncated() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_refresh_schema(&connection);
    seed_existing_dependency(&connection);
    connection
        .execute(
            "
            INSERT INTO code_repository_chunks (
                repository_id, source_scope, chunk_id, file_id, path, content,
                byte_start, byte_end, line_start, line_end, symbol_snapshot_id
            )
            VALUES ('repo', 'scope', 'pom-chunk', 'pom-file', 'pom.xml',
                '<project><modelVersion>4.0.0</modelVersion>',
                0, 9000, 1, 300, NULL)
            ",
            [],
        )
        .expect("truncated pom chunk should seed");

    let transaction = connection.transaction().expect("transaction should start");
    refresh_effective_dependencies(&transaction, "scope")
        .expect("refresh should preserve stale rows until full POM is available");
    transaction.commit().expect("refresh should commit");

    assert_eq!(
        count_rows(
            &connection,
            "SELECT COUNT(*) FROM code_repository_dependencies"
        ),
        1
    );
    assert_eq!(
        count_rows(&connection, "SELECT COUNT(*) FROM code_repository_search"),
        1
    );
}

#[test]
fn build_targets_honor_scope_language_filters() {
    let connection = Connection::open_in_memory().expect("sqlite should open");
    create_refresh_schema(&connection);
    connection
        .execute(
            "
            INSERT INTO code_repository_chunks (
                repository_id, source_scope, chunk_id, file_id, path, content,
                byte_start, byte_end, line_start, line_end, symbol_snapshot_id
            )
            VALUES ('repo', 'scope', 'pom-chunk', 'pom-file', 'pom.xml',
                '<project><modelVersion>4.0.0</modelVersion><groupId>com.acme</groupId><artifactId>service</artifactId><version>1.0.0</version></project>',
                0, 139, 1, 1, NULL)
            ",
            [],
        )
        .expect("pom chunk should seed");

    let targets = build_target_inputs(&connection, "scope", GraphVersion::new(1))
        .expect("targets should build");

    assert!(targets.iter().any(|target| target.language_id == "kotlin"));
    assert!(!targets.iter().any(|target| target.language_id == "java"));
    assert!(!targets.iter().any(|target| target.language_id == "scala"));
}

#[test]
fn dependency_refresh_uses_session_language_filters_before_scope_row_exists() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_refresh_schema(&connection);
    connection
        .execute(
            "
            INSERT INTO code_repository_chunks (
                repository_id, source_scope, chunk_id, file_id, path, content,
                byte_start, byte_end, line_start, line_end, symbol_snapshot_id
            )
            VALUES ('repo', 'fresh', 'pom-chunk', 'pom-file', 'pom.xml',
                '<project><modelVersion>4.0.0</modelVersion><groupId>com.acme</groupId><artifactId>service</artifactId><version>1.0.0</version><dependencies><dependency><groupId>org.example</groupId><artifactId>api</artifactId><version>1.2.3</version></dependency></dependencies></project>',
                0, 272, 1, 1, NULL)
            ",
            [],
        )
        .expect("pom chunk should seed");

    let transaction = connection.transaction().expect("transaction should start");
    refresh_effective_dependencies_with_language_filters(
        &transaction,
        "fresh",
        &["kotlin".to_owned()],
    )
    .expect("refresh should use session filters");
    transaction.commit().expect("refresh should commit");

    let kotlin_count: i64 = connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM code_repository_dependencies
            WHERE source_scope = 'fresh'
              AND ecosystem = 'maven'
              AND language_id = 'kotlin'
            ",
            [],
            |row| row.get(0),
        )
        .expect("kotlin count should load");
    let other_jvm_count: i64 = connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM code_repository_dependencies
            WHERE source_scope = 'fresh'
              AND ecosystem = 'maven'
              AND language_id IN ('java', 'scala')
            ",
            [],
            |row| row.get(0),
        )
        .expect("other JVM count should load");
    assert!(kotlin_count > 0, "kotlin Maven rows should be inserted");
    assert_eq!(
        other_jvm_count, 0,
        "session filters must apply before scope row exists"
    );
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

fn create_refresh_schema(connection: &Connection) {
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_scopes (
                source_scope TEXT PRIMARY KEY,
                repository_id TEXT NOT NULL,
                language_filters_json TEXT NOT NULL
            );
            INSERT INTO code_repository_scopes (
                source_scope, repository_id, language_filters_json
            )
            VALUES ('scope', 'repo', '[\"kotlin\"]');
            CREATE TABLE code_repository_chunks (
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                chunk_id TEXT NOT NULL,
                file_id TEXT NOT NULL,
                path TEXT NOT NULL,
                content TEXT NOT NULL,
                byte_start INTEGER NOT NULL,
                byte_end INTEGER NOT NULL,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL,
                symbol_snapshot_id TEXT
            );
            CREATE TABLE code_repository_dependencies (
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                dependency_id TEXT NOT NULL,
                file_id TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL,
                ecosystem TEXT NOT NULL,
                package_name TEXT NOT NULL,
                requirement TEXT,
                resolved_version TEXT,
                dependency_group TEXT NOT NULL,
                source_kind TEXT NOT NULL,
                is_lockfile INTEGER NOT NULL,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL,
                excerpt TEXT NOT NULL
            );
            CREATE VIRTUAL TABLE code_repository_search USING fts5(
                source_scope UNINDEXED,
                document_kind UNINDEXED,
                record_id UNINDEXED,
                path UNINDEXED,
                language_id UNINDEXED,
                content
            );
            ",
        )
        .expect("schema should create");
}

fn seed_existing_dependency(connection: &Connection) {
    connection
        .execute(
            "
            INSERT INTO code_repository_dependencies (
                repository_id, source_scope, dependency_id, file_id, path, language_id,
                ecosystem, package_name, requirement, resolved_version, dependency_group,
                source_kind, is_lockfile, line_start, line_end, excerpt
            )
            VALUES ('repo', 'scope', 'dep-old', 'pom-file', 'pom.xml', 'java',
                'maven', 'org.slf4j:slf4j-api', '2.0.9', NULL, 'compile',
                'pom.xml', 0, 1, 1, 'old')
            ",
            [],
        )
        .expect("dependency should seed");
    connection
        .execute(
            "
            INSERT INTO code_repository_search (
                source_scope, document_kind, record_id, path, language_id, content
            )
            VALUES ('scope', 'dependency', 'dep-old', 'pom.xml', 'java', 'old')
            ",
            [],
        )
        .expect("search row should seed");
}

fn count_rows(connection: &Connection, sql: &str) -> i64 {
    connection
        .query_row(sql, params![], |row| row.get(0))
        .expect("row count should load")
}
