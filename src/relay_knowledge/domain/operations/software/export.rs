use serde::{Deserialize, Serialize};

/// Interoperability profiles supported by the software ontology exporter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SoftwareExportProfile {
    #[serde(rename = "spdx-3")]
    Spdx3,
    #[serde(rename = "cyclonedx-1.7")]
    Cyclonedx17,
    #[serde(rename = "prov-o")]
    ProvO,
}

impl SoftwareExportProfile {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Spdx3 => "spdx-3",
            Self::Cyclonedx17 => "cyclonedx-1.7",
            Self::ProvO => "prov-o",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "spdx-3" => Some(Self::Spdx3),
            "cyclonedx-1.7" => Some(Self::Cyclonedx17),
            "prov-o" => Some(Self::ProvO),
            _ => None,
        }
    }

    pub const fn media_type(self) -> &'static str {
        match self {
            Self::Spdx3 | Self::ProvO => "application/ld+json",
            Self::Cyclonedx17 => "application/vnd.cyclonedx+json; version=1.7",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SoftwareExportProfile;

    #[test]
    fn profile_names_round_trip_without_aliases() {
        for profile in [
            SoftwareExportProfile::Spdx3,
            SoftwareExportProfile::Cyclonedx17,
            SoftwareExportProfile::ProvO,
        ] {
            assert_eq!(
                SoftwareExportProfile::parse(profile.as_str()),
                Some(profile)
            );
            assert_eq!(
                serde_json::to_value(profile).expect("profile should serialize"),
                serde_json::json!(profile.as_str())
            );
            assert_eq!(
                serde_json::from_value::<SoftwareExportProfile>(serde_json::json!(
                    profile.as_str()
                ))
                .expect("profile should deserialize"),
                profile
            );
        }
        assert_eq!(SoftwareExportProfile::parse("spdx"), None);
    }
}
