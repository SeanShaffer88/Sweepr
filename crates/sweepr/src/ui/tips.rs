//! Tooltip text for every interactive control and column header. Each says what the
//! control does and its side effects (TesselGUI / charter usability rule).

use super::model::Col;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ctl {
    Check,
    UpdateSelected,
    UpdateAll,
    Filter,
    SrcWinget,
    SrcChoco,
    SrcScoop,
    ShowIgnored,
    Settings,
    LogToggle,
    LogCopy,
    LogSave,
    Cancel,
    List,
    Palette,
    DetailsToggle,
}

#[cfg(test)]
pub const ALL_CTLS: [Ctl; 16] = [
    Ctl::Check,
    Ctl::UpdateSelected,
    Ctl::UpdateAll,
    Ctl::Filter,
    Ctl::SrcWinget,
    Ctl::SrcChoco,
    Ctl::SrcScoop,
    Ctl::ShowIgnored,
    Ctl::Settings,
    Ctl::LogToggle,
    Ctl::LogCopy,
    Ctl::LogSave,
    Ctl::Cancel,
    Ctl::List,
    Ctl::Palette,
    Ctl::DetailsToggle,
];

pub fn tooltip(c: Ctl) -> &'static str {
    match c {
        Ctl::Check => "Check for updates (F5). Runs the enabled package managers' own outdated-list commands in parallel. Installs nothing.",
        Ctl::UpdateSelected => "Update the checked packages (Ctrl+U). Machine-wide installs share one UAC prompt; user installs run without elevation.",
        Ctl::UpdateAll => "Update every non-ignored package (Ctrl+Shift+U). Skips ignored items. Machine-wide installs share one UAC prompt.",
        Ctl::Filter => "Filter the list by name or id as you type. Only changes what is shown; checks are kept.",
        Ctl::SrcWinget => "Include WinGet in checks. Off means WinGet is never run.",
        Ctl::SrcChoco => "Include Chocolatey in checks. Off means Chocolatey is never run. Its updates always need admin rights.",
        Ctl::SrcScoop => "Include Scoop in checks. Off means Scoop is never run. Scoop updates need no admin rights.",
        Ctl::ShowIgnored => "Show packages on your ignore list or pinned to a skipped version. They stay excluded from Update all.",
        Ctl::Settings => "Open settings (Ctrl+,): timeout, theme, sources and data location. Changes apply on the next check.",
        Ctl::LogToggle => "Show or hide the live log drawer (Ctrl+L) with the managers' exact stdout and stderr.",
        Ctl::LogCopy => "Copy the whole log to the clipboard as plain text.",
        Ctl::LogSave => "Save the log to a text file. Your user profile path is replaced with a placeholder.",
        Ctl::Cancel => "Cancel the running check or update (Esc). Running installers may finish their current package.",
        Ctl::List => "Outdated packages. Space toggles the check, click a header to sort, right-click for ignore, pin and copy.",
        Ctl::Palette => "Open the command palette (Ctrl+K) to run any command by name.",
        Ctl::DetailsToggle => "Show or hide the details panel (Ctrl+D) for the selected package.",
    }
}

pub fn column_tooltip(c: Col) -> &'static str {
    match c {
        Col::Name => "Name: click to sort by display name. Sorting only reorders the list.",
        Col::Id => "Id: the package id the manager uses. Click to sort by id.",
        Col::Current => "Current: installed version. Click to sort; digits sort by value.",
        Col::Available => "Available: newest version the manager offers. Click to sort.",
        Col::Source => "Source: which manager supplies the update (WinGet, Choco, Scoop). Click to sort.",
        Col::Scope => "Scope: Machine needs admin rights (UAC), User does not, Unknown is decided by the manager. Click to sort.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::model::Col;

    #[test]
    fn tooltip_table_covers_every_control_id() {
        let mut seen = std::collections::HashSet::new();
        for c in ALL_CTLS {
            let t = tooltip(c);
            assert!(t.len() > 10, "{c:?} has no real tooltip");
            assert!(seen.insert(t), "duplicate tooltip text for {c:?}");
        }
        assert_eq!(ALL_CTLS.len(), 16);
    }

    #[test]
    fn every_column_header_has_a_tooltip() {
        for c in [
            Col::Name,
            Col::Id,
            Col::Current,
            Col::Available,
            Col::Source,
            Col::Scope,
        ] {
            assert!(column_tooltip(c).contains("sort"), "{c:?}");
        }
    }

    #[test]
    fn update_buttons_name_their_side_effects() {
        assert!(tooltip(Ctl::UpdateSelected).contains("UAC"));
        assert!(tooltip(Ctl::UpdateAll).contains("ignored"));
    }
}
