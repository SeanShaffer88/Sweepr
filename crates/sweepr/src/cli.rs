//! Command line surface. The same exe is the GUI (no flags) and the headless CLI.

use std::io::Write;
use std::path::PathBuf;
use sweepr_core::adapter::{all_adapters, PackageManager};
use sweepr_core::errors::SwpError;
use sweepr_core::exec::CancelToken;
use sweepr_core::model::UpdatePackage;
use sweepr_core::orchestrator::{discover, Event, Status};
use sweepr_core::settings::{data_dir, remove_data, Settings};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Cli {
    pub headless: bool,
    pub json: bool,
    pub dry_run: bool,
    pub update_all: bool,
    pub silent: bool,
    pub verbose: bool,
    pub debug: bool,
    pub trace: bool,
    pub help: bool,
    pub version: bool,
    pub clean: bool,
    pub uninstall: bool,
    pub elevated_run: Option<PathBuf>,
    pub pipe: Option<String>,
}

pub const USAGE: &str = "Sweepr - one list of outdated software from WinGet, Chocolatey and Scoop.

USAGE: sweepr [OPTIONS]        (no options starts the window)

  --headless          list outdated packages without a window
  --json              print the list as JSON (implies --headless)
  --dry-run           also print the exact upgrade commands, run nothing (implies --headless)
  --update-all        update every outdated package (implies --headless);
                      system-scope packages share one UAC prompt on Windows
  --silent            no text list on stdout
  --verbose, --debug, --trace   more detail on stderr
  --clean             delete Sweepr's stored data (settings, logs, jobs); the program stays
  --uninstall         delete all of Sweepr's stored data and its data folder;
                      remove the program itself from Settings > Apps (installed)
                      or by deleting its folder (portable)
  --elevated-run <job> --pipe <name>   internal: elevated helper
  --version, --help

Exit codes: 0 ok, 1 a manager failed (partial results), 2 usage error.
";

pub fn parse_args(args: &[String]) -> Result<Cli, String> {
    let mut c = Cli::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--headless" => c.headless = true,
            "--json" => c.json = true,
            "--dry-run" => c.dry_run = true,
            "--update-all" => c.update_all = true,
            "--silent" => c.silent = true,
            "--verbose" => c.verbose = true,
            "--debug" => c.debug = true,
            "--trace" => c.trace = true,
            "--help" | "-h" => c.help = true,
            "--version" => c.version = true,
            "--clean" => c.clean = true,
            "--uninstall" => c.uninstall = true,
            "--elevated-run" => {
                let v = it.next().ok_or("--elevated-run needs a job path")?;
                c.elevated_run = Some(PathBuf::from(v));
            }
            "--pipe" => c.pipe = Some(it.next().ok_or("--pipe needs a name")?.clone()),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(c)
}

/// Handles CLI modes. `None` means no CLI mode was asked for: start the GUI.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> Option<i32> {
    if args.is_empty() {
        return None;
    }
    let c = match parse_args(args) {
        Ok(c) => c,
        Err(e) => {
            let _ = writeln!(err, "sweepr: {e}\n\n{USAGE}");
            return Some(2);
        }
    };
    if c.help {
        let _ = write!(out, "{USAGE}");
        return Some(0);
    }
    if c.version {
        let _ = writeln!(out, "sweepr {}", env!("CARGO_PKG_VERSION"));
        return Some(0);
    }
    if c.clean || c.uninstall {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_default();
        return Some(clean(&data_dir(&exe_dir), c.uninstall, c.silent, out, err));
    }
    if let Some(job) = &c.elevated_run {
        let Some(pipe) = &c.pipe else {
            let _ = writeln!(err, "sweepr: --elevated-run needs --pipe");
            return Some(2);
        };
        return Some(elevated(job, pipe, err));
    }
    if c.headless || c.json || c.dry_run || c.update_all {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_default();
        let dir = data_dir(&exe_dir);
        let settings = Settings::load(&dir).settings;
        if c.update_all && !c.dry_run {
            return Some(update_all(
                &c,
                &|| all_adapters(),
                &settings,
                &dir,
                out,
                err,
            ));
        }
        return Some(run_headless_with(&c, &|| all_adapters(), &settings, out));
    }
    None
}

/// `--clean` / `--uninstall`: delete what Sweepr stored in its data dir.
pub fn clean(
    dir: &std::path::Path,
    uninstall: bool,
    silent: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    match remove_data(dir, uninstall) {
        Ok(removed) => {
            if !silent {
                if removed.is_empty() {
                    let _ = writeln!(out, "sweepr: nothing stored at {}", dir.display());
                } else {
                    for p in &removed {
                        let _ = writeln!(out, "removed {}", p.display());
                    }
                    if uninstall {
                        let _ = writeln!(
                            out,
                            "sweepr: data removed. Remove the program from Settings > Apps \
                             (installed) or by deleting its folder (portable)."
                        );
                    } else {
                        let _ =
                            writeln!(out, "sweepr: data removed; settings are back to defaults.");
                    }
                }
            }
            0
        }
        Err(e) => {
            let _ = writeln!(
                err,
                "sweepr: could not remove data at {}: {e}",
                dir.display()
            );
            1
        }
    }
}

#[cfg(windows)]
fn elevated(job: &std::path::Path, pipe: &str, _err: &mut dyn Write) -> i32 {
    sweepr_core::elevation::win::run_helper(job, pipe)
}

#[cfg(not(windows))]
fn elevated(_job: &std::path::Path, _pipe: &str, err: &mut dyn Write) -> i32 {
    let _ = writeln!(err, "sweepr: --elevated-run is Windows only");
    2
}

fn detail_on(c: &Cli) -> bool {
    c.verbose || c.debug || c.trace
}

/// Discovery without a window. Missing managers are not failures; timeouts and parse errors are.
pub fn run_headless_with(
    c: &Cli,
    factory: &dyn Fn() -> Vec<Box<dyn PackageManager>>,
    settings: &Settings,
    out: &mut dyn Write,
) -> i32 {
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = discover(factory(), settings, CancelToken::new(), tx);
    let mut pkgs: Vec<UpdatePackage> = Vec::new();
    let mut failed = false;
    for ev in rx {
        match ev {
            Event::Found(_, p) => pkgs.extend(p),
            Event::ManagerStatus(m, Status::Failed(e)) => {
                if detail_on(c) || !matches!(e, SwpError::ManagerMissing(_)) {
                    eprintln!("sweepr: {}: {e}", m.as_str());
                }
                if !matches!(e, SwpError::ManagerMissing(_)) {
                    failed = true;
                }
            }
            Event::Log(m, _, l) if c.debug || c.trace => eprintln!("[{}] {l}", m.as_str()),
            Event::Done => break,
            _ => {}
        }
    }
    let _ = handle.join();
    pkgs.retain(|p| !p.ignored);
    pkgs.sort_by(|a, b| (a.manager, &a.id).cmp(&(b.manager, &b.id)));

    let mut commands = Vec::new();
    if c.dry_run {
        let adapters = factory();
        for p in &pkgs {
            if let Some(a) = adapters.iter().find(|a| a.manager() == p.manager) {
                match (a.detect(), a.dry_run_args(p)) {
                    (Some(exe), Ok(args)) => {
                        commands.push(format!("{} {}", exe.display(), args.join(" ")))
                    }
                    (_, Err(e)) => eprintln!("sweepr: {}: {e}", p.id),
                    (None, _) => commands.push(format!("{} {}", a.manager().as_str(), p.id)),
                }
            }
        }
    }
    if c.json {
        let _ = serde_json::to_writer_pretty(&mut *out, &pkgs);
        let _ = writeln!(out);
        for l in &commands {
            eprintln!("{l}");
        }
    } else {
        if !c.silent {
            for p in &pkgs {
                let _ = writeln!(
                    out,
                    "{}\t{}\t{}\t{}",
                    p.manager.as_str(),
                    p.id,
                    p.current_version,
                    p.available_version
                );
            }
        }
        for l in &commands {
            let _ = writeln!(out, "{l}");
        }
    }
    i32::from(failed)
}

/// One updated package's outcome, for text or JSON reporting.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct UpdateResult {
    pub manager: sweepr_core::model::Manager,
    pub id: String,
    pub ok: bool,
    pub code: Option<i32>,
}

/// Runs one job in-process and collects per-item results from the wire stream.
fn run_job_in_process(
    job: &sweepr_core::job::Job,
    factory: &dyn Fn() -> Vec<Box<dyn PackageManager>>,
    results: &mut Vec<UpdateResult>,
) -> i32 {
    let adapters = factory();
    let mut sink_results: Vec<(String, bool, Option<i32>)> = Vec::new();
    let code = sweepr_core::elevation::execute_job(
        job,
        &adapters,
        &mut sweepr_core::elevation::RealRunner,
        &CancelToken::new(),
        &mut |w| {
            if let sweepr_core::elevation::Wire::Item { id, state, code } = w {
                if state != sweepr_core::elevation::ItemState::Started {
                    sink_results.push((id, state == sweepr_core::elevation::ItemState::Done, code));
                }
            }
        },
    );
    for it in &job.items {
        let (ok, c) = sink_results
            .iter()
            .find(|(id, _, _)| *id == it.id)
            .map(|(_, ok, c)| (*ok, *c))
            .unwrap_or((false, None));
        results.push(UpdateResult {
            manager: it.manager,
            id: it.id.clone(),
            ok,
            code: c,
        });
    }
    code
}

/// `--update-all`: discover, then update everything not ignored or pinned.
/// User-scope packages run in-process; the system queue shares one UAC
/// prompt on Windows (the same flow as the window). `--dry-run` never
/// reaches this function: `run` routes it to the listing instead.
pub fn update_all(
    c: &Cli,
    factory: &dyn Fn() -> Vec<Box<dyn PackageManager>>,
    settings: &Settings,
    dir: &std::path::Path,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = discover(factory(), settings, CancelToken::new(), tx);
    let mut pkgs: Vec<UpdatePackage> = Vec::new();
    let mut failed = false;
    for ev in rx {
        match ev {
            Event::Found(_, p) => pkgs.extend(p),
            Event::ManagerStatus(m, Status::Failed(e)) => {
                if !matches!(e, SwpError::ManagerMissing(_)) {
                    let _ = writeln!(err, "sweepr: {}: {e}", m.as_str());
                    failed = true;
                }
            }
            Event::Done => break,
            _ => {}
        }
    }
    let _ = handle.join();
    pkgs.retain(|p| !p.ignored);
    if pkgs.is_empty() {
        if !c.silent && !c.json {
            let _ = writeln!(out, "everything is up to date");
        }
        if c.json {
            let _ = writeln!(out, "[]");
        }
        return i32::from(failed);
    }

    let elevated = is_elevated();
    let (user, sys) = crate::ui::actions::build_jobs(&pkgs, elevated);
    let mut results: Vec<UpdateResult> = Vec::new();
    if let Some(job) = &user {
        if run_job_in_process(job, factory, &mut results) != 0 {
            failed = true;
        }
    }
    if let Some(job) = &sys {
        if elevated {
            if run_job_in_process(job, factory, &mut results) != 0 {
                failed = true;
            }
        } else if !run_system_job(job, dir, &mut results, err) {
            failed = true;
        }
    }
    if results.iter().any(|r| !r.ok) {
        failed = true;
    }

    if c.json {
        let _ = serde_json::to_writer_pretty(&mut *out, &results);
        let _ = writeln!(out);
    } else if !c.silent {
        for r in &results {
            let _ = writeln!(
                out,
                "{}\t{}\t{}",
                r.manager.as_str(),
                r.id,
                if r.ok { "updated" } else { "FAILED" }
            );
        }
    }
    i32::from(failed)
}

#[cfg(windows)]
fn is_elevated() -> bool {
    sweepr_core::elevation::win::is_elevated()
}

#[cfg(not(windows))]
fn is_elevated() -> bool {
    false
}

#[cfg(windows)]
fn run_system_job(
    job: &sweepr_core::job::Job,
    dir: &std::path::Path,
    results: &mut Vec<UpdateResult>,
    err: &mut dyn Write,
) -> bool {
    let mut sink_results: Vec<(String, bool, Option<i32>)> = Vec::new();
    let r = sweepr_core::elevation::win::run_elevated(dir, job, &CancelToken::new(), &mut |w| {
        if let sweepr_core::elevation::Wire::Item { id, state, code } = w {
            if *state != sweepr_core::elevation::ItemState::Started {
                sink_results.push((
                    id.clone(),
                    *state == sweepr_core::elevation::ItemState::Done,
                    *code,
                ));
            }
        }
    });
    match r {
        Ok(_) => {
            for it in &job.items {
                let (ok, c) = sink_results
                    .iter()
                    .find(|(id, _, _)| *id == it.id)
                    .map(|(_, ok, c)| (*ok, *c))
                    .unwrap_or((false, None));
                results.push(UpdateResult {
                    manager: it.manager,
                    id: it.id.clone(),
                    ok,
                    code: c,
                });
            }
            true
        }
        Err(e) => {
            let _ = writeln!(err, "sweepr: {e}");
            for it in &job.items {
                results.push(UpdateResult {
                    manager: it.manager,
                    id: it.id.clone(),
                    ok: false,
                    code: None,
                });
            }
            false
        }
    }
}

#[cfg(not(windows))]
fn run_system_job(
    job: &sweepr_core::job::Job,
    _dir: &std::path::Path,
    results: &mut Vec<UpdateResult>,
    err: &mut dyn Write,
) -> bool {
    let _ = writeln!(
        err,
        "sweepr: system-scope updates need Windows elevation; {} package(s) skipped",
        job.items.len()
    );
    for it in &job.items {
        results.push(UpdateResult {
            manager: it.manager,
            id: it.id.clone(),
            ok: false,
            code: None,
        });
    }
    false
}

#[cfg(test)]
#[cfg_attr(not(unix), allow(dead_code, unused_imports))]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use sweepr_core::adapter::PackageManager;
    use sweepr_core::errors::SwpError;
    use sweepr_core::model::{Manager, UpdatePackage};
    use sweepr_core::settings::Settings;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_flags() {
        let c = parse_args(&args(&["--headless", "--json", "--dry-run", "--verbose"])).unwrap();
        assert!(c.headless && c.json && c.dry_run && c.verbose);
        assert!(!c.silent && !c.debug && !c.trace);
        let c = parse_args(&args(&[
            "--elevated-run",
            "C:\\j.json",
            "--pipe",
            "\\\\.\\pipe\\x",
        ]))
        .unwrap();
        assert_eq!(c.elevated_run, Some(PathBuf::from("C:\\j.json")));
        assert_eq!(c.pipe.as_deref(), Some("\\\\.\\pipe\\x"));
    }

    #[test]
    fn unknown_flag_exit_2() {
        assert!(parse_args(&args(&["--bogus"])).is_err());
        let (mut o, mut e) = (Vec::new(), Vec::new());
        assert_eq!(run(&args(&["--bogus"]), &mut o, &mut e), Some(2));
        assert!(String::from_utf8(e).unwrap().contains("--bogus"));
    }

    #[test]
    fn clean_and_uninstall_flags_parse() {
        let c = parse_args(&args(&["--clean"])).unwrap();
        assert!(c.clean && !c.uninstall);
        let c = parse_args(&args(&["--uninstall"])).unwrap();
        assert!(c.uninstall && !c.clean);
    }

    #[test]
    fn clean_removes_data_and_reports() {
        let d = std::env::temp_dir().join(format!("sweepr-cli-clean-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("settings.json"), "{}").unwrap();
        let (mut o, mut e) = (Vec::new(), Vec::new());
        assert_eq!(clean(&d, false, false, &mut o, &mut e), 0);
        assert!(d.exists());
        assert!(std::fs::read_dir(&d).unwrap().next().is_none());
        let s = String::from_utf8(o).unwrap();
        assert!(s.contains("settings.json"), "{s}");
        let (mut o, mut e) = (Vec::new(), Vec::new());
        assert_eq!(clean(&d, true, false, &mut o, &mut e), 0);
        assert!(!d.exists());
        // Missing dir is fine, not an error.
        let (mut o, mut e) = (Vec::new(), Vec::new());
        assert_eq!(clean(&d, true, false, &mut o, &mut e), 0);
        assert!(String::from_utf8(o).unwrap().contains("nothing stored"));
    }

    #[test]
    fn help_and_version_exit_0() {
        let (mut o, mut e) = (Vec::new(), Vec::new());
        assert_eq!(run(&args(&["--help"]), &mut o, &mut e), Some(0));
        assert!(String::from_utf8(o).unwrap().contains("--headless"));
        let (mut o, mut e) = (Vec::new(), Vec::new());
        assert_eq!(run(&args(&["--version"]), &mut o, &mut e), Some(0));
    }

    #[test]
    fn no_flags_means_gui() {
        let (mut o, mut e) = (Vec::new(), Vec::new());
        assert_eq!(run(&[], &mut o, &mut e), None);
    }

    struct Fake;
    impl PackageManager for Fake {
        fn manager(&self) -> Manager {
            Manager::Choco
        }
        fn detect(&self) -> Option<PathBuf> {
            Some(PathBuf::from("/bin/sh"))
        }
        fn discovery_args(&self) -> Vec<String> {
            args(&["-c", "printf 'git|1.0|2.0|false\\n'"])
        }
        fn parse(&self, out: &str) -> Result<Vec<UpdatePackage>, SwpError> {
            sweepr_core::parse::parse_choco(out)
        }
        fn upgrade_args(&self, _: &UpdatePackage) -> Result<Vec<String>, SwpError> {
            Ok(args(&["-c", "touch /tmp/sweepr-cli-test-marker"]))
        }
        fn dry_run_args(&self, p: &UpdatePackage) -> Result<Vec<String>, SwpError> {
            Ok(args(&["upgrade", &p.id, "-y", "--noop"]))
        }
    }

    #[cfg(unix)]
    #[test]
    fn headless_json_output_shape() {
        let c = parse_args(&args(&["--headless", "--json"])).unwrap();
        let mut out = Vec::new();
        let code = run_headless_with(&c, &|| vec![Box::new(Fake)], &Settings::default(), &mut out);
        assert_eq!(code, 0);
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["id"], "git");
        assert_eq!(v[0]["manager"], "choco");
        assert_eq!(v[0]["available_version"], "2.0");
    }

    #[cfg(unix)]
    struct UserFake;
    #[cfg(unix)]
    impl PackageManager for UserFake {
        fn manager(&self) -> Manager {
            Manager::Scoop
        }
        fn detect(&self) -> Option<PathBuf> {
            Some(PathBuf::from("/bin/sh"))
        }
        fn discovery_args(&self) -> Vec<String> {
            args(&["-c", "printf 'x\\n'"])
        }
        fn parse(&self, _: &str) -> Result<Vec<UpdatePackage>, SwpError> {
            Ok(vec![UpdatePackage {
                id: "fd".into(),
                name: "fd".into(),
                current_version: "1".into(),
                available_version: "2".into(),
                manager: Manager::Scoop,
                scope: sweepr_core::model::Scope::User,
                elevation_required: false,
                pinned_by_manager: false,
                ignored: false,
                user_pin: None,
            }])
        }
        fn upgrade_args(&self, _: &UpdatePackage) -> Result<Vec<String>, SwpError> {
            Ok(args(&["-c", "touch /tmp/sweepr-update-all-marker"]))
        }
    }

    #[cfg(unix)]
    #[test]
    fn update_all_runs_user_queue_and_reports() {
        let _ = std::fs::remove_file("/tmp/sweepr-update-all-marker");
        let c = parse_args(&args(&["--update-all"])).unwrap();
        assert!(c.update_all);
        let (mut o, mut e) = (Vec::new(), Vec::new());
        let d = std::env::temp_dir().join(format!("sweepr-cli-upd-{}", std::process::id()));
        let code = update_all(
            &c,
            &|| vec![Box::new(UserFake) as Box<dyn PackageManager>],
            &Settings::default(),
            &d,
            &mut o,
            &mut e,
        );
        assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&e));
        assert!(std::path::Path::new("/tmp/sweepr-update-all-marker").exists());
        let s = String::from_utf8(o).unwrap();
        assert!(s.contains("fd") && s.contains("updated"), "{s}");
    }

    #[cfg(unix)]
    #[test]
    fn dry_run_executes_nothing() {
        let _ = std::fs::remove_file("/tmp/sweepr-cli-test-marker");
        let c = parse_args(&args(&["--headless", "--dry-run"])).unwrap();
        let mut out = Vec::new();
        let code = run_headless_with(&c, &|| vec![Box::new(Fake)], &Settings::default(), &mut out);
        assert_eq!(code, 0);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("upgrade git -y --noop"), "{s}");
        assert!(!std::path::Path::new("/tmp/sweepr-cli-test-marker").exists());
    }

    #[cfg(unix)]
    struct Garbled;
    #[cfg(unix)]
    impl PackageManager for Garbled {
        fn manager(&self) -> Manager {
            Manager::Scoop
        }
        fn detect(&self) -> Option<PathBuf> {
            Some(PathBuf::from("/bin/sh"))
        }
        fn discovery_args(&self) -> Vec<String> {
            args(&["-c", "echo garbage"])
        }
        fn parse(&self, _: &str) -> Result<Vec<UpdatePackage>, SwpError> {
            Err(SwpError::ParseFailed {
                manager: Manager::Scoop,
                detail: "x".into(),
            })
        }
        fn upgrade_args(&self, _: &UpdatePackage) -> Result<Vec<String>, SwpError> {
            Ok(vec![])
        }
    }

    #[cfg(unix)]
    struct Missing;
    #[cfg(unix)]
    impl PackageManager for Missing {
        fn manager(&self) -> Manager {
            Manager::Scoop
        }
        fn detect(&self) -> Option<PathBuf> {
            None
        }
        fn discovery_args(&self) -> Vec<String> {
            vec![]
        }
        fn parse(&self, _: &str) -> Result<Vec<UpdatePackage>, SwpError> {
            Ok(vec![])
        }
        fn upgrade_args(&self, _: &UpdatePackage) -> Result<Vec<String>, SwpError> {
            Ok(vec![])
        }
    }

    #[cfg(unix)]
    #[test]
    fn failed_manager_gives_exit_1_but_keeps_partial_results() {
        let c = parse_args(&args(&["--headless", "--json"])).unwrap();
        let mut out = Vec::new();
        let f = || -> Vec<Box<dyn PackageManager>> { vec![Box::new(Fake), Box::new(Garbled)] };
        let code = run_headless_with(&c, &f, &Settings::default(), &mut out);
        assert_eq!(code, 1);
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn missing_manager_is_not_a_failure() {
        let c = parse_args(&args(&["--headless", "--json"])).unwrap();
        let mut out = Vec::new();
        let f = || -> Vec<Box<dyn PackageManager>> { vec![Box::new(Fake), Box::new(Missing)] };
        assert_eq!(run_headless_with(&c, &f, &Settings::default(), &mut out), 0);
    }
}
