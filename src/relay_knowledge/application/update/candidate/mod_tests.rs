// Direct tests for update candidate selection.

use super::*;

#[test]
fn highest_stable_candidate_drives_the_update_response() {
    let response = response_from_candidates(
        StableVersion::new(1, 0, 4),
        vec![
            ReleaseCandidate {
                source: UpdateSource::Github,
                version: StableVersion::new(1, 0, 5),
                release_url: "https://github.example/release".to_owned(),
            },
            ReleaseCandidate {
                source: UpdateSource::CratesIo,
                version: StableVersion::new(1, 0, 6),
                release_url: "https://crates.example/release".to_owned(),
            },
        ],
        Vec::new(),
        42,
    );

    assert!(response.update_available);
    assert_eq!(response.latest_version, Some("1.0.6".to_owned()));
    assert_eq!(response.source, Some("crates.io".to_owned()));
}

#[test]
fn matching_stable_candidate_is_newer_than_a_prerelease_binary() {
    let response = response_from_candidates(
        StableVersion::prerelease(1, 0, 5),
        vec![ReleaseCandidate {
            source: UpdateSource::Github,
            version: StableVersion::new(1, 0, 5),
            release_url: "https://github.example/release".to_owned(),
        }],
        Vec::new(),
        42,
    );

    assert!(response.update_available);
    assert_eq!(response.latest_version, Some("1.0.5".to_owned()));
}
