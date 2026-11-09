//! Pure helpers behind the toolbar, palette and exports. No Win32 here.

use sweepr_core::job::{new_job_id, Job, JobItem};
use sweepr_core::model::{Manager, UpdatePackage};
use sweepr_core::orchestrator::plan_queues;
use sweepr_core::validate::valid_package_id;

use super::model::MgrState;

/// 0 or 1: the user-space queue never prompts; all machine installs share one prompt.
pub fn uac_prompts_needed(sel: &[UpdatePackage], elevated: bool) -> u8 {
    u8::from(!plan_queues(sel, elevated).system.is_empty())
}

fn job_of(pkgs: &[UpdatePackage]) -> Option<Job> {
    let items: Vec<JobItem> = pkgs
        .iter()
        .filter(|p| valid_package_id(&p.id))
        .map(|p| JobItem {
            manager: p.manager,
            id: p.id.clone(),
            scope: p.scope,
        })
        .collect();
    if items.is_empty() {
        None
    } else {
        Some(Job {
            id: new_job_id(),
            items,
        })
    }
}

/// (user-space job, system job). Packages with ids that fail validation are left out.
pub fn build_jobs(sel: &[UpdatePackage], elevated: bool) -> (Option<Job>, Option<Job>) {
    let plan = plan_queues(sel, elevated);
    (job_of(&plan.user), job_of(&plan.system))
}

/// Replaces the user profile path (any case) so exported logs carry no account name.
pub fn redact(text: &str, profile: &str) -> String {
    if profile.is_empty() {
        return text.to_string();
    }
    let (lt, lp) = (text.to_lowercase(), profile.to_lowercase());
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while let Some(pos) = lt[i..].find(&lp) {
        out.push_str(&text[i..i + pos]);
        out.push_str("%USERPROFILE%");
        i += pos + lp.len();
    }
    out.push_str(&text[i..]);
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    Check,
    UpdateSelected,
    UpdateAll,
    ToggleLog,
    Settings,
    ShowIgnored,
    SelectAll,
    SelectNone,
}

pub const COMMANDS: [(Cmd, &str, &str); 8] = [
    (Cmd::Check, "Check for updates", "F5"),
    (Cmd::UpdateSelected, "Update selected", "Ctrl+U"),
    (Cmd::UpdateAll, "Update all", "Ctrl+Shift+U"),
    (Cmd::ToggleLog, "Toggle log drawer", "Ctrl+L"),
    (Cmd::Settings, "Open settings", "Ctrl+,"),
    (Cmd::ShowIgnored, "Show or hide ignored packages", "Ctrl+I"),
    (Cmd::SelectAll, "Select all", ""),
    (Cmd::SelectNone, "Select none", ""),
];

pub fn filter_commands(q: &str) -> Vec<Cmd> {
    let q = q.trim().to_lowercase();
    COMMANDS
        .iter()
        .filter(|(_, title, _)| q.is_empty() || title.to_lowercase().contains(&q))
        .map(|(c, _, _)| *c)
        .collect()
}

/// Title and hint for the empty list, or `None` while rows exist or a check is running.
pub fn empty_state(
    states: &[(Manager, MgrState)],
    total: usize,
    checking: bool,
) -> Option<(&'static str, &'static str)> {
    if total > 0 || checking || states.is_empty() {
        return None;
    }
    if states.iter().any(|(_, s)| *s == MgrState::Checking) {
        return None;
    }
    if states.iter().all(|(_, s)| *s == MgrState::Missing) {
        return Some((
            "No package manager found",
            "Install WinGet, Chocolatey or Scoop. Sweepr never installs them for you.",
        ));
    }
    if states.iter().any(|(_, s)| matches!(s, MgrState::Failed(_))) {
        return Some((
            "Sweepr could not read some package managers",
            "Open the log (Ctrl+L) to see each manager's output and error code.",
        ));
    }
    Some((
        "Everything is up to date",
        "Nothing to update right now. Press F5 to check again.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sweepr_core::model::{Manager, Scope, UpdatePackage};

    fn pkg(m: Manager, id: &str, scope: Scope) -> UpdatePackage {
        UpdatePackage {
            id: id.into(),
            name: id.into(),
            current_version: "1".into(),
            available_version: "2".into(),
            manager: m,
            scope,
            elevation_required: scope == Scope::Machine,
            pinned_by_manager: false,
            ignored: false,
            user_pin: None,
        }
    }

    #[test]
    fn uac_count_is_zero_when_only_user_items() {
        let sel = [
            pkg(Manager::Scoop, "fd", Scope::User),
            pkg(Manager::Winget, "a.b", Scope::Unknown),
        ];
        assert_eq!(uac_prompts_needed(&sel, false), 0);
    }

    #[test]
    fn uac_count_is_one_for_mixed_selection() {
        let sel = [
            pkg(Manager::Scoop, "fd", Scope::User),
            pkg(Manager::Choco, "git", Scope::Machine),
            pkg(Manager::Choco, "7zip", Scope::Machine),
        ];
        assert_eq!(uac_prompts_needed(&sel, false), 1);
        assert_eq!(uac_prompts_needed(&sel, true), 0, "already elevated");
    }

    #[test]
    fn build_jobs_splits_queues_and_keeps_ids_only() {
        let sel = [
            pkg(Manager::Scoop, "fd", Scope::User),
            pkg(Manager::Choco, "git", Scope::Machine),
        ];
        let (user, sys) = build_jobs(&sel, false);
        let (user, sys) = (user.unwrap(), sys.unwrap());
        assert_eq!(user.items.len(), 1);
        assert_eq!(user.items[0].id, "fd");
        assert_eq!(sys.items[0].id, "git");
        assert_ne!(user.id, sys.id);
        let (u2, s2) = build_jobs(&sel[..1], false);
        assert!(u2.is_some() && s2.is_none());
        let (u3, s3) = build_jobs(&[], false);
        assert!(u3.is_none() && s3.is_none());
    }

    #[test]
    fn palette_filters_commands() {
        let upd = filter_commands("upd");
        assert_eq!(upd, vec![Cmd::Check, Cmd::UpdateSelected, Cmd::UpdateAll]);
        assert_eq!(filter_commands("LOG"), vec![Cmd::ToggleLog]);
        assert_eq!(filter_commands("").len(), COMMANDS.len());
        assert!(filter_commands("zzz").is_empty());
    }

    #[test]
    fn redacted_export_strips_user_profile_path() {
        let log = "installing to C:\\Users\\Sean\\scoop\\apps\\fd and c:\\users\\sean\\x";
        let r = redact(log, "C:\\Users\\Sean");
        assert!(!r.to_lowercase().contains("sean"), "{r}");
        assert!(r.contains("%USERPROFILE%\\scoop\\apps\\fd"));
    }

    #[test]
    fn empty_state_cases() {
        use crate::ui::model::MgrState::*;
        let none = [
            (Manager::Winget, Missing),
            (Manager::Choco, Missing),
            (Manager::Scoop, Missing),
        ];
        assert!(empty_state(&none, 0, false)
            .unwrap()
            .0
            .contains("No package manager"));
        let ok = [(Manager::Winget, Found(0))];
        assert!(empty_state(&ok, 0, false).unwrap().0.contains("up to date"));
        let failed = [
            (Manager::Winget, Failed("SWP-102 timeout".into())),
            (Manager::Choco, Missing),
        ];
        assert!(empty_state(&failed, 0, false)
            .unwrap()
            .0
            .contains("could not"));
        assert!(
            empty_state(&ok, 3, false).is_none(),
            "rows present: no empty state"
        );
        assert!(
            empty_state(&[(Manager::Winget, Checking)], 0, true).is_none(),
            "still checking"
        );
    }
}
