use std::path::PathBuf;

use super::shard_locator;
use crate::paths::RuntimePaths;

#[test]
fn shard_locator_is_relative_only_inside_the_runtime_data_directory() {
    let paths = runtime_paths(PathBuf::from("/var/lib/relay-knowledge"));
    let internal = paths.data_dir.join("repositories/repo.db");
    let external = PathBuf::from("/mnt/shards/repo.db");

    assert_eq!(shard_locator(&paths, &internal), "repositories/repo.db");
    assert_eq!(
        shard_locator(&paths, &external),
        external.display().to_string()
    );
}

fn runtime_paths(data_dir: PathBuf) -> RuntimePaths {
    RuntimePaths {
        config_dir: PathBuf::from("/etc/relay-knowledge"),
        data_dir,
        state_dir: PathBuf::from("/var/lib/relay-knowledge/state"),
        cache_dir: PathBuf::from("/var/cache/relay-knowledge"),
        log_dir: PathBuf::from("/var/log/relay-knowledge"),
        temp_dir: PathBuf::from("/tmp/relay-knowledge"),
        runtime_dir: PathBuf::from("/run/relay-knowledge"),
        service_dir: PathBuf::from("/etc/systemd/system"),
    }
}
