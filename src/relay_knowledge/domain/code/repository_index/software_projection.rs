//! Typed checkpoints for the durable software-global projection tail.

pub(crate) const SOFTWARE_PROJECTION_CHECKPOINT: &str = "finalizing:software_projection";
const SOFTWARE_PROJECTION_CHECKPOINT_PREFIX_V1: &str = "finalizing:software_projection:v1:";
const SOFTWARE_PROJECTION_CHECKPOINT_PREFIX_V2: &str = "finalizing:software_projection:v2:";
const SOFTWARE_PROJECTION_CHECKPOINT_PREFIX_V3: &str = "finalizing:software_projection:v3:";

/// One bounded writer phase in the fenced software projection workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodeSoftwareProjectionPhase {
    Reset,
    Dependencies,
    SdkUsages,
    Lifecycle,
    Files,
    Topics,
    Relationships,
    Ontology,
    Publish,
}

impl CodeSoftwareProjectionPhase {
    pub(crate) const COUNT: usize = 9;

    pub(crate) const fn checkpoint_state(self) -> &'static str {
        match self {
            Self::Reset => "finalizing:software_projection:v3:reset",
            Self::Dependencies => "finalizing:software_projection:v3:dependencies",
            Self::SdkUsages => "finalizing:software_projection:v3:sdk_usages",
            Self::Lifecycle => "finalizing:software_projection:v3:lifecycle",
            Self::Files => "finalizing:software_projection:v3:files",
            Self::Topics => "finalizing:software_projection:v3:topics",
            Self::Relationships => "finalizing:software_projection:v3:relationships",
            Self::Ontology => "finalizing:software_projection:v3:ontology",
            Self::Publish => "finalizing:software_projection:v3:publish",
        }
    }

    pub(crate) const fn next(self) -> Option<Self> {
        match self {
            Self::Reset => Some(Self::Dependencies),
            Self::Dependencies => Some(Self::SdkUsages),
            Self::SdkUsages => Some(Self::Lifecycle),
            Self::Lifecycle => Some(Self::Files),
            Self::Files => Some(Self::Topics),
            Self::Topics => Some(Self::Relationships),
            Self::Relationships => Some(Self::Ontology),
            Self::Ontology => Some(Self::Publish),
            Self::Publish => None,
        }
    }
}

/// Decodes both the legacy coarse checkpoint and versioned durable phases.
pub(crate) fn code_software_projection_phase(state: &str) -> Option<CodeSoftwareProjectionPhase> {
    if state == SOFTWARE_PROJECTION_CHECKPOINT {
        return Some(CodeSoftwareProjectionPhase::Reset);
    }
    // Old nonterminal checkpoints must replay all derivations, including the new reactor phase.
    // The fenced writer atomically commits reset rows and a v3 token using the original token as CAS.
    if let Some(phase) = state
        .strip_prefix(SOFTWARE_PROJECTION_CHECKPOINT_PREFIX_V1)
        .or_else(|| state.strip_prefix(SOFTWARE_PROJECTION_CHECKPOINT_PREFIX_V2))
    {
        return matches!(
            phase,
            "reset"
                | "dependencies"
                | "sdk_usages"
                | "lifecycle"
                | "files"
                | "topics"
                | "relationships"
                | "ontology"
                | "publish"
        )
        .then_some(CodeSoftwareProjectionPhase::Reset);
    }
    let phase = state.strip_prefix(SOFTWARE_PROJECTION_CHECKPOINT_PREFIX_V3)?;
    match phase {
        "reset" => Some(CodeSoftwareProjectionPhase::Reset),
        "dependencies" => Some(CodeSoftwareProjectionPhase::Dependencies),
        "sdk_usages" => Some(CodeSoftwareProjectionPhase::SdkUsages),
        "lifecycle" => Some(CodeSoftwareProjectionPhase::Lifecycle),
        "files" => Some(CodeSoftwareProjectionPhase::Files),
        "topics" => Some(CodeSoftwareProjectionPhase::Topics),
        "relationships" => Some(CodeSoftwareProjectionPhase::Relationships),
        "ontology" => Some(CodeSoftwareProjectionPhase::Ontology),
        "publish" => Some(CodeSoftwareProjectionPhase::Publish),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CodeSoftwareProjectionPhase, SOFTWARE_PROJECTION_CHECKPOINT, code_software_projection_phase,
    };

    #[test]
    fn legacy_checkpoint_resumes_at_reset() {
        assert_eq!(
            code_software_projection_phase(SOFTWARE_PROJECTION_CHECKPOINT),
            Some(CodeSoftwareProjectionPhase::Reset)
        );
    }

    #[test]
    fn pre_reactor_checkpoints_restart_instead_of_skipping_lifecycle() {
        for version in ["v1", "v2"] {
            for phase in [
                "reset",
                "dependencies",
                "sdk_usages",
                "lifecycle",
                "files",
                "topics",
                "relationships",
                "ontology",
                "publish",
            ] {
                let token = format!("finalizing:software_projection:{version}:{phase}");
                assert_eq!(
                    code_software_projection_phase(&token),
                    Some(CodeSoftwareProjectionPhase::Reset)
                );
            }
        }
    }

    #[test]
    fn versioned_checkpoints_round_trip_in_order() {
        let mut phase = CodeSoftwareProjectionPhase::Reset;
        let mut count = 0;
        loop {
            assert_eq!(
                code_software_projection_phase(phase.checkpoint_state()),
                Some(phase)
            );
            count += 1;
            let Some(next) = phase.next() else { break };
            phase = next;
        }
        assert_eq!(count, CodeSoftwareProjectionPhase::COUNT);
    }

    #[test]
    fn malformed_or_future_checkpoints_are_not_silently_accepted() {
        for state in [
            "finalizing:software_projection:v1:",
            "finalizing:software_projection:v1:unknown",
            "finalizing:software_projection:v4:reset",
            "finalizing:software_projection:reset",
        ] {
            assert_eq!(code_software_projection_phase(state), None, "state={state}");
        }
    }
}
