//! AST facts must resolve through the same bounded persisted service as Java.
use super::*;

#[test]
fn portable_imported_getter_fallback_keeps_unproved_return_semantics() {
    let db = fixture();
    add_source(
        &db,
        "settings.js",
        "export function flag(){return Boolean(process.env.FEATURE)}",
    );
    add_source(
        &db,
        "reader.js",
        "import {flag} from './settings.js'; const enabled=flag() ?? true; if(enabled) {}",
    );
    let groups = search(
        &db,
        &status(),
        &request(Some("FEATURE"), CodeConfigFilter::default()),
    )
    .unwrap();
    assert!(groups.iter().any(|group| !group.analysis_complete));
    assert!(
        groups
            .iter()
            .flat_map(|group| &group.usages)
            .filter(|u| u.path == "reader.js")
            .all(|u| u.metadata.default_value.as_deref() != Some("true"))
    );
}

#[test]
fn portable_imports_preserve_exports_extensions_and_lexical_scope() {
    for sources in [
        vec![
            (
                "Settings.cs",
                "namespace Local {public class Safe {public static bool flag(){return Boolean.Parse(System.Environment.GetEnvironmentVariable(\"FEATURE\"));}}}",
            ),
            (
                "Reader.cs",
                "namespace Consumer {using Settings = Local.Safe; class Reader {void Run(){if(Settings.flag()) {}}}}",
            ),
        ],
        vec![
            (
                "settings.sh",
                "flag() { printf '%s' \"${FEATURE:-false}\"; }\nunset -f flag\n",
            ),
            (
                "reader.sh",
                "source \"$(dirname \"${BASH_SOURCE[0]}\")/settings.sh\"\nvalue=$(flag)\nif [ \"$value\" = true ]; then echo ok; fi\n",
            ),
        ],
        vec![
            (
                "Settings.cs",
                "public class Settings {public static string flag(){return System.Environment.GetEnvironmentVariable(\"FEATURE\");}}",
            ),
            (
                "Reader.cs",
                "namespace Consumer {class Reader {void Run(){if(Settings.flag()==\"on\") {}}}}",
            ),
        ],
        vec![
            (
                "Settings.cs",
                "public class Settings {public static string flag(){return System.Environment.GetEnvironmentVariable(\"FEATURE\");}}",
            ),
            (
                "Reader.cs",
                "namespace Consumer; class Reader {void Run(){if(Settings.flag()==\"on\") {}}}",
            ),
        ],
        vec![
            (
                "settings.scala",
                "object Settings {def flag(): String = System.getenv(\"FEATURE\")}",
            ),
            (
                "reader.scala",
                "import Other.{Safe => Settings}\ndef run() = if(Settings.flag()!=null) {}",
            ),
        ],
        vec![
            (
                "Settings.cs",
                "public class Settings {public static string flag(){return System.Environment.GetEnvironmentVariable(\"FEATURE\");}}",
            ),
            (
                "Reader.cs",
                "using /* comment */ Settings = Other.Safe; class Reader {void Run(){if(Settings.flag()==\"on\") {}}}",
            ),
        ],
        vec![
            (
                "settings.py",
                "import os\nclass Settings:\n    @staticmethod\n    def flag(): return os.getenv('FEATURE')\nSettings.flag=lambda: False\n",
            ),
            (
                "reader.py",
                "from settings import Settings\nif Settings.flag(): pass\n",
            ),
        ],
        vec![
            (
                "settings.js",
                "export function flag(){return process.env.FEATURE}; flag=()=>false;",
            ),
            (
                "reader.js",
                "import {flag} from './settings.js'; if(flag()) {}",
            ),
        ],
        vec![
            (
                "settings.py",
                "import os\ndef flag(): return os.getenv('FEATURE')\nflag=lambda: False\n",
            ),
            ("reader.py", "from settings import flag\nif flag(): pass\n"),
        ],
        vec![
            (
                "Settings.cs",
                "public class Settings {public static string flag(){return System.Environment.GetEnvironmentVariable(\"FEATURE\");}}",
            ),
            (
                "Safe.cs",
                "namespace Other {public class Safe {public static string flag(){return \"off\";}}}",
            ),
            (
                "Reader.cs",
                "using Settings = Other.Safe; class Reader {void Run(){if(Settings.flag()==\"on\") {}}}",
            ),
        ],
        vec![
            (
                "Settings.kt",
                "object Settings {fun flag(): String {return System.getenv(\"FEATURE\")}}",
            ),
            (
                "Reader.kt",
                "import Other.Safe as Settings\nfun run(){if(Settings.flag()!=null){}}",
            ),
        ],
        vec![
            ("src/lib.rs", "mod settings; mod reader;"),
            (
                "src/settings.rs",
                "/* pub */ fn flag()->String { std::env::var(\"FEATURE\").unwrap_or(\"off\".to_owned()) }",
            ),
            (
                "src/reader.rs",
                "use crate::settings::flag; fn run(){if flag()==\"on\" {}}",
            ),
        ],
        vec![
            (
                "settings.js",
                "export function flag(){return process.env.FEATURE} export default function safe(){return 'off'}",
            ),
            (
                "reader.js",
                "import flag from './settings.js'; if(flag()) {}",
            ),
        ],
        vec![
            ("settings.js", "export function flag(){return false}"),
            (
                "settings.jsx",
                "export function flag(){return process.env.FEATURE}",
            ),
            (
                "reader.js",
                "import {flag} from './settings.js'; if(flag()) {}",
            ),
        ],
        vec![
            ("settings.ts", "export function flag(){return false}"),
            (
                "settings.tsx",
                "export function flag(){return process.env.FEATURE}",
            ),
            (
                "reader.ts",
                "import {flag} from './settings.ts'; if(flag()) {}",
            ),
        ],
        vec![
            ("settings.js", "function flag(){return process.env.FEATURE}"),
            (
                "reader.js",
                "import {flag} from './settings.js'; if(flag()) {}",
            ),
        ],
        vec![
            (
                "settings.py",
                "import os\ndef flag(): return os.getenv('FEATURE')\n",
            ),
            (
                "reader.py",
                "def load():\n    from settings import flag\nvalue=flag()\n",
            ),
        ],
        vec![
            (
                "settings.py",
                "import os\ndef flag(): return os.getenv('FEATURE')\n",
            ),
            (
                "reader.py",
                "if False:\n    from settings import flag\nvalue=flag()\n",
            ),
        ],
    ] {
        let db = fixture();
        for (path, source) in &sources {
            add_source(&db, path, source);
        }
        let reader = sources.last().unwrap().0;
        let mut req = request(None, CodeConfigFilter::default());
        req.repository.path_filters = vec![reader.into()];
        let groups = search(&db, &status(), &req).unwrap();
        assert!(
            !groups
                .iter()
                .any(|g| g.source_key == "FEATURE" && g.usages.iter().any(|u| u.path == reader)),
            "{sources:?}: {groups:?}"
        );
    }
    let db = fixture();
    add_source(
        &db,
        "settings.js",
        "export default function flag(){return process.env.FEATURE}",
    );
    add_source(
        &db,
        "reader.js",
        "import renamed from './settings.js'; if(renamed()) {}",
    );
    let groups = search(
        &db,
        &status(),
        &request(Some("FEATURE"), CodeConfigFilter::default()),
    )
    .unwrap();
    assert!(
        groups.iter().any(|g| g
            .usages
            .iter()
            .any(|u| u.path == "reader.js" && u.edge_kind == "guards_code")),
        "{groups:?}"
    );
}

fn add_source(db: &Connection, path: &str, source: &str) {
    let snapshot = crate::code::syntax_snapshot_for_tests(&[(path, source)]);
    let language = snapshot.files[0].language_id.as_str();
    let rows = snapshot.feature_flags;
    for symbol in snapshot.symbols {
        db.execute("INSERT INTO code_repository_symbols (source_scope,path,line_start,line_end,symbol_snapshot_id,name,language_id,type_owner_json) VALUES ('scope',?1,?2,?3,?4,?5,?6,?7)",params![path,symbol.line_range.start,symbol.line_range.end,symbol.symbol_snapshot_id,symbol.name,symbol.language_id,symbol.type_owner.map(|o|serde_json::to_string(&o).unwrap())]).unwrap();
    }
    db.execute(
        "INSERT INTO code_repository_files VALUES ('scope',?1,?2)",
        params![path, language],
    )
    .unwrap();
    for row in rows {
        db.execute("INSERT INTO code_repository_feature_flags VALUES (?1,?2,?3,?3,?4,?5,?6,?7,?8,9000,'extracted',?9,?10,?11,?12,?13,?14,'scope')",
            params![row.feature_flag_id, row.usage_id, path, row.language_id, row.name, row.source_kind, row.source_key, row.edge_kind, row.byte_range.start, row.byte_range.end, row.line_range.start, row.line_range.end, row.excerpt, serde_json::to_string(&row.metadata).unwrap()]).unwrap();
    }
}

#[test]
fn portable_configuration_constants_and_getters_resolve_across_files() {
    let mut failures = Vec::new();
    for (provider, provider_source, reader, reader_source) in [
        (
            "settings.php",
            "<?php const KEY='FEATURE'; function flag() { return getenv(KEY); }",
            "reader.php",
            "<?php require __DIR__ . '/settings.php'; $x=getenv(KEY); $y=flag(); if($y) {}",
        ),
        (
            "settings.jsx",
            "export const KEY='FEATURE'; export function flag() { return process.env[KEY]; }",
            "reader.jsx",
            "import {KEY,flag} from './settings.jsx'; const x=process.env[KEY]; const y=flag(); if(y) {}",
        ),
        (
            "settings.tsx",
            "export const KEY='FEATURE'; export function flag() { return process.env[KEY]; }",
            "reader.tsx",
            "import {KEY,flag} from './settings.tsx'; const x=process.env[KEY]; const y=flag(); if(y) {}",
        ),
        (
            "settings.py",
            "import os\nKEY='FEATURE'\ndef flag():\n    return os.getenv(KEY, 'false')\n",
            "reader.py",
            "import os\nfrom settings import KEY, flag\nx=os.getenv(KEY)\ny=flag()\nif y:\n    pass\n",
        ),
        (
            "settings.js",
            "export const KEY='FEATURE'; export function flag() { return process.env[KEY]; }",
            "reader.js",
            "import {KEY, flag} from './settings.js'; const x=process.env[KEY]; const y=flag(); if (y) {}",
        ),
        (
            "settings.ts",
            "export const KEY='FEATURE'; export function flag() { return process.env[KEY]; }",
            "reader.ts",
            "import {KEY, flag} from './settings.ts'; const x=process.env[KEY]; const y=flag(); if (y) {}",
        ),
        (
            "src/settings.rs",
            "pub const KEY: &str = \"FEATURE\"; pub fn flag() -> Result<String,std::env::VarError> { std::env::var(KEY) }",
            "src/reader.rs",
            "use crate::settings::KEY; use crate::settings::flag; fn run() { let x=std::env::var(KEY); let y=flag(); if y.is_ok() {} }",
        ),
        (
            "settings.h",
            "const char * const KEY = \"FEATURE\"; char *flag(void) { return getenv(KEY); }",
            "reader.c",
            "#include \"settings.h\"\nvoid run() { char *x=getenv(KEY); char *y=flag(); if (y) {} }",
        ),
        (
            "settings.hpp",
            "const char * const KEY = \"FEATURE\"; char *flag() { return std::getenv(KEY); }",
            "reader.cpp",
            "#include \"settings.hpp\"\nvoid run() { auto x=std::getenv(KEY); auto y=flag(); if (y) {} }",
        ),
        (
            "settings.go",
            "package demo\nconst KEY=\"FEATURE\"\nfunc flag() string { return os.Getenv(KEY) }",
            "reader.go",
            "package demo\nfunc run() { x:=os.Getenv(KEY); y:=flag(); if y != \"\" {} }",
        ),
        (
            "Settings.cs",
            "class Settings { public const string KEY=\"FEATURE\"; public static string flag() { return System.Environment.GetEnvironmentVariable(KEY); } }",
            "Reader.cs",
            "class Reader { void run() { var x=System.Environment.GetEnvironmentVariable(Settings.KEY); var y=Settings.flag(); if(y!=null) {} } }",
        ),
        (
            "Settings.cs",
            "namespace Shared; public class Settings { public const string KEY=\"FEATURE\"; public static string flag() { return System.Environment.GetEnvironmentVariable(KEY); } }",
            "Reader.cs",
            "namespace Shared; class Reader { void run() { var x=System.Environment.GetEnvironmentVariable(Settings.KEY); var y=Settings.flag(); if(y!=null) {} } }",
        ),
        (
            "settings.kt",
            "val KEY=\"FEATURE\"\nfun flag(): String { return System.getenv(KEY) }",
            "reader.kt",
            "fun run() { val x=System.getenv(KEY); val y=flag(); if(y!=null) {} }",
        ),
        (
            "settings.scala",
            "val KEY=\"FEATURE\"\ndef flag(): String = { return System.getenv(KEY) }",
            "reader.scala",
            "def run(): Unit = { val x=System.getenv(KEY); val y=flag(); if(y!=null) {} }",
        ),
        (
            "settings.rb",
            "KEY='FEATURE'\ndef flag\n ENV.fetch(KEY, 'false')\nend\n",
            "reader.rb",
            "require_relative 'settings'\nx=ENV.fetch(KEY)\ny=flag()\nif y\nend\n",
        ),
        (
            "settings.swift",
            "let KEY=\"FEATURE\"\nfunc flag() -> String? { return ProcessInfo.processInfo.environment[KEY] }",
            "reader.swift",
            "func run() { let x=ProcessInfo.processInfo.environment[KEY]; let y=flag(); if y != nil {} }",
        ),
        (
            "settings.bzl",
            "KEY='FEATURE'\ndef flag():\n    return config.get(KEY)\n",
            "reader.bzl",
            "load(':settings.bzl', 'KEY', 'flag')\nx=config.get(KEY)\ny=flag()\nif y:\n    pass\n",
        ),
    ] {
        let db = fixture();
        if provider.ends_with(".rs") {
            add_source(&db, "src/lib.rs", "mod settings; mod reader;");
        }
        add_source(&db, provider, provider_source);
        add_source(&db, reader, reader_source);
        let mut req = request(None, CodeConfigFilter::default());
        req.limit = 100;
        req.repository.path_filters = vec![reader.into()];
        let groups = search(&db, &status(), &req).unwrap();
        if !groups.iter().any(|g| {
            g.source_key == "FEATURE"
                && g.analysis_complete
                && g.usages
                    .iter()
                    .filter(|u| u.path == reader && u.edge_kind == "reads_config")
                    .count()
                    >= 2
                && g.usages.iter().any(|u| u.edge_kind == "guards_code")
        }) {
            let mut statement = db.prepare("SELECT path,source_key,edge_kind,metadata_json FROM code_repository_feature_flags").unwrap();
            let rows: Vec<(String, String, String, String)> = statement
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            failures.push(format!("{reader}: {rows:?}; groups={groups:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn portable_module_loads_do_not_join_unloaded_files() {
    for sources in [
        vec![
            (
                "src/main.rs",
                "#[path=\"safe.rs\"] mod settings; use crate::settings::flag; fn main(){if flag() {}}",
            ),
            ("src/safe.rs", "pub fn flag()->bool {false}"),
            (
                "src/settings.rs",
                "pub fn flag()->Result<String,std::env::VarError>{std::env::var(\"FEATURE\")}",
            ),
        ],
        vec![
            (
                "reader.php",
                "<?php require __DIR__.'/safe.php'; if(flag()) {}",
            ),
            ("safe.php", "<?php function flag(){return 'off';}"),
            (
                "feature.php",
                "<?php function flag(){return getenv('FEATURE');}",
            ),
        ],
    ] {
        let db = fixture();
        for (path, source) in &sources {
            add_source(&db, path, source);
        }
        let mut req = request(None, CodeConfigFilter::default());
        req.repository.path_filters = vec![sources[0].0.into()];
        let groups = search(&db, &status(), &req).unwrap();
        assert!(
            !groups
                .iter()
                .any(|g| g.source_key == "FEATURE"
                    && g.usages.iter().any(|u| u.path == sources[0].0)),
            "{groups:?}"
        );
    }
}

#[test]
fn portable_vue_scripts_and_anchored_shell_sources_resolve_getters() {
    for (provider, source, reader, consumer) in [
        (
            "settings.js",
            "export function flag(){return process.env.FEATURE}",
            "reader.vue",
            "<script>import {flag} from './settings.js'; const value=flag(); if(value) {}</script><template><div /></template>",
        ),
        (
            "settings.ts",
            "export function flag(){return process.env.FEATURE}",
            "reader.vue",
            "<script lang=\"ts\">import {flag} from './settings.ts'; const value=flag(); if(value) {}</script><template><div /></template>",
        ),
        (
            "scripts/settings.sh",
            "flag() { printf '%s' \"${FEATURE:-false}\"; }\n",
            "scripts/reader.sh",
            "source \"$(dirname \"${BASH_SOURCE[0]}\")/settings.sh\"\nvalue=$(flag)\nif [ \"$value\" = true ]; then echo ok; fi\n",
        ),
    ] {
        let db = fixture();
        add_source(&db, provider, source);
        add_source(&db, reader, consumer);
        let groups = search(
            &db,
            &status(),
            &request(Some("FEATURE"), CodeConfigFilter::default()),
        )
        .unwrap();
        assert!(
            groups.iter().any(|g| g.source_key == "FEATURE"
                && g.usages
                    .iter()
                    .any(|u| u.path == reader && u.edge_kind == "guards_code")),
            "{reader}: {groups:?}"
        );
    }
}

#[test]
fn code_index_persistence_performance_suite_portable_configuration_bounds_unrelated_evidence() {
    let db = fixture();
    add_source(
        &db,
        "settings.py",
        "import os\nKEY='FEATURE'\ndef flag():\n    return os.getenv(KEY)\n",
    );
    add_source(
        &db,
        "reader.py",
        "from settings import flag\nvalue=flag()\nif value:\n    pass\n",
    );
    db.execute_batch("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<8192)
        INSERT INTO code_repository_feature_flags SELECT 'unused:'||x,'usage:'||x,'file','other.ts','typescript','unused','config_key','unused','declares_string_constant',9000,'extracted',0,1,1,1,'unrelated','{}','scope' FROM n;").unwrap();
    let mut req = request(Some("FEATURE"), CodeConfigFilter::default());
    req.limit = 10;
    let result = search(&db, &status(), &req).unwrap();
    assert_eq!(result.len(), 1);
    assert!(result[0].analysis_complete);
    assert!(
        result[0]
            .usages
            .iter()
            .any(|u| u.path == "reader.py" && u.edge_kind == "guards_code")
    );
    assert!(
        (1..2_000_000).contains(&super::super::LAST_QUERY_STEPS.with(std::cell::Cell::get)),
        "configuration work must fit the existing SQLite budget"
    );
}
