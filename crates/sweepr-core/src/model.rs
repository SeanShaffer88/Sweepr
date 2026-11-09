use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Manager {
    Winget,
    Choco,
    Scoop,
}

impl Manager {
    pub fn as_str(&self) -> &'static str {
        match self {
            Manager::Winget => "winget",
            Manager::Choco => "choco",
            Manager::Scoop => "scoop",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Machine,
    User,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdatePackage {
    pub id: String,
    pub name: String,
    pub current_version: String,
    pub available_version: String,
    pub manager: Manager,
    pub scope: Scope,
    pub elevation_required: bool,
    pub pinned_by_manager: bool,
    pub ignored: bool,
    pub user_pin: Option<String>,
}

impl UpdatePackage {
    pub fn key(&self) -> (Manager, &str) {
        (self.manager, self.id.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_json_roundtrip() {
        let p = UpdatePackage {
            id: "Google.Chrome".into(),
            name: "Google Chrome".into(),
            current_version: "120.0".into(),
            available_version: "121.0".into(),
            manager: Manager::Winget,
            scope: Scope::Machine,
            elevation_required: true,
            pinned_by_manager: false,
            ignored: false,
            user_pin: None,
        };
        let s = serde_json::to_string(&p).unwrap();
        assert!(s.contains("\"manager\":\"winget\""));
        let back: UpdatePackage = serde_json::from_str(&s).unwrap();
        assert_eq!(back, p);
        assert_eq!(p.key(), (Manager::Winget, "Google.Chrome"));
    }
}
