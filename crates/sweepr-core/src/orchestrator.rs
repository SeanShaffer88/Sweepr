//! Discovery fan-out and upgrade queue planning. No UI, no shell.

use crate::adapter::PackageManager;
use crate::errors::SwpError;
use crate::exec::{self, CancelToken, Stream};
use crate::model::{Manager, Scope, UpdatePackage};
use crate::rules::apply_rules;
use crate::settings::Settings;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Running,
    Ok(usize),
    Failed(SwpError),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Running,
    Done,
    Failed(SwpError),
}

#[derive(Debug, Clone)]
pub enum Event {
    ManagerStatus(Manager, Status),
    Found(Manager, Vec<UpdatePackage>),
    Log(Manager, Stream, String),
    JobProgress { id: String, state: JobState },
    Done,
}

fn enabled(s: &Settings, m: Manager) -> bool {
    match m {
        Manager::Winget => s.sources.winget,
        Manager::Choco => s.sources.choco,
        Manager::Scoop => s.sources.scoop,
    }
}

#[cfg(windows)]
fn low_priority() {
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
    };
    // SAFETY: pseudo-handle of the current thread, valid for this call.
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

#[cfg(not(windows))]
fn low_priority() {}

fn discover_one(a: &dyn PackageManager, s: &Settings, cancel: &CancelToken, tx: &Sender<Event>) {
    let m = a.manager();
    let status = |st: Status| {
        let _ = tx.send(Event::ManagerStatus(m, st));
    };
    low_priority();
    let Some(exe) = a.detect() else {
        status(Status::Failed(SwpError::ManagerMissing(m)));
        return;
    };
    status(Status::Running);
    let mut on_line = |st: Stream, l: &str| {
        let _ = tx.send(Event::Log(m, st, l.to_string()));
    };
    let timeout = Duration::from_secs(s.timeout_secs);
    match exec::run(&exe, &a.discovery_args(), timeout, cancel, &mut on_line) {
        Err(_) => status(Status::Failed(SwpError::ManagerMissing(m))),
        Ok(o) if o.cancelled => status(Status::Cancelled),
        Ok(o) if o.timed_out => status(Status::Failed(SwpError::Timeout(m))),
        Ok(o) => match a.parse(&o.stdout) {
            Ok(p) => {
                let p = apply_rules(p, s);
                let n = p.len();
                let _ = tx.send(Event::Found(m, p));
                status(Status::Ok(n));
            }
            Err(e) => status(Status::Failed(e)),
        },
    }
}

/// Runs discovery for every enabled manager in parallel, one low-priority
/// thread each. Sends `Event::Done` last.
pub fn discover(
    adapters: Vec<Box<dyn PackageManager>>,
    settings: &Settings,
    cancel: CancelToken,
    tx: Sender<Event>,
) -> JoinHandle<()> {
    let settings = settings.clone();
    thread::spawn(move || {
        let mut workers = vec![];
        for a in adapters {
            if !enabled(&settings, a.manager()) {
                continue;
            }
            let a: Arc<dyn PackageManager> = Arc::from(a);
            let (s, c, t) = (settings.clone(), cancel.clone(), tx.clone());
            workers.push(thread::spawn(move || discover_one(&*a, &s, &c, &t)));
        }
        for w in workers {
            let _ = w.join();
        }
        let _ = tx.send(Event::Done);
    })
}

#[derive(Debug, Clone, Default)]
pub struct QueuePlan {
    pub user: Vec<UpdatePackage>,
    pub system: Vec<UpdatePackage>,
}

/// Machine-scope and Chocolatey items need elevation. When already elevated
/// everything runs in-process. WinGet items of unknown scope go to the user
/// queue; WinGet elevates per installer itself.
pub fn plan_queues(sel: &[UpdatePackage], is_elevated: bool) -> QueuePlan {
    let mut plan = QueuePlan::default();
    for p in sel {
        let needs = !is_elevated && (p.manager == Manager::Choco || p.scope == Scope::Machine);
        if needs {
            plan.system.push(p.clone());
        } else {
            plan.user.push(p.clone());
        }
    }
    plan
}

#[cfg(test)]
#[cfg_attr(not(unix), allow(dead_code, unused_imports))]
mod tests {
    use super::*;
    use crate::adapter::PackageManager;
    use crate::errors::SwpError;
    use crate::exec::CancelToken;
    use crate::model::{Manager, Scope, UpdatePackage};
    use crate::settings::Settings;
    use std::path::PathBuf;
    use std::sync::mpsc::channel;
    use std::time::{Duration, Instant};

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

    /// Runs `/bin/sh -c <script>` as the "manager" and returns canned packages.
    struct Fake {
        m: Manager,
        script: &'static str,
        present: bool,
    }

    impl PackageManager for Fake {
        fn manager(&self) -> Manager {
            self.m
        }
        fn detect(&self) -> Option<PathBuf> {
            self.present.then(|| PathBuf::from("/bin/sh"))
        }
        fn discovery_args(&self) -> Vec<String> {
            vec!["-c".into(), self.script.into()]
        }
        fn parse(&self, _out: &str) -> Result<Vec<UpdatePackage>, SwpError> {
            Ok(vec![pkg(
                self.m,
                &format!("{}-pkg", self.m.as_str()),
                Scope::User,
            )])
        }
        fn upgrade_args(&self, _p: &UpdatePackage) -> Result<Vec<String>, SwpError> {
            Ok(vec![])
        }
    }

    fn fake(m: Manager, script: &'static str, present: bool) -> Box<dyn PackageManager> {
        Box::new(Fake { m, script, present })
    }

    fn collect(rx: std::sync::mpsc::Receiver<Event>) -> Vec<Event> {
        let mut v = vec![];
        while let Ok(e) = rx.recv_timeout(Duration::from_secs(10)) {
            let done = matches!(e, Event::Done);
            v.push(e);
            if done {
                break;
            }
        }
        v
    }

    fn found(events: &[Event]) -> Vec<Manager> {
        let mut v: Vec<Manager> = events
            .iter()
            .filter_map(|e| match e {
                Event::Found(m, p) if !p.is_empty() => Some(*m),
                _ => None,
            })
            .collect();
        v.sort();
        v
    }

    fn status(events: &[Event], m: Manager) -> Option<Status> {
        events.iter().rev().find_map(|e| match e {
            Event::ManagerStatus(x, s) if *x == m && !matches!(s, Status::Running) => {
                Some(s.clone())
            }
            _ => None,
        })
    }

    #[cfg(unix)]
    #[test]
    fn managers_run_in_parallel() {
        let (tx, rx) = channel();
        let t = Instant::now();
        let h = discover(
            vec![
                fake(Manager::Winget, "sleep 0.3", true),
                fake(Manager::Choco, "sleep 0.3", true),
                fake(Manager::Scoop, "sleep 0.3", true),
            ],
            &Settings::default(),
            CancelToken::new(),
            tx,
        );
        let ev = collect(rx);
        h.join().unwrap();
        assert!(
            t.elapsed() < Duration::from_millis(600),
            "{:?}",
            t.elapsed()
        );
        assert_eq!(
            found(&ev),
            vec![Manager::Winget, Manager::Choco, Manager::Scoop]
        );
    }

    #[cfg(unix)]
    #[test]
    fn timeout_gives_partial_results() {
        let s = Settings {
            timeout_secs: 1,
            ..Settings::default()
        };
        let (tx, rx) = channel();
        let h = discover(
            vec![
                fake(Manager::Winget, "sleep 30", true),
                fake(Manager::Choco, "true", true),
                fake(Manager::Scoop, "true", true),
            ],
            &s,
            CancelToken::new(),
            tx,
        );
        let ev = collect(rx);
        h.join().unwrap();
        assert_eq!(found(&ev), vec![Manager::Choco, Manager::Scoop]);
        match status(&ev, Manager::Winget) {
            Some(Status::Failed(e)) => assert_eq!(e.code(), "SWP-102"),
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn missing_manager_reports_101_and_skips() {
        let (tx, rx) = channel();
        let h = discover(
            vec![fake(Manager::Scoop, "true", false)],
            &Settings::default(),
            CancelToken::new(),
            tx,
        );
        let ev = collect(rx);
        h.join().unwrap();
        match status(&ev, Manager::Scoop) {
            Some(Status::Failed(e)) => assert_eq!(e.code(), "SWP-101"),
            o => panic!("{o:?}"),
        }
        assert!(found(&ev).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn disabled_source_not_run() {
        let mut s = Settings::default();
        s.sources.choco = false;
        let (tx, rx) = channel();
        let h = discover(
            vec![
                fake(Manager::Choco, "true", true),
                fake(Manager::Scoop, "true", true),
            ],
            &s,
            CancelToken::new(),
            tx,
        );
        let ev = collect(rx);
        h.join().unwrap();
        assert_eq!(found(&ev), vec![Manager::Scoop]);
        assert_eq!(status(&ev, Manager::Choco), None);
    }

    #[cfg(unix)]
    #[test]
    fn ignored_rule_applied_to_found() {
        let mut s = Settings::default();
        s.ignore.push("scoop-pkg".into());
        let (tx, rx) = channel();
        let h = discover(
            vec![fake(Manager::Scoop, "true", true)],
            &s,
            CancelToken::new(),
            tx,
        );
        let ev = collect(rx);
        h.join().unwrap();
        let p = ev
            .iter()
            .find_map(|e| match e {
                Event::Found(_, p) => p.first().cloned(),
                _ => None,
            })
            .unwrap();
        assert!(p.ignored);
    }

    #[test]
    fn plan_splits_scopes() {
        let sel = vec![
            pkg(Manager::Choco, "git", Scope::Machine),
            pkg(Manager::Winget, "A.Machine", Scope::Machine),
            pkg(Manager::Scoop, "g", Scope::Machine),
            pkg(Manager::Winget, "B.User", Scope::User),
            pkg(Manager::Winget, "C.Unknown", Scope::Unknown),
            pkg(Manager::Scoop, "rg", Scope::User),
        ];
        let plan = plan_queues(&sel, false);
        let ids = |v: &[UpdatePackage]| v.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&plan.system), vec!["git", "A.Machine", "g"]);
        assert_eq!(ids(&plan.user), vec!["B.User", "C.Unknown", "rg"]);
    }

    #[test]
    fn plan_all_user_when_elevated() {
        let sel = vec![
            pkg(Manager::Choco, "git", Scope::Machine),
            pkg(Manager::Scoop, "rg", Scope::User),
        ];
        let plan = plan_queues(&sel, true);
        assert!(plan.system.is_empty());
        assert_eq!(plan.user.len(), 2);
    }
}
