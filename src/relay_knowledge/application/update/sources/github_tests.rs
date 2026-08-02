use super::*;
use crate::application::update::version::StableVersion;

#[test]
fn github_payload_maps_stable_release_and_rejects_prerelease() {
    let stable = github_candidate(GithubLatestRelease {
        tag_name: "v1.2.3".to_owned(),
        html_url: "https://github.example/release".to_owned(),
        prerelease: false,
    })
    .expect("GitHub release should parse");
    let prerelease = github_candidate(GithubLatestRelease {
        tag_name: "v1.2.4-rc.1".to_owned(),
        html_url: "https://github.example/prerelease".to_owned(),
        prerelease: true,
    })
    .expect_err("GitHub prerelease should be ignored");

    assert_eq!(stable.version, StableVersion::new(1, 2, 3));
    assert_eq!(prerelease.code, "prerelease_ignored");
}
