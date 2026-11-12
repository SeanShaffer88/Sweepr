//! Pure presentation rules from TesselGUI: status severity and wording, details panel
//! content, determinate progress segments, hint bar and inspector width. No Win32 here.

use super::actions::Cmd;
use super::geometry::to_device;
use super::model::{scope_label, source_label, status_line, MgrState};
use super::theme::Severity;
use sweepr_core::errors::SwpError;
use sweepr_core::model::{Manager, UpdatePackage};

/// What happened, why, and what to do (TesselGUI error anatomy). The code comes last.
pub fn failure_text(e: &SwpError) -> String {
    match e {
        SwpError::Timeout(_) => format!(
            "took too long to answer. Raise the timeout in Settings, then check again ({}).",
            e.code()
        ),
        SwpError::ParseFailed { .. } => format!(
            "gave output Sweepr could not read. Open the log (Ctrl+L) to see it, then check again ({}).",
            e.code()
        ),
        _ => format!("failed. Open the log (Ctrl+L) for details ({}).", e.code()),
    }
}

/// Severity and one-line text for the status bar. Text always carries the meaning;
/// the glyph and color only support it.
pub fn summary(
    states: &[(Manager, MgrState)],
    total: usize,
    selected: usize,
) -> (Severity, String) {
    let sev = if states.iter().any(|(_, s)| matches!(s, MgrState::Failed(_))) {
        Severity::Error
    } else if states.iter().any(|(_, s)| *s == MgrState::Cancelled)
        || (!states.is_empty() && states.iter().all(|(_, s)| *s == MgrState::Missing))
    {
        Severity::Warning
    } else if states.iter().any(|(_, s)| *s == MgrState::Checking) {
        Severity::Info
    } else if total == 0 && !states.is_empty() {
        Severity::Success
    } else {
        Severity::Info
    };
    (sev, status_line(states, total, selected))
}

/// Rows for the details panel: label and value. Empty values never appear.
pub fn detail_lines(p: &UpdatePackage) -> Vec<(&'static str, String)> {
    let access = if p.elevation_required {
        "Needs administrator rights (one UAC prompt per batch)"
    } else {
        "No administrator rights needed"
    };
    let rule = if p.ignored {
        "Ignored: skipped by Update all".to_string()
    } else if let Some(v) = &p.user_pin {
        format!("Pinned: skips version {v}")
    } else if p.pinned_by_manager {
        "Held by the package manager".to_string()
    } else {
        "None".to_string()
    };
    vec![
        ("Name", p.name.clone()),
        ("Id", p.id.clone()),
        ("Source", source_label(p.manager).to_string()),
        ("Scope", scope_label(p.scope).to_string()),
        ("Installed", p.current_version.clone()),
        ("Available", p.available_version.clone()),
        ("Access", access.to_string()),
        ("Rule", rule),
    ]
}

/// Details panel text; `None` shows the empty hint.
pub fn details_text(p: Option<&UpdatePackage>) -> String {
    match p {
        None => "Select a package to see its details.\r\nSpace checks it for updating.".to_string(),
        Some(p) => detail_lines(p)
            .into_iter()
            .map(|(k, v)| format!("{k}\r\n{v}"))
            .collect::<Vec<_>>()
            .join("\r\n\r\n"),
    }
}

/// Filled segments of the ten-segment determinate progress bar.
pub fn segments(done: usize, total: usize) -> usize {
    (done.min(total) * 10).checked_div(total).unwrap_or(0)
}

/// Progress text with units, e.g. "3 of 8 packages".
pub fn progress_text(done: usize, total: usize, unit: &str) -> String {
    format!("{} of {} {}", done.min(total), total, unit)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    Toolbar,
    List,
    Details,
    Log,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hint {
    pub key: &'static str,
    pub action: &'static str,
    pub cmd: Cmd,
}

const fn h(key: &'static str, action: &'static str, cmd: Cmd) -> Hint {
    Hint { key, action, cmd }
}

/// Up to six hints for the focused region. While work runs, Esc Cancel leads.
pub fn hints(region: Region, busy: bool) -> Vec<Hint> {
    let mut v = match region {
        Region::Toolbar => vec![
            h("Ctrl+F", "Find", Cmd::FocusFilter),
            h("F5", "Check", Cmd::Check),
            h("Ctrl+U", "Update", Cmd::UpdateSelected),
            h("Ctrl+,", "Settings", Cmd::Settings),
            h("Ctrl+K", "Commands", Cmd::Palette),
            h("F1", "Help", Cmd::ShowHelp),
        ],
        Region::List => vec![
            h("Space", "Check row", Cmd::ToggleRow),
            h("Ctrl+A", "Select all", Cmd::SelectAll),
            h("Ctrl+U", "Update", Cmd::UpdateSelected),
            h("F5", "Check", Cmd::Check),
            h("Ctrl+D", "Details", Cmd::ToggleDetails),
            h("Ctrl+K", "Commands", Cmd::Palette),
        ],
        Region::Details => vec![
            h("Ctrl+D", "Hide details", Cmd::ToggleDetails),
            h("F6", "Next region", Cmd::NextRegion),
            h("Ctrl+F", "Find", Cmd::FocusFilter),
            h("Ctrl+U", "Update", Cmd::UpdateSelected),
            h("Ctrl+K", "Commands", Cmd::Palette),
            h("F1", "Help", Cmd::ShowHelp),
        ],
        Region::Log => vec![
            h("Ctrl+S", "Save log", Cmd::SaveLog),
            h("Ctrl+L", "Hide log", Cmd::ToggleLog),
            h("F6", "Next region", Cmd::NextRegion),
            h("Ctrl+F", "Find", Cmd::FocusFilter),
            h("Ctrl+K", "Commands", Cmd::Palette),
            h("F1", "Help", Cmd::ShowHelp),
        ],
    };
    if busy {
        v.insert(0, h("Esc", "Cancel", Cmd::Cancel));
        v.truncate(6);
    }
    v
}

/// Next region in the F6 cycle, skipping closed panels.
pub fn next_region(cur: Region, details_open: bool, log_open: bool) -> Region {
    let order = [Region::Toolbar, Region::List, Region::Details, Region::Log];
    let open = |r: Region| match r {
        Region::Details => details_open,
        Region::Log => log_open,
        _ => true,
    };
    let start = order.iter().position(|r| *r == cur).unwrap_or(0);
    (1..=order.len())
        .map(|k| order[(start + k) % order.len()])
        .find(|r| open(*r))
        .unwrap_or(Region::List)
}

/// Details panel width in device pixels: 300 logical, kept inside 240..=420 logical and
/// inside 20..=35% of the client width, so the list stays the dominant 55..=70%+ region.
pub fn inspector_width(client_w: i32, dpi: u32) -> i32 {
    let want = to_device(300, dpi).clamp(to_device(240, dpi), to_device(420, dpi));
    let lo = client_w * 20 / 100;
    let hi = client_w * 35 / 100;
    want.clamp(lo.min(hi), hi)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sweepr_core::model::Scope;

    fn pkg() -> UpdatePackage {
        UpdatePackage {
            id: "Git.Git".into(),
            name: "Git".into(),
            current_version: "2.43".into(),
            available_version: "2.44".into(),
            manager: Manager::Winget,
            scope: Scope::Machine,
            elevation_required: true,
            pinned_by_manager: false,
            ignored: false,
            user_pin: None,
        }
    }

    #[test]
    fn failures_say_what_why_and_what_to_do() {
        let t = failure_text(&SwpError::Timeout(Manager::Choco));
        assert!(t.contains("took too long") && t.contains("Settings") && t.ends_with("(SWP-102)."));
        let p = failure_text(&SwpError::ParseFailed {
            manager: Manager::Winget,
            detail: "x".into(),
        });
        assert!(p.contains("log") && p.contains("SWP-103"));
        assert!(
            !failure_text(&SwpError::PipeLost).starts_with("SWP"),
            "sentence first"
        );
    }

    #[test]
    fn severity_follows_state() {
        use MgrState::*;
        let w = Manager::Winget;
        assert_eq!(summary(&[(w, Failed("x".into()))], 0, 0).0, Severity::Error);
        assert_eq!(summary(&[(w, Cancelled)], 0, 0).0, Severity::Warning);
        assert_eq!(summary(&[(w, Missing)], 0, 0).0, Severity::Warning);
        assert_eq!(summary(&[(w, Checking)], 0, 0).0, Severity::Info);
        assert_eq!(summary(&[(w, Found(0))], 0, 0).0, Severity::Success);
        assert_eq!(summary(&[(w, Found(2))], 2, 0).0, Severity::Info);
    }

    #[test]
    fn details_show_access_and_rules() {
        let mut p = pkg();
        let d = detail_lines(&p);
        assert_eq!(d[0], ("Name", "Git".to_string()));
        assert!(d.iter().any(|(k, v)| *k == "Access" && v.contains("UAC")));
        p.user_pin = Some("2.44".into());
        assert!(detail_lines(&p)
            .iter()
            .any(|(k, v)| *k == "Rule" && v.contains("skips version 2.44")));
        p.ignored = true;
        assert!(detail_lines(&p)
            .iter()
            .any(|(_, v)| v.starts_with("Ignored")));
        assert!(details_text(None).contains("Select a package"));
        assert!(details_text(Some(&pkg())).contains("Id\r\nGit.Git"));
    }

    #[test]
    fn progress_has_ten_segments_and_units() {
        assert_eq!(segments(0, 8), 0);
        assert_eq!(segments(4, 8), 5);
        assert_eq!(segments(8, 8), 10);
        assert_eq!(segments(9, 8), 10, "never over");
        assert_eq!(segments(1, 0), 0);
        assert_eq!(progress_text(3, 8, "packages"), "3 of 8 packages");
    }

    #[test]
    fn hint_bar_has_at_most_six_real_commands() {
        for r in [Region::Toolbar, Region::List, Region::Details, Region::Log] {
            for busy in [false, true] {
                let v = hints(r, busy);
                assert!(!v.is_empty() && v.len() <= 6, "{r:?} {busy}");
                assert_eq!(v[0].cmd == Cmd::Cancel, busy);
            }
        }
    }

    #[test]
    fn f6_cycles_open_regions_only() {
        assert_eq!(next_region(Region::Toolbar, true, true), Region::List);
        assert_eq!(next_region(Region::List, true, true), Region::Details);
        assert_eq!(next_region(Region::List, false, true), Region::Log);
        assert_eq!(next_region(Region::Log, true, true), Region::Toolbar);
        assert_eq!(next_region(Region::List, false, false), Region::Toolbar);
    }

    #[test]
    fn list_stays_dominant() {
        for (w, dpi) in [(900, 96), (1000, 96), (1600, 96), (2400, 192), (1500, 144)] {
            let cw = to_device(w, dpi);
            let ins = inspector_width(cw, dpi);
            let list_pct = (cw - ins) * 100 / cw;
            assert!(list_pct >= 55, "w={w} dpi={dpi} list={list_pct}%");
            assert!(
                ins * 100 / cw >= 20 || ins >= to_device(240, dpi) - 1,
                "w={w} ins={ins}"
            );
        }
        assert_eq!(inspector_width(1000, 96), 300);
    }
}
