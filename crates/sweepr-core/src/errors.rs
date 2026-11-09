use crate::model::Manager;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwpError {
    ManagerMissing(Manager),
    Timeout(Manager),
    ParseFailed { manager: Manager, detail: String },
    UacDeclined,
    PipeLost,
    JobRejected(String),
    SettingsCorrupt(String),
    InstallerFailed { id: String, exit_code: i32 },
}

impl SwpError {
    pub fn code(&self) -> &'static str {
        match self {
            SwpError::ManagerMissing(_) => "SWP-101",
            SwpError::Timeout(_) => "SWP-102",
            SwpError::ParseFailed { .. } => "SWP-103",
            SwpError::UacDeclined => "SWP-201",
            SwpError::PipeLost => "SWP-202",
            SwpError::JobRejected(_) => "SWP-203",
            SwpError::SettingsCorrupt(_) => "SWP-301",
            SwpError::InstallerFailed { .. } => "SWP-401",
        }
    }
}

impl fmt::Display for SwpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = self.code();
        match self {
            SwpError::ManagerMissing(m) => {
                write!(f, "{c} manager-missing: {} not found", m.as_str())
            }
            SwpError::Timeout(m) => write!(f, "{c} timeout: {} did not answer in time", m.as_str()),
            SwpError::ParseFailed { manager, detail } => {
                write!(f, "{c} parse-failed: {}: {detail}", manager.as_str())
            }
            SwpError::UacDeclined => write!(f, "{c} uac-declined: elevation was refused"),
            SwpError::PipeLost => write!(
                f,
                "{c} elevated-pipe-lost: lost contact with the elevated helper"
            ),
            SwpError::JobRejected(d) => write!(f, "{c} job-rejected: {d}"),
            SwpError::SettingsCorrupt(d) => write!(f, "{c} settings-corrupt: {d}"),
            SwpError::InstallerFailed { id, exit_code } => {
                write!(f, "{c} installer-failed: {id} exited with code {exit_code}")
            }
        }
    }
}

impl std::error::Error for SwpError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Manager;

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(SwpError::ManagerMissing(Manager::Choco).code(), "SWP-101");
        assert_eq!(SwpError::Timeout(Manager::Scoop).code(), "SWP-102");
        assert_eq!(
            SwpError::ParseFailed {
                manager: Manager::Winget,
                detail: "x".into()
            }
            .code(),
            "SWP-103"
        );
        assert_eq!(SwpError::UacDeclined.code(), "SWP-201");
        assert_eq!(SwpError::PipeLost.code(), "SWP-202");
        assert_eq!(SwpError::JobRejected("x".into()).code(), "SWP-203");
        assert_eq!(SwpError::SettingsCorrupt("x".into()).code(), "SWP-301");
        assert_eq!(
            SwpError::InstallerFailed {
                id: "a".into(),
                exit_code: 1
            }
            .code(),
            "SWP-401"
        );
        assert!(SwpError::UacDeclined.to_string().starts_with("SWP-201"));
    }
}
