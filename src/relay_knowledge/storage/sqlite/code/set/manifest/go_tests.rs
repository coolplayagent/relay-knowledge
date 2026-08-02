use super::super::{ManifestChunk, module_keys_for_path_with_prefixes, path as manifest_path};
use super::{collect_module_prefixes, module_allowed, workspaces};

#[test]
fn go_work_use_paths_limit_module_prefixes_to_workspace_members() {
    let chunks = vec![
        ManifestChunk {
            path: "go.work".to_owned(),
            content: "go 1.22\nuse (\n    ./component\n)\n".to_owned(),
        },
        ManifestChunk {
            path: "component/go.mod".to_owned(),
            content: "module go.opentelemetry.io/collector/component\n".to_owned(),
        },
        ManifestChunk {
            path: "sandbox/go.mod".to_owned(),
            content: "module example.com/sandbox\n".to_owned(),
        },
    ];
    let workspaces = workspaces(&chunks);
    let mut prefixes = Vec::new();
    for chunk in &chunks {
        if manifest_path::is_go_mod(&chunk.path) && module_allowed(&chunk.path, &workspaces) {
            collect_module_prefixes(&chunk.path, &chunk.content, &mut prefixes);
        }
    }

    assert_eq!(prefixes.len(), 1);
    assert_eq!(prefixes[0].source_path_prefix, "component");
    assert!(
        module_keys_for_path_with_prefixes("component/identifiable.go", &prefixes)
            .contains("go.opentelemetry.io.collector.component")
    );
    assert!(
        !module_keys_for_path_with_prefixes("sandbox/main.go", &prefixes)
            .contains("example.com.sandbox")
    );
}

#[test]
fn nested_go_work_only_filters_modules_under_its_root() {
    let chunks = vec![
        ManifestChunk {
            path: "go.mod".to_owned(),
            content: "module example.com/root\n".to_owned(),
        },
        ManifestChunk {
            path: "examples/go.work".to_owned(),
            content: "go 1.22\nuse ./demo\n".to_owned(),
        },
        ManifestChunk {
            path: "examples/demo/go.mod".to_owned(),
            content: "module example.com/demo\n".to_owned(),
        },
        ManifestChunk {
            path: "examples/other/go.mod".to_owned(),
            content: "module example.com/other\n".to_owned(),
        },
    ];
    let workspaces = workspaces(&chunks);
    let mut prefixes = Vec::new();
    for chunk in &chunks {
        if manifest_path::is_go_mod(&chunk.path) && module_allowed(&chunk.path, &workspaces) {
            collect_module_prefixes(&chunk.path, &chunk.content, &mut prefixes);
        }
    }

    assert_eq!(prefixes.len(), 2);
    assert!(module_keys_for_path_with_prefixes("main.go", &prefixes).contains("example.com.root"));
    assert!(
        module_keys_for_path_with_prefixes("examples/demo/main.go", &prefixes)
            .contains("example.com.demo")
    );
    assert!(
        !module_keys_for_path_with_prefixes("examples/other/main.go", &prefixes)
            .contains("example.com.other")
    );
}
