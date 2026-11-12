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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cmd {
    Check,
    UpdateSelected,
    UpdateAll,
    ToggleLog,
    ToggleDetails,
    Settings,
    ShowIgnored,
    SelectAll,
    SelectNone,
    FocusFilter,
    SaveLog,
    History,
    ShowHelp,
    ResetLayout,
    NextRegion,
    Cancel,
    /// Space on the focused list row. Not listed in the palette.
    ToggleRow,
    /// Opens the palette itself. Not listed in the palette.
    Palette,
}

/// Palette entries: verb-first label, shortcut (empty when none), extra search words.
pub const COMMANDS: &[(Cmd, &str, &str, &str)] = &[
    (
        Cmd::Check,
        "Check for updates",
        "F5",
        "refresh scan outdated",
    ),
    (
        Cmd::UpdateSelected,
        "Update selected packages",
        "Ctrl+U",
        "install upgrade",
    ),
    (
        Cmd::UpdateAll,
        "Update all packages",
        "Ctrl+Shift+U",
        "install upgrade everything",
    ),
    (Cmd::Cancel, "Cancel running work", "Esc", "stop abort"),
    (
        Cmd::SelectAll,
        "Select all packages",
        "Ctrl+A",
        "check every",
    ),
    (Cmd::SelectNone, "Clear selection", "", "uncheck none"),
    (Cmd::FocusFilter, "Find packages", "Ctrl+F", "filter search"),
    (
        Cmd::ShowIgnored,
        "Show or hide ignored packages",
        "Ctrl+I",
        "pinned skipped",
    ),
    (
        Cmd::ToggleDetails,
        "Show or hide details",
        "Ctrl+D",
        "inspector panel",
    ),
    (
        Cmd::ToggleLog,
        "Show or hide log",
        "Ctrl+L",
        "drawer output",
    ),
    (Cmd::SaveLog, "Save log to file", "Ctrl+S", "export"),
    (
        Cmd::Settings,
        "Open settings",
        "Ctrl+,",
        "preferences options theme timeout",
    ),
    (Cmd::NextRegion, "Move to next region", "F6", "focus panel"),
    (
        Cmd::ResetLayout,
        "Reset layout",
        "",
        "default window panels",
    ),
    (Cmd::ShowHelp, "Show keyboard shortcuts", "F1", "help keys"),
];

/// Fuzzy subsequence score: lower is better, `None` when `q` is not a subsequence.
/// Contiguous runs and early matches score better.
pub fn fuzzy_score(q: &str, text: &str) -> Option<u32> {
    let q: Vec<char> = q
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if q.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut qi, mut score, mut last) = (0usize, 0u32, None::<usize>);
    for (i, c) in t.iter().enumerate() {
        if qi < q.len() && *c == q[qi] {
            score += match last {
                None if i == 0 || t[i - 1] == ' ' => 0,
                None => i as u32 + 1,
                Some(l) => (i - l - 1) as u32 * 2,
            };
            last = Some(i);
            qi += 1;
        }
    }
    (qi == q.len()).then_some(score)
}

/// Commands matching `q` (label and keywords), recent ones first (most recent first),
/// then by best match. An empty query lists recent commands first, then the rest.
pub fn filter_commands(q: &str, recent: &[Cmd]) -> Vec<Cmd> {
    let mut hits: Vec<(Cmd, u32)> = COMMANDS
        .iter()
        .filter_map(|(c, label, _, kw)| {
            let by_label = fuzzy_score(q, label);
            let by_kw = fuzzy_score(q, kw).map(|s| s + 20);
            by_label.into_iter().chain(by_kw).min().map(|s| (*c, s))
        })
        .collect();
    let rank = |c: Cmd| recent.iter().position(|r| *r == c).unwrap_or(usize::MAX);
    hits.sort_by_key(|(c, s)| (rank(*c), *s));
    hits.into_iter().map(|(c, _)| c).collect()
}

/// Why a command cannot run right now, or `None` when it can.
pub fn disabled_reason(c: Cmd, selected: usize, total: usize, busy: bool) -> Option<&'static str> {
    match c {
        Cmd::UpdateSelected if busy => Some("work is running"),
        Cmd::UpdateSelected if selected == 0 => Some("no package is checked"),
        Cmd::UpdateAll if busy => Some("work is running"),
        Cmd::UpdateAll if total == 0 => Some("nothing to update"),
        Cmd::Check if busy => Some("work is running"),
        Cmd::Cancel if !busy => Some("nothing is running"),
        Cmd::SelectAll | Cmd::SelectNone if total == 0 => Some("the list is empty"),
        _ => None,
    }
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
    fn palette_is_fuzzy_and_ranks_best_match_first() {
        let upd = filter_commands("upd", &[]);
        let top: std::collections::HashSet<_> = upd[..3].iter().copied().collect();
        let want: std::collections::HashSet<_> =
            [Cmd::Check, Cmd::UpdateSelected, Cmd::UpdateAll].into();
        assert_eq!(top, want, "word-start matches lead");
        assert_eq!(filter_commands("LOG", &[])[0], Cmd::ToggleLog);
        assert!(
            filter_commands("svlg", &[]).contains(&Cmd::SaveLog),
            "subsequence"
        );
        assert_eq!(filter_commands("", &[]).len(), COMMANDS.len());
        assert!(filter_commands("zzzq", &[]).is_empty());
        assert!(
            filter_commands("preferences", &[]).contains(&Cmd::Settings),
            "keywords"
        );
    }

    #[test]
    fn palette_lists_recent_commands_first() {
        let all = filter_commands("", &[Cmd::SaveLog, Cmd::Settings]);
        assert_eq!(&all[..2], &[Cmd::SaveLog, Cmd::Settings]);
        let q = filter_commands("log", &[Cmd::SaveLog]);
        assert_eq!(q[0], Cmd::SaveLog, "recent beats a better match");
    }

    #[test]
    fn every_command_has_a_unique_label_and_shortcut() {
        let mut labels = std::collections::HashSet::new();
        let mut keys = std::collections::HashSet::new();
        for (_, l, k, _) in COMMANDS {
            assert!(labels.insert(*l), "dup label {l}");
            assert!(k.is_empty() || keys.insert(*k), "dup shortcut {k}");
        }
        for k in [
            "Ctrl+K", "Ctrl+,", "F1", "Esc", "F6", "Ctrl+F", "Ctrl+S", "Ctrl+A",
        ] {
            let bound = COMMANDS.iter().any(|(_, _, key, _)| *key == k);
            assert!(
                bound || k == "Ctrl+K",
                "reserved or required key {k} is unbound"
            );
        }
    }

    #[test]
    fn disabled_commands_explain_why() {
        assert_eq!(
            disabled_reason(Cmd::UpdateSelected, 0, 5, false),
            Some("no package is checked")
        );
        assert_eq!(disabled_reason(Cmd::UpdateSelected, 2, 5, false), None);
        assert_eq!(
            disabled_reason(Cmd::Cancel, 0, 0, false),
            Some("nothing is running")
        );
        assert_eq!(disabled_reason(Cmd::Cancel, 0, 0, true), None);
        assert_eq!(
            disabled_reason(Cmd::Check, 0, 0, true),
            Some("work is running")
        );
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
