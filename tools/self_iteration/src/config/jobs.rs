#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Jobs {
    Auto,
    Fixed(usize),
}

impl Jobs {
    pub(super) fn parse(value: &str) -> Result<Self, String> {
        if value == "auto" {
            return Ok(Self::Auto);
        }
        let parsed = value
            .parse::<usize>()
            .map_err(|_| format!("invalid job value: {value}"))?;
        if parsed == 0 {
            return Err("job value must be greater than zero".to_owned());
        }
        Ok(Self::Fixed(parsed))
    }

    pub(super) fn resolve(self, default: usize) -> usize {
        match self {
            Self::Auto => default.max(1),
            Self::Fixed(value) => value.max(1),
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::Auto => "auto".to_owned(),
            Self::Fixed(value) => value.to_string(),
        }
    }
}

#[cfg(test)]
#[path = "jobs_tests.rs"]
mod jobs_tests;
