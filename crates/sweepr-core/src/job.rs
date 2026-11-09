//! Job file and journal for the elevated batch. A job names packages only:
//! it can never carry command lines or paths.

use crate::errors::SwpError;
use crate::model::{Manager, Scope};
use crate::settings::atomic_write;
use crate::validate::valid_package_id;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

const MAX_ITEMS: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobItem {
    pub manager: Manager,
    pub id: String,
    pub scope: Scope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub id: String,
    pub items: Vec<JobItem>,
}

pub fn new_job_id() -> String {
    static N: AtomicU32 = AtomicU32::new(0);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!(
        "{t:x}-{:x}-{:x}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    )
}

/// Rejects anything that is not a bounded list of well-formed package ids.
pub fn validate_job(job: &Job) -> Result<(), SwpError> {
    let reject = |d: String| Err(SwpError::JobRejected(d));
    if !valid_package_id(&job.id) {
        return reject("bad job id".into());
    }
    if job.items.is_empty() || job.items.len() > MAX_ITEMS {
        return reject(format!("item count {} out of range", job.items.len()));
    }
    for it in &job.items {
        if !valid_package_id(&it.id) {
            return reject(format!("bad package id ({} chars)", it.id.len()));
        }
    }
    Ok(())
}

fn jobs_dir(dir: &Path) -> PathBuf {
    dir.join("jobs")
}

pub fn write_job(dir: &Path, job: &Job) -> std::io::Result<PathBuf> {
    validate_job(job)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))?;
    let jd = jobs_dir(dir);
    std::fs::create_dir_all(&jd)?;
    let path = jd.join(format!("{}.json", job.id));
    atomic_write(&path, serde_json::to_string_pretty(job)?.as_bytes())?;
    Journal::open(dir, &job.id).append("queued");
    Ok(path)
}

pub fn read_job(path: &Path) -> Result<Job, SwpError> {
    let text = std::fs::read_to_string(path).map_err(|e| SwpError::JobRejected(e.to_string()))?;
    let job: Job = serde_json::from_str(&text).map_err(|e| SwpError::JobRejected(e.to_string()))?;
    validate_job(&job)?;
    Ok(job)
}

/// Append-only progress log. A job whose journal has no `finished` line was interrupted.
pub struct Journal {
    job_path: PathBuf,
    path: PathBuf,
}

impl Journal {
    pub fn open(dir: &Path, job_id: &str) -> Self {
        let jd = jobs_dir(dir);
        Journal {
            job_path: jd.join(format!("{job_id}.json")),
            path: jd.join(format!("{job_id}.journal")),
        }
    }

    pub fn append(&self, line: &str) {
        if let Some(p) = self.path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            let _ = writeln!(f, "{line}");
            let _ = f.sync_all();
        }
    }

    /// Marks the job complete and removes its files.
    pub fn finish(&self) {
        self.append("finished");
        let _ = std::fs::remove_file(&self.job_path);
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Ids of jobs that started but never finished. Nothing re-runs them automatically.
pub fn unfinished(dir: &Path) -> Vec<String> {
    let mut v = Vec::new();
    let Ok(rd) = std::fs::read_dir(jobs_dir(dir)) else {
        return v;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("journal") {
            continue;
        }
        let done = std::fs::read_to_string(&p).is_ok_and(|t| t.lines().any(|l| l == "finished"));
        if !done {
            if let Some(s) = p.file_stem().and_then(|s| s.to_str()) {
                v.push(s.to_string());
            }
        }
    }
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Manager, Scope};

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("sweepr-job-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn item(id: &str) -> JobItem {
        JobItem {
            manager: Manager::Choco,
            id: id.into(),
            scope: Scope::Machine,
        }
    }

    #[test]
    fn job_roundtrip() {
        let d = tmp("rt");
        let job = Job {
            id: new_job_id(),
            items: vec![item("git"), item("7zip")],
        };
        let p = write_job(&d, &job).unwrap();
        assert_eq!(read_job(&p).unwrap(), job);
    }

    #[test]
    fn job_with_extra_field_rejected() {
        let d = tmp("extra");
        let p = d.join("j.json");
        std::fs::write(&p, r#"{"id":"j1","items":[{"manager":"choco","id":"git","scope":"machine","cmd":"calc"}]}"#).unwrap();
        let e = read_job(&p).unwrap_err();
        assert_eq!(e.code(), "SWP-203");
    }

    #[test]
    fn job_with_hostile_id_rejected() {
        let d = tmp("hostile");
        let p = d.join("j.json");
        for bad in ["foo & calc", "..\\x", &"a".repeat(500)] {
            let job = Job {
                id: "j1".into(),
                items: vec![item(bad)],
            };
            std::fs::write(&p, serde_json::to_string(&job).unwrap()).unwrap();
            assert_eq!(read_job(&p).unwrap_err().code(), "SWP-203", "{bad}");
        }
    }

    #[test]
    fn journal_marks_interrupted_job_after_restart() {
        let d = tmp("journal");
        let job = Job {
            id: "j-interrupted".into(),
            items: vec![item("git")],
        };
        write_job(&d, &job).unwrap();
        let j = Journal::open(&d, &job.id);
        j.append("started 0 git");
        assert_eq!(unfinished(&d), vec!["j-interrupted".to_string()]);
        j.finish();
        assert!(unfinished(&d).is_empty());
        assert!(!d.join("jobs").join("j-interrupted.json").exists());
    }
}
