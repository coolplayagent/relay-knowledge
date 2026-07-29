#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Loop,
    Once,
    Evaluate,
    Chart,
    ResearchPlan,
}

impl Mode {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "loop" => Some(Self::Loop),
            "once" => Some(Self::Once),
            "evaluate" => Some(Self::Evaluate),
            "chart" => Some(Self::Chart),
            "research" | "research-plan" | "research_plan" => Some(Self::ResearchPlan),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    Single,
    UnattendedLayered,
}

impl Strategy {
    fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "single" => Ok(Self::Single),
            "unattended-layered" | "unattended_layered" | "layered" => Ok(Self::UnattendedLayered),
            other => Err(format!("invalid strategy: {other}")),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::UnattendedLayered => "unattended-layered",
        }
    }
}
