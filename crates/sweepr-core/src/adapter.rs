//! One adapter per manager: fixed command templates, no shell, validated ids.

use crate::errors::SwpError;
use crate::model::{Manager, Scope, UpdatePackage};
use crate::parse;
use crate::validate::valid_package_id;
use std::path::PathBuf;

pub trait PackageManager: Send + Sync {
    fn manager(&self) -> Manager;
    /// Absolute path of the manager executable, from fixed locations then `PATH`.
    fn detect(&self) -> Option<PathBuf>;
    fn discovery_args(&self) -> Vec<String>;
    fn parse(&self, out: &str) -> Result<Vec<UpdatePackage>, SwpError>;
    fn upgrade_args(&self, p: &UpdatePackage) -> Result<Vec<String>, SwpError>;
    /// Arguments for `--dry-run` display. Defaults to the real arguments.
    fn dry_run_args(&self, p: &UpdatePackage) -> Result<Vec<String>, SwpError> {
        self.upgrade_args(p)
    }
}

fn s(a: &[&str]) -> Vec<String> {
    a.iter().map(|x| x.to_string()).collect()
}

fn checked_id(p: &UpdatePackage) -> Result<&str, SwpError> {
    if valid_package_id(&p.id) {
        Ok(&p.id)
    } else {
        Err(SwpError::JobRejected(format!(
            "invalid package id: {:?}",
            p.id
        )))
    }
}

pub fn find_in_dirs(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter().map(|d| d.join(name)).find(|p| p.is_file())
}

fn path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default()
}

fn env_dir(var: &str, rel: &str, fallback: &str) -> PathBuf {
    let base = std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(fallback));
    base.join(rel)
}

pub struct WingetAdapter;
pub struct ChocoAdapter;
pub struct ScoopAdapter;

impl PackageManager for WingetAdapter {
    fn manager(&self) -> Manager {
        Manager::Winget
    }
    fn detect(&self) -> Option<PathBuf> {
        let fixed = env_dir(
            "LOCALAPPDATA",
            "Microsoft\\WindowsApps",
            "C:\\Users\\Default\\AppData\\Local",
        );
        find_in_dirs("winget.exe", &[fixed]).or_else(|| find_in_dirs("winget.exe", &path_dirs()))
    }
    fn discovery_args(&self) -> Vec<String> {
        s(&[
            "upgrade",
            "--accept-source-agreements",
            "--disable-interactivity",
        ])
    }
    fn parse(&self, out: &str) -> Result<Vec<UpdatePackage>, SwpError> {
        parse::parse_winget(out)
    }
    fn upgrade_args(&self, p: &UpdatePackage) -> Result<Vec<String>, SwpError> {
        let id = checked_id(p)?;
        Ok(s(&[
            "upgrade",
            "--id",
            id,
            "--exact",
            "--silent",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--disable-interactivity",
        ]))
    }
}

impl PackageManager for ChocoAdapter {
    fn manager(&self) -> Manager {
        Manager::Choco
    }
    fn detect(&self) -> Option<PathBuf> {
        let fixed = env_dir("ProgramData", "chocolatey\\bin", "C:\\ProgramData");
        find_in_dirs("choco.exe", &[fixed]).or_else(|| find_in_dirs("choco.exe", &path_dirs()))
    }
    fn discovery_args(&self) -> Vec<String> {
        s(&["outdated", "--limit-output"])
    }
    fn parse(&self, out: &str) -> Result<Vec<UpdatePackage>, SwpError> {
        parse::parse_choco(out)
    }
    fn upgrade_args(&self, p: &UpdatePackage) -> Result<Vec<String>, SwpError> {
        Ok(s(&["upgrade", checked_id(p)?, "-y"]))
    }
    fn dry_run_args(&self, p: &UpdatePackage) -> Result<Vec<String>, SwpError> {
        let mut a = self.upgrade_args(p)?;
        a.push("--noop".into());
        Ok(a)
    }
}

impl PackageManager for ScoopAdapter {
    fn manager(&self) -> Manager {
        Manager::Scoop
    }
    fn detect(&self) -> Option<PathBuf> {
        let fixed = env_dir("USERPROFILE", "scoop\\shims", "C:\\Users\\Default");
        find_in_dirs("scoop.cmd", &[fixed]).or_else(|| find_in_dirs("scoop.cmd", &path_dirs()))
    }
    fn discovery_args(&self) -> Vec<String> {
        s(&["status"])
    }
    fn parse(&self, out: &str) -> Result<Vec<UpdatePackage>, SwpError> {
        parse::parse_scoop(out)
    }
    fn upgrade_args(&self, p: &UpdatePackage) -> Result<Vec<String>, SwpError> {
        let mut a = s(&["update", checked_id(p)?]);
        if p.scope == Scope::Machine {
            a.push("-g".into());
        }
        Ok(a)
    }
}

pub fn all_adapters() -> Vec<Box<dyn PackageManager>> {
    vec![
        Box::new(WingetAdapter),
        Box::new(ChocoAdapter),
        Box::new(ScoopAdapter),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(m: Manager, id: &str, scope: Scope) -> UpdatePackage {
        UpdatePackage {
            id: id.into(),
            name: id.into(),
            current_version: "1".into(),
            available_version: "2".into(),
            manager: m,
            scope,
            elevation_required: false,
            pinned_by_manager: false,
            ignored: false,
            user_pin: None,
        }
    }
    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn discovery_args_match_spec() {
        assert_eq!(
            WingetAdapter.discovery_args(),
            v(&[
                "upgrade",
                "--accept-source-agreements",
                "--disable-interactivity"
            ])
        );
        assert_eq!(
            ChocoAdapter.discovery_args(),
            v(&["outdated", "--limit-output"])
        );
        assert_eq!(ScoopAdapter.discovery_args(), v(&["status"]));
    }

    #[test]
    fn winget_upgrade_args_exact() {
        let a = WingetAdapter
            .upgrade_args(&pkg(Manager::Winget, "Google.Chrome", Scope::Machine))
            .unwrap();
        assert_eq!(
            a,
            v(&[
                "upgrade",
                "--id",
                "Google.Chrome",
                "--exact",
                "--silent",
                "--accept-package-agreements",
                "--accept-source-agreements",
                "--disable-interactivity"
            ])
        );
    }

    #[test]
    fn choco_upgrade_args_are_y_only() {
        let a = ChocoAdapter
            .upgrade_args(&pkg(Manager::Choco, "git", Scope::Machine))
            .unwrap();
        assert_eq!(a, v(&["upgrade", "git", "-y"]));
    }

    #[test]
    fn choco_dry_run_adds_noop() {
        let a = ChocoAdapter
            .dry_run_args(&pkg(Manager::Choco, "git", Scope::Machine))
            .unwrap();
        assert_eq!(a, v(&["upgrade", "git", "-y", "--noop"]));
    }

    #[test]
    fn scoop_global_adds_g() {
        let u = ScoopAdapter
            .upgrade_args(&pkg(Manager::Scoop, "fd", Scope::User))
            .unwrap();
        assert_eq!(u, v(&["update", "fd"]));
        let g = ScoopAdapter
            .upgrade_args(&pkg(Manager::Scoop, "fd", Scope::Machine))
            .unwrap();
        assert_eq!(g, v(&["update", "fd", "-g"]));
    }

    #[test]
    fn hostile_id_rejected_in_upgrade_args() {
        for (ad, m) in [
            (
                Box::new(WingetAdapter) as Box<dyn PackageManager>,
                Manager::Winget,
            ),
            (Box::new(ChocoAdapter), Manager::Choco),
            (Box::new(ScoopAdapter), Manager::Scoop),
        ] {
            let e = ad
                .upgrade_args(&pkg(m, "foo & calc", Scope::User))
                .unwrap_err();
            assert_eq!(e.code(), "SWP-203");
        }
    }

    #[test]
    fn all_adapters_has_three_distinct_managers() {
        let mut m: Vec<_> = all_adapters().iter().map(|a| a.manager()).collect();
        m.sort();
        m.dedup();
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn find_in_dirs_returns_first_hit() {
        let base = std::env::temp_dir().join(format!("sweepr-adapter-{}", std::process::id()));
        let (a, b) = (base.join("a"), base.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("tool.exe"), "").unwrap();
        assert_eq!(
            find_in_dirs("tool.exe", &[a.clone(), b.clone()]),
            Some(b.join("tool.exe"))
        );
        assert_eq!(find_in_dirs("nope.exe", &[a, b]), None);
    }
}
