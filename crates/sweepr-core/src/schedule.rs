//! Optional daily check through Windows Task Scheduler. The task only runs
//! `sweepr --scheduled-check`, which checks and never installs. Commands are fixed
//! argument vectors for `schtasks.exe`, never a shell string.

use serde::{Deserialize, Serialize};
use std::path::Path;

pub const TASK_NAME: &str = "Sweepr\\Daily check";
const FILE: &str = "last_check.json";

/// `HH:MM`, 24-hour.
pub fn valid_time(t: &str) -> bool {
    let b = t.as_bytes();
    if b.len() != 5 || b[2] != b':' {
        return false;
    }
    let (h, m) = (&t[0..2], &t[3..5]);
    h.bytes().all(|c| c.is_ascii_digit())
        && m.bytes().all(|c| c.is_ascii_digit())
        && h.parse::<u8>().is_ok_and(|h| h < 24)
        && m.parse::<u8>().is_ok_and(|m| m < 60)
}

/// Arguments for `schtasks.exe` that create (or replace) the daily task.
pub fn create_args(exe: &str, time: &str) -> Result<Vec<String>, String> {
    if !valid_time(time) {
        return Err(format!("time must be HH:MM, got {time:?}"));
    }
    if exe.is_empty() || exe.contains('"') || exe.contains('\n') || exe.contains('\r') {
        return Err("program path has characters a task cannot hold".into());
    }
    Ok(vec![
        "/Create".into(),
        "/TN".into(),
        TASK_NAME.into(),
        "/TR".into(),
        format!("\"{exe}\" --scheduled-check"),
        "/SC".into(),
        "DAILY".into(),
        "/ST".into(),
        time.into(),
        "/F".into(),
    ])
}

pub fn delete_args() -> Vec<String> {
    vec![
        "/Delete".into(),
        "/TN".into(),
        TASK_NAME.into(),
        "/F".into(),
    ]
}

/// Runs `schtasks.exe` with the given arguments. Windows only; elsewhere it reports so.
pub fn run_schtasks(args: &[String]) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("Task Scheduler is Windows only".into());
    }
    let out = std::process::Command::new("schtasks.exe")
        .args(args)
        .output()
        .map_err(|e| format!("could not run schtasks.exe: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "schtasks.exe failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// What the last scheduled check saw.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastCheck {
    pub ts: u64,
    pub count: usize,
    /// `manager:id@available` for each outdated package, sorted.
    pub keys: Vec<String>,
}

pub fn save_last(dir: &Path, l: &LastCheck) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join("last_check.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(l).unwrap_or_default())?;
    std::fs::rename(tmp, dir.join(FILE))
}

pub fn load_last(dir: &Path) -> Option<LastCheck> {
    serde_json::from_slice(&std::fs::read(dir.join(FILE)).ok()?).ok()
}

/// Notify only when something is outdated and the set changed since the last notified run,
/// so an unchanged backlog does not nag every day.
pub fn should_notify(prev: Option<&LastCheck>, now: &LastCheck) -> bool {
    now.count > 0 && prev.is_none_or(|p| p.keys != now.keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_validated() {
        for ok in ["00:00", "09:30", "23:59"] {
            assert!(valid_time(ok), "{ok}");
        }
        for bad in [
            "24:00", "9:30", "09:60", "ab:cd", "09-30", "", "09:300", "+9:30",
        ] {
            assert!(!valid_time(bad), "{bad}");
        }
    }

    #[test]
    fn create_args_are_a_fixed_vector() {
        let a = create_args("C:\\Program Files\\Sweepr\\sweepr.exe", "09:30").unwrap();
        assert_eq!(a[0], "/Create");
        assert_eq!(a[2], TASK_NAME);
        assert_eq!(
            a[4],
            "\"C:\\Program Files\\Sweepr\\sweepr.exe\" --scheduled-check"
        );
        assert_eq!(&a[5..], ["/SC", "DAILY", "/ST", "09:30", "/F"]);
    }

    #[test]
    fn create_args_reject_bad_input() {
        assert!(create_args("C:\\a\".exe", "09:00").is_err());
        assert!(create_args("", "09:00").is_err());
        assert!(create_args("C:\\a.exe", "9am").is_err());
    }

    #[test]
    fn delete_args_name_the_task() {
        assert_eq!(delete_args(), ["/Delete", "/TN", TASK_NAME, "/F"]);
    }

    #[test]
    fn notify_only_on_new_or_changed_backlog() {
        let a = LastCheck {
            ts: 1,
            count: 2,
            keys: vec!["winget:A@2".into(), "winget:B@3".into()],
        };
        let same = LastCheck { ts: 2, ..a.clone() };
        let more = LastCheck {
            ts: 2,
            count: 3,
            keys: vec!["winget:A@2".into(), "winget:B@3".into(), "scoop:C@1".into()],
        };
        let none = LastCheck {
            ts: 2,
            count: 0,
            keys: vec![],
        };
        assert!(should_notify(None, &a));
        assert!(!should_notify(Some(&a), &same));
        assert!(should_notify(Some(&a), &more));
        assert!(!should_notify(Some(&a), &none));
        assert!(!should_notify(None, &none));
    }

    #[test]
    fn last_check_roundtrips_and_bad_file_is_none() {
        let d = std::env::temp_dir().join(format!("sweepr-sched-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        assert!(load_last(&d).is_none());
        let l = LastCheck {
            ts: 5,
            count: 1,
            keys: vec!["winget:A@2".into()],
        };
        save_last(&d, &l).unwrap();
        assert_eq!(load_last(&d), Some(l));
        std::fs::write(d.join(FILE), "{").unwrap();
        assert!(load_last(&d).is_none());
    }
}
