use crate::model::UpdatePackage;
use crate::settings::Settings;

/// Mark ignored packages and attach user pins. A pin means "skip this exact
/// available version".
pub fn apply_rules(pkgs: Vec<UpdatePackage>, s: &Settings) -> Vec<UpdatePackage> {
    pkgs.into_iter()
        .map(|mut p| {
            p.user_pin = s.pins.get(&p.id).cloned();
            p.ignored = s.ignore.iter().any(|i| i == &p.id)
                || p.user_pin.as_deref() == Some(p.available_version.as_str());
            p
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Manager, Scope, UpdatePackage};
    use crate::settings::Settings;

    fn pkg(id: &str, avail: &str) -> UpdatePackage {
        UpdatePackage {
            id: id.into(),
            name: id.into(),
            current_version: "1".into(),
            available_version: avail.into(),
            manager: Manager::Winget,
            scope: Scope::Unknown,
            elevation_required: false,
            pinned_by_manager: false,
            ignored: false,
            user_pin: None,
        }
    }

    #[test]
    fn ignored_and_pinned_rules_apply() {
        let mut s = Settings::default();
        s.ignore.push("A".into());
        s.pins.insert("B".into(), "2.0".into());
        s.pins.insert("C".into(), "9.9".into());
        let out = apply_rules(
            vec![
                pkg("A", "2.0"),
                pkg("B", "2.0"),
                pkg("C", "3.0"),
                pkg("D", "1.1"),
            ],
            &s,
        );
        assert!(out[0].ignored);
        assert!(out[1].ignored, "available == pin is skipped");
        assert_eq!(out[1].user_pin.as_deref(), Some("2.0"));
        assert!(!out[2].ignored, "different version than the pin is offered");
        assert!(!out[3].ignored);
    }
}
