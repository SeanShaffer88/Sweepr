//! Local update history: one JSON line per attempted update, kept in the data folder.
//! Never leaves the machine. Undo hints are text to copy; Sweepr never runs them.

use crate::model::Manager;
use crate::validate::valid_package_id;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

const FILE: &str = "history.jsonl";
/// Above this many lines the file is cut back to `KEEP`.
const MAX_LINES: usize = 2000;
const KEEP: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Seconds since the Unix epoch, UTC.
    pub ts: u64,
    pub manager: Manager,
    pub id: String,
    pub from: String,
    pub to: String,
    pub ok: bool,
    pub code: Option<i32>,
}

pub fn file(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Appends entries; trims old ones past the cap. Errors are returned, never fatal.
pub fn append(dir: &Path, entries: &[Entry]) -> std::io::Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file(dir))?;
    for e in entries {
        if let Ok(line) = serde_json::to_string(e) {
            writeln!(f, "{line}")?;
        }
    }
    drop(f);
    trim(dir)
}

fn trim(dir: &Path) -> std::io::Result<()> {
    let text = std::fs::read_to_string(file(dir))?;
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= MAX_LINES {
        return Ok(());
    }
    let keep = lines[lines.len() - KEEP..].join("\n") + "\n";
    let tmp = dir.join("history.jsonl.tmp");
    std::fs::write(&tmp, keep)?;
    std::fs::rename(tmp, file(dir))
}

/// Entries newest first. Lines that do not parse are skipped, so a damaged file still loads.
pub fn load(dir: &Path) -> Vec<Entry> {
    let Ok(text) = std::fs::read_to_string(file(dir)) else {
        return Vec::new();
    };
    let mut v: Vec<Entry> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    v.reverse();
    v
}

/// `YYYY-MM-DD HH:MM` in UTC.
pub fn utc(ts: u64) -> String {
    let (days, rem) = ((ts / 86_400) as i64, ts % 86_400);
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60
    )
}

fn plain_version(v: &str) -> bool {
    !v.is_empty()
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_' | '~'))
}

/// A command the user may run to go back, or `None` when the old version is unknown or odd
/// (WinGet shows "Unknown" or "< 1.2"). Shown as text only; downgrades are not guaranteed.
pub fn undo_hint(e: &Entry) -> Option<String> {
    if !e.ok || !valid_package_id(&e.id) || !plain_version(&e.from) || e.from == "Unknown" {
        return None;
    }
    Some(match e.manager {
        Manager::Winget => format!("winget install --id {} --version {} --force", e.id, e.from),
        Manager::Choco => format!(
            "choco install {} --version {} --allow-downgrade -y",
            e.id, e.from
        ),
        Manager::Scoop => format!("scoop reset {}@{}", e.id, e.from),
    })
}

/// One printable line: time, manager, id, versions, outcome.
pub fn line(e: &Entry) -> String {
    format!(
        "{}  {:<6} {}  {} -> {}  {}",
        utc(e.ts),
        e.manager.as_str(),
        e.id,
        e.from,
        e.to,
        if e.ok {
            "updated".to_string()
        } else {
            match e.code {
                Some(c) => format!("FAILED (exit code {c})"),
                None => "FAILED".to_string(),
            }
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sweepr-hist-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn e(i: u64, ok: bool) -> Entry {
        Entry {
            ts: i,
            manager: Manager::Winget,
            id: format!("Foo.Bar{i}"),
            from: "1.0".into(),
            to: "2.0".into(),
            ok,
            code: if ok { None } else { Some(1603) },
        }
    }

    #[test]
    fn append_then_load_newest_first() {
        let d = tmp("rt");
        append(&d, &[e(1, true), e(2, false)]).unwrap();
        append(&d, &[e(3, true)]).unwrap();
        let v = load(&d);
        assert_eq!(v.iter().map(|x| x.ts).collect::<Vec<_>>(), vec![3, 2, 1]);
        assert_eq!(v[1].code, Some(1603));
    }

    #[test]
    fn damaged_lines_are_skipped() {
        let d = tmp("bad");
        append(&d, &[e(1, true)]).unwrap();
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(file(&d))
            .unwrap();
        writeln!(f, "not json").unwrap();
        drop(f);
        append(&d, &[e(2, true)]).unwrap();
        assert_eq!(load(&d).len(), 2);
    }

    #[test]
    fn missing_file_is_empty() {
        assert!(load(&tmp("none")).is_empty());
    }

    #[test]
    fn file_is_trimmed_past_cap() {
        let d = tmp("cap");
        let many: Vec<Entry> = (0..(MAX_LINES as u64 + 5)).map(|i| e(i, true)).collect();
        append(&d, &many).unwrap();
        let v = load(&d);
        assert_eq!(v.len(), KEEP);
        assert_eq!(v[0].ts, MAX_LINES as u64 + 4);
    }

    #[test]
    fn utc_formats_known_instants() {
        assert_eq!(utc(0), "1970-01-01 00:00");
        assert_eq!(utc(1_700_000_000), "2023-11-14 22:13");
        assert_eq!(utc(1_782_864_000), "2026-07-01 00:00");
        assert_eq!(utc(951_782_400), "2000-02-29 00:00");
    }

    #[test]
    fn undo_hints_per_manager() {
        let mut x = e(1, true);
        assert_eq!(
            undo_hint(&x).unwrap(),
            "winget install --id Foo.Bar1 --version 1.0 --force"
        );
        x.manager = Manager::Choco;
        assert!(undo_hint(&x).unwrap().starts_with("choco install Foo.Bar1"));
        x.manager = Manager::Scoop;
        assert_eq!(undo_hint(&x).unwrap(), "scoop reset Foo.Bar1@1.0");
    }

    #[test]
    fn no_hint_for_failures_unknown_or_hostile_values() {
        assert!(undo_hint(&e(1, false)).is_none());
        let mut x = e(1, true);
        x.from = "Unknown".into();
        assert!(undo_hint(&x).is_none());
        x.from = "< 3.15".into();
        assert!(undo_hint(&x).is_none());
        x.from = "1.0 & calc".into();
        assert!(undo_hint(&x).is_none());
        x.from = "1.0".into();
        x.id = "a b;c".into();
        assert!(undo_hint(&x).is_none());
    }

    #[test]
    fn line_shows_failure_code() {
        assert!(line(&e(1, false)).contains("FAILED (exit code 1603)"));
    }
}
