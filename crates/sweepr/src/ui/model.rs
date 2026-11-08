//! List state for the main grid: rows, checks, filter and sort. No Win32 here.

use std::cmp::Ordering;
use std::collections::HashSet;
use sweepr_core::model::{Manager, Scope, UpdatePackage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Col {
    Name,
    Id,
    Current,
    Available,
    Source,
    Scope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub col: Col,
    pub asc: bool,
}

/// Per-manager discovery state, always shown as text (never color only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MgrState {
    Checking,
    Found(usize),
    Missing,
    Failed(String),
    Cancelled,
}

fn mgr_label(m: Manager) -> &'static str {
    source_label(m)
}

pub fn status_line(states: &[(Manager, MgrState)], total: usize, selected: usize) -> String {
    let parts: Vec<String> = states
        .iter()
        .map(|(m, s)| {
            let t = match s {
                MgrState::Checking => "checking".to_string(),
                MgrState::Found(n) => format!("{n} found"),
                MgrState::Missing => "not installed".to_string(),
                MgrState::Failed(e) => e.clone(),
                MgrState::Cancelled => "cancelled".to_string(),
            };
            format!("{}: {t}", mgr_label(*m))
        })
        .collect();
    format!("{} | {selected} of {total} selected", parts.join(" | "))
}

pub struct ListModel {
    all: Vec<UpdatePackage>,
    checked: HashSet<(Manager, String)>,
    visible: Vec<usize>,
    sort: Sort,
    filter: String,
    show_ignored: bool,
}

impl Default for ListModel {
    fn default() -> Self {
        ListModel {
            all: Vec::new(),
            checked: HashSet::new(),
            visible: Vec::new(),
            sort: Sort {
                col: Col::Name,
                asc: true,
            },
            filter: String::new(),
            show_ignored: false,
        }
    }
}

/// Compares strings with digit runs ordered by value, ignoring case.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    fn chunks(s: &str) -> Vec<(bool, String)> {
        let mut v: Vec<(bool, String)> = Vec::new();
        for c in s.chars() {
            let d = c.is_ascii_digit();
            match v.last_mut() {
                Some((ld, t)) if *ld == d => t.push(c.to_ascii_lowercase()),
                _ => v.push((d, c.to_ascii_lowercase().to_string())),
            }
        }
        v
    }
    let (ca, cb) = (chunks(a), chunks(b));
    for (x, y) in ca.iter().zip(cb.iter()) {
        let o = if x.0 && y.0 {
            let (tx, ty) = (x.1.trim_start_matches('0'), y.1.trim_start_matches('0'));
            tx.len().cmp(&ty.len()).then_with(|| tx.cmp(ty))
        } else {
            x.1.cmp(&y.1)
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    ca.len().cmp(&cb.len())
}

pub fn source_label(m: Manager) -> &'static str {
    match m {
        Manager::Winget => "WinGet",
        Manager::Choco => "Choco",
        Manager::Scoop => "Scoop",
    }
}

pub fn scope_label(s: Scope) -> &'static str {
    match s {
        Scope::Machine => "Machine",
        Scope::User => "User",
        Scope::Unknown => "Unknown",
    }
}

fn cell(p: &UpdatePackage, col: Col) -> String {
    match col {
        Col::Name => p.name.clone(),
        Col::Id => p.id.clone(),
        Col::Current => p.current_version.clone(),
        Col::Available => p.available_version.clone(),
        Col::Source => source_label(p.manager).to_string(),
        Col::Scope => scope_label(p.scope).to_string(),
    }
}

impl ListModel {
    /// Replaces every row of one manager. New rows start checked unless ignored or pinned
    /// by the manager; rows that were already present keep their check state.
    pub fn replace_manager(&mut self, m: Manager, pkgs: Vec<UpdatePackage>) {
        let old: HashSet<(Manager, String)> = self
            .all
            .iter()
            .filter(|p| p.manager == m)
            .map(|p| (p.manager, p.id.clone()))
            .collect();
        self.all.retain(|p| p.manager != m);
        let keep: HashSet<(Manager, String)> =
            pkgs.iter().map(|p| (p.manager, p.id.clone())).collect();
        self.checked.retain(|k| k.0 != m || keep.contains(k));
        for p in pkgs {
            let key = (p.manager, p.id.clone());
            if !old.contains(&key) && !p.ignored && !p.pinned_by_manager {
                self.checked.insert(key);
            }
            self.all.push(p);
        }
        self.rebuild();
    }

    pub fn set_filter(&mut self, f: &str) {
        self.filter = f.to_lowercase();
        self.rebuild();
    }

    pub fn set_sort(&mut self, s: Sort) {
        self.sort = s;
        self.rebuild();
    }

    pub fn set_show_ignored(&mut self, on: bool) {
        self.show_ignored = on;
        self.rebuild();
    }

    pub fn sort(&self) -> Sort {
        self.sort
    }

    fn rebuild(&mut self) {
        let f = &self.filter;
        let mut v: Vec<usize> = (0..self.all.len())
            .filter(|&i| {
                let p = &self.all[i];
                (self.show_ignored || !p.ignored)
                    && (f.is_empty()
                        || p.name.to_lowercase().contains(f)
                        || p.id.to_lowercase().contains(f))
            })
            .collect();
        let (col, asc) = (self.sort.col, self.sort.asc);
        v.sort_by(|&a, &b| {
            let o = natural_cmp(&cell(&self.all[a], col), &cell(&self.all[b], col));
            if asc {
                o
            } else {
                o.reverse()
            }
        });
        self.visible = v;
    }

    pub fn visible_len(&self) -> usize {
        self.visible.len()
    }

    pub fn visible(&self, i: usize) -> &UpdatePackage {
        &self.all[self.visible[i]]
    }

    pub fn text(&self, i: usize, col: Col) -> String {
        cell(self.visible(i), col)
    }

    pub fn is_checked(&self, i: usize) -> bool {
        let p = self.visible(i);
        !p.ignored && self.checked.contains(&(p.manager, p.id.clone()))
    }

    /// Flips the check on a visible row. Ignored rows cannot be checked.
    pub fn toggle(&mut self, i: usize) {
        let p = self.visible(i);
        if p.ignored {
            return;
        }
        let key = (p.manager, p.id.clone());
        if !self.checked.remove(&key) {
            self.checked.insert(key);
        }
    }

    /// Checked, non-ignored rows (including rows hidden by the filter).
    pub fn selected(&self) -> Vec<UpdatePackage> {
        self.all
            .iter()
            .filter(|p| !p.ignored && self.checked.contains(&(p.manager, p.id.clone())))
            .cloned()
            .collect()
    }

    pub fn total(&self) -> usize {
        self.all.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sweepr_core::model::{Manager, Scope, UpdatePackage};

    fn pkg(m: Manager, id: &str, name: &str, cur: &str, new: &str) -> UpdatePackage {
        UpdatePackage {
            id: id.into(),
            name: name.into(),
            current_version: cur.into(),
            available_version: new.into(),
            manager: m,
            scope: Scope::User,
            elevation_required: false,
            pinned_by_manager: false,
            ignored: false,
            user_pin: None,
        }
    }

    fn sample() -> ListModel {
        let mut m = ListModel::default();
        m.replace_manager(
            Manager::Winget,
            vec![
                pkg(
                    Manager::Winget,
                    "Google.Chrome",
                    "Google Chrome",
                    "120",
                    "121",
                ),
                pkg(Manager::Winget, "7zip.7zip", "7-Zip", "23.01", "24.07"),
            ],
        );
        m.replace_manager(
            Manager::Scoop,
            vec![pkg(
                Manager::Scoop,
                "ripgrep",
                "ripgrep",
                "14.0.3",
                "14.1.0",
            )],
        );
        m
    }

    #[test]
    fn list_model_filter_and_sort() {
        let mut m = sample();
        assert_eq!(m.visible_len(), 3);
        m.set_sort(Sort {
            col: Col::Name,
            asc: true,
        });
        assert_eq!(m.visible(0).name, "7-Zip");
        m.set_sort(Sort {
            col: Col::Name,
            asc: false,
        });
        assert_eq!(m.visible(0).name, "ripgrep");
        m.set_filter("CHROME");
        assert_eq!(m.visible_len(), 1);
        assert_eq!(m.visible(0).id, "Google.Chrome");
        m.set_filter("");
        assert_eq!(m.visible_len(), 3);
    }

    #[test]
    fn versions_sort_naturally() {
        let mut m = ListModel::default();
        m.replace_manager(
            Manager::Choco,
            vec![
                pkg(Manager::Choco, "a", "a", "10.0", "x"),
                pkg(Manager::Choco, "b", "b", "9.0", "x"),
            ],
        );
        m.set_sort(Sort {
            col: Col::Current,
            asc: true,
        });
        assert_eq!(m.visible(0).id, "b");
    }

    #[test]
    fn selected_excludes_ignored_and_unchecked() {
        let mut m = ListModel::default();
        let mut ig = pkg(Manager::Choco, "git", "git", "1", "2");
        ig.ignored = true;
        m.replace_manager(
            Manager::Choco,
            vec![ig, pkg(Manager::Choco, "7zip", "7zip", "1", "2")],
        );
        assert_eq!(m.visible_len(), 1, "ignored hidden by default");
        assert_eq!(m.selected().len(), 1);
        m.set_show_ignored(true);
        m.set_sort(Sort {
            col: Col::Id,
            asc: true,
        });
        assert_eq!(m.visible_len(), 2);
        assert_eq!(m.selected().len(), 1, "ignored never selected");
        m.toggle(1); // the ignored row: toggling must not select it
        assert_eq!(m.selected().len(), 1);
        m.toggle(0); // 7zip unchecked
        assert!(m.selected().is_empty());
    }

    #[test]
    fn replace_manager_only_touches_that_manager_and_keeps_checks() {
        let mut m = sample();
        m.set_sort(Sort {
            col: Col::Id,
            asc: true,
        });
        let idx = (0..m.visible_len())
            .find(|&i| m.visible(i).id == "ripgrep")
            .unwrap();
        m.toggle(idx); // uncheck ripgrep
        m.replace_manager(
            Manager::Winget,
            vec![pkg(
                Manager::Winget,
                "Google.Chrome",
                "Google Chrome",
                "120",
                "122",
            )],
        );
        assert_eq!(m.visible_len(), 2);
        let sel: Vec<String> = m.selected().into_iter().map(|p| p.id).collect();
        assert_eq!(sel, vec!["Google.Chrome".to_string()]);
    }

    #[test]
    fn cell_text_per_column() {
        let m = sample();
        let p = m.visible(0).clone();
        assert_eq!(m.text(0, Col::Id), p.id);
        assert_eq!(
            m.text(0, Col::Source),
            if p.manager == Manager::Winget {
                "WinGet"
            } else {
                "Scoop"
            }
        );
        assert_eq!(m.text(0, Col::Scope), "User");
    }

    #[test]
    fn status_line_has_text_for_every_state() {
        let st = [
            (Manager::Winget, MgrState::Found(3)),
            (Manager::Choco, MgrState::Checking),
            (Manager::Scoop, MgrState::Missing),
        ];
        let s = status_line(&st, 3, 2);
        assert_eq!(
            s,
            "WinGet: 3 found | Choco: checking | Scoop: not installed | 2 of 3 selected"
        );
        let f = status_line(
            &[(Manager::Scoop, MgrState::Failed("SWP-102 timeout".into()))],
            0,
            0,
        );
        assert!(f.contains("Scoop: SWP-102 timeout"), "{f}");
    }
}
