use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Environment {
    Ot,
    Rd,
}

impl Environment {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Ot => "OT",
            Self::Rd => "RD",
        }
    }
}

impl fmt::Display for Environment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_environment_labels() {
        assert_eq!(Environment::Ot.to_string(), "OT");
        assert_eq!(Environment::Rd.to_string(), "RD");
    }
}
