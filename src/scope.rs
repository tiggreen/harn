use crate::ScopeArg;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AnalysisScope {
    Project,
    User,
    Both,
}

impl AnalysisScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::User => "user",
            Self::Both => "both",
        }
    }

    pub fn headline(self) -> &'static str {
        match self {
            Self::Project => "this project",
            Self::User => "your broader Claude Code history",
            Self::Both => "this project and your broader Claude Code history",
        }
    }
}

impl From<ScopeArg> for AnalysisScope {
    fn from(value: ScopeArg) -> Self {
        match value {
            ScopeArg::Project => Self::Project,
            ScopeArg::User => Self::User,
            ScopeArg::Both => Self::Both,
        }
    }
}
