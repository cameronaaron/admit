use std::fmt;

/// Outcome of checking the profile or a portal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// Wrong in a way that matters (rejected by the portal, or implausible to
    /// a reviewer): `admit audit` exits non-zero.
    Violation(String),
    /// Something still to do, usually by a person.
    Pending(String),
    /// Worth knowing, nothing to fix.
    Note(String),
}

impl Finding {
    pub fn is_violation(&self) -> bool {
        matches!(self, Finding::Violation(_))
    }

    /// The text, without its severity.
    pub fn message(&self) -> &str {
        match self {
            Finding::Violation(m) | Finding::Pending(m) | Finding::Note(m) => m,
        }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Finding::Violation(m) => write!(f, "VIOLATION  {m}"),
            Finding::Pending(m) => write!(f, "PENDING    {m}"),
            Finding::Note(m) => write!(f, "note       {m}"),
        }
    }
}
