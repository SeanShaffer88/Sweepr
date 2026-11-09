//! One-prompt elevation: the unelevated app relaunches itself with `runas` and
//! the helper streams progress back over a named pipe. The helper only builds
//! commands from adapter templates and validated package ids.

use crate::adapter::PackageManager;
use crate::errors::SwpError;
use crate::exec::{self, CancelToken, RunOutput, Stream};
use crate::job::{validate_job, Job};
use crate::model::{Manager, Scope, UpdatePackage};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemState {
    Started,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Wire {
    Log {
        manager: Manager,
        stderr: bool,
        line: String,
    },
    Item {
        id: String,
        state: ItemState,
        code: Option<i32>,
    },
    End {
        code: i32,
    },
}

pub const EXIT_OK: i32 = 0;
pub const EXIT_SOME_FAILED: i32 = 1;
pub const EXIT_REJECTED: i32 = 3;
pub const EXIT_CANCELLED: i32 = 130;

pub trait JobRunner {
    fn run(
        &mut self,
        program: &Path,
        args: &[String],
        cancel: &CancelToken,
        on_line: &mut dyn FnMut(Stream, &str),
    ) -> std::io::Result<RunOutput>;
}

pub struct RealRunner;

impl JobRunner for RealRunner {
    fn run(
        &mut self,
        program: &Path,
        args: &[String],
        cancel: &CancelToken,
        on_line: &mut dyn FnMut(Stream, &str),
    ) -> std::io::Result<RunOutput> {
        exec::run(program, args, Duration::from_secs(3600), cancel, on_line)
    }
}

pub fn write_wire<W: Write>(w: &mut W, m: &Wire) -> std::io::Result<()> {
    let mut s = serde_json::to_string(m).map_err(std::io::Error::other)?;
    s.push('\n');
    w.write_all(s.as_bytes())?;
    w.flush()
}

/// Reads messages until `End`. EOF before `End` means the helper or pipe died: `SWP-202`.
pub fn read_wire<R: BufRead>(r: R, on_msg: &mut dyn FnMut(&Wire)) -> Result<i32, SwpError> {
    for line in r.lines() {
        let Ok(line) = line else { break };
        let Ok(m) = serde_json::from_str::<Wire>(&line) else {
            continue;
        };
        on_msg(&m);
        if let Wire::End { code } = m {
            return Ok(code);
        }
    }
    Err(SwpError::PipeLost)
}

/// `ShellExecuteExW` failure code to error. 1223 is the user declining UAC.
pub fn launch_error(code: u32) -> SwpError {
    if code == 1223 {
        SwpError::UacDeclined
    } else {
        SwpError::JobRejected(format!(
            "elevated helper failed to start (Win32 error {code})"
        ))
    }
}

/// Random pipe name; the server instance is created first and restricted by DACL.
pub fn pipe_name() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let a = RandomState::new().build_hasher().finish();
    let b = RandomState::new().build_hasher().finish();
    format!(r"\\.\pipe\sweepr-{a:016x}{b:016x}")
}

/// Runs every item once. Validation happens first: a rejected job runs nothing.
pub fn execute_job(
    job: &Job,
    adapters: &[Box<dyn PackageManager>],
    runner: &mut dyn JobRunner,
    cancel: &CancelToken,
    sink: &mut dyn FnMut(Wire),
) -> i32 {
    if validate_job(job).is_err() {
        sink(Wire::End {
            code: EXIT_REJECTED,
        });
        return EXIT_REJECTED;
    }
    let mut failed = false;
    for it in &job.items {
        if cancel.is_cancelled() {
            sink(Wire::End {
                code: EXIT_CANCELLED,
            });
            return EXIT_CANCELLED;
        }
        let item_failed = |sink: &mut dyn FnMut(Wire), code: Option<i32>| {
            sink(Wire::Item {
                id: it.id.clone(),
                state: ItemState::Failed,
                code,
            });
        };
        let Some(adapter) = adapters.iter().find(|a| a.manager() == it.manager) else {
            item_failed(sink, None);
            failed = true;
            continue;
        };
        let pkg = UpdatePackage {
            id: it.id.clone(),
            name: it.id.clone(),
            current_version: String::new(),
            available_version: String::new(),
            manager: it.manager,
            scope: it.scope,
            elevation_required: it.scope == Scope::Machine,
            pinned_by_manager: false,
            ignored: false,
            user_pin: None,
        };
        let (Some(exe), Ok(args)) = (adapter.detect(), adapter.upgrade_args(&pkg)) else {
            item_failed(sink, None);
            failed = true;
            continue;
        };
        sink(Wire::Item {
            id: it.id.clone(),
            state: ItemState::Started,
            code: None,
        });
        let m = it.manager;
        let mut sink_cell = std::cell::RefCell::new(&mut *sink);
        let mut on_line = |s: Stream, l: &str| {
            (sink_cell.get_mut())(Wire::Log {
                manager: m,
                stderr: s == Stream::Stderr,
                line: l.to_string(),
            });
        };
        let res = runner.run(&exe, &args, cancel, &mut on_line);
        match res {
            Ok(o) if o.cancelled => {
                sink(Wire::End {
                    code: EXIT_CANCELLED,
                });
                return EXIT_CANCELLED;
            }
            Ok(o) if o.exit_code == Some(0) => {
                sink(Wire::Item {
                    id: it.id.clone(),
                    state: ItemState::Done,
                    code: Some(0),
                });
            }
            Ok(o) => {
                failed = true;
                item_failed(sink, o.exit_code);
            }
            Err(_) => {
                failed = true;
                item_failed(sink, None);
            }
        }
    }
    let code = if failed { EXIT_SOME_FAILED } else { EXIT_OK };
    sink(Wire::End { code });
    code
}

#[cfg(windows)]
pub mod win {
    //! Windows side: named pipe server with a restrictive DACL, `runas` relaunch,
    //! and the elevated helper entry point. Unverified at runtime until CI/real run.

    use super::*;
    use crate::adapter::all_adapters;
    use crate::job::{read_job, write_job, Journal};
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::*;
    use windows_sys::Win32::Storage::FileSystem::*;
    use windows_sys::Win32::System::Pipes::*;
    use windows_sys::Win32::System::Threading::*;
    use windows_sys::Win32::UI::Shell::*;

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    /// True when this process runs with an elevated token.
    pub fn is_elevated() -> bool {
        unsafe {
            let mut tok: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut tok) == 0 {
                return false;
            }
            let mut el = TOKEN_ELEVATION { TokenIsElevated: 0 };
            let mut n = 0u32;
            let ok = GetTokenInformation(
                tok,
                TokenElevation,
                &mut el as *mut _ as *mut _,
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut n,
            );
            CloseHandle(tok);
            ok != 0 && el.TokenIsElevated != 0
        }
    }

    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    struct PipeReader(HANDLE);
    impl std::io::Read for PipeReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let mut n = 0u32;
            let ok = unsafe {
                ReadFile(
                    self.0,
                    buf.as_mut_ptr(),
                    buf.len() as u32,
                    &mut n,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Ok(0); // broken pipe or any read failure: treated as EOF
            }
            Ok(n as usize)
        }
    }

    struct PipeWriter(HANDLE);
    impl Write for PipeWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let mut n = 0u32;
            let ok = unsafe {
                WriteFile(
                    self.0,
                    buf.as_ptr(),
                    buf.len() as u32,
                    &mut n,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(n as usize)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Creates the pipe server first (so no one can squat the name). DACL: owner,
    /// Administrators (the elevated helper) and SYSTEM only; remote clients rejected.
    fn create_pipe(name: &str) -> Result<Handle, SwpError> {
        unsafe {
            let sddl = wide("D:P(A;;GA;;;OW)(A;;GA;;;BA)(A;;GA;;;SY)".as_ref());
            let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                std::ptr::null_mut(),
            ) == 0
            {
                return Err(SwpError::PipeLost);
            }
            let sa = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: sd,
                bInheritHandle: 0,
            };
            let h = CreateNamedPipeW(
                wide(name.as_ref()).as_ptr(),
                PIPE_ACCESS_INBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                0,
                64 * 1024,
                0,
                &sa,
            );
            LocalFree(sd as *mut _);
            if h == INVALID_HANDLE_VALUE {
                return Err(SwpError::PipeLost);
            }
            Ok(Handle(h))
        }
    }

    /// Runs the system queue in an elevated helper: one UAC prompt for the whole batch.
    pub fn run_elevated(
        data_dir: &Path,
        job: &Job,
        cancel: &CancelToken,
        sink: &mut dyn FnMut(&Wire),
    ) -> Result<i32, SwpError> {
        let job_path =
            write_job(data_dir, job).map_err(|e| SwpError::JobRejected(e.to_string()))?;
        let name = pipe_name();
        let pipe = create_pipe(&name)?;
        let exe = std::env::current_exe().map_err(|e| SwpError::JobRejected(e.to_string()))?;
        let params = format!("--elevated-run \"{}\" --pipe {}", job_path.display(), name);
        let (verb, file, par) = (
            wide("runas".as_ref()),
            wide(exe.as_os_str()),
            wide(params.as_ref()),
        );
        let mut sei: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
        sei.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        sei.fMask = SEE_MASK_NOCLOSEPROCESS;
        sei.lpVerb = verb.as_ptr();
        sei.lpFile = file.as_ptr();
        sei.lpParameters = par.as_ptr();
        sei.nShow = 0; // SW_HIDE
        if unsafe { ShellExecuteExW(&mut sei) } == 0 {
            return Err(launch_error(unsafe { GetLastError() }));
        }
        let proc = Handle(sei.hProcess);
        // Poll for the helper to connect; bail if it exits first or the user cancels.
        loop {
            let ok = unsafe { ConnectNamedPipe(pipe.0, std::ptr::null_mut()) };
            let err = unsafe { GetLastError() };
            if ok != 0 || err == ERROR_PIPE_CONNECTED {
                break;
            }
            if err != ERROR_PIPE_LISTENING && err != ERROR_NO_DATA {
                return Err(SwpError::PipeLost);
            }
            if cancel.is_cancelled()
                || (!proc.0.is_null()
                    && unsafe { WaitForSingleObject(proc.0, 50) } == WAIT_OBJECT_0)
            {
                return Err(SwpError::PipeLost);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let wait = PIPE_WAIT;
        unsafe { SetNamedPipeHandleState(pipe.0, &wait, std::ptr::null(), std::ptr::null()) };
        let r = read_wire(std::io::BufReader::new(PipeReader(pipe.0)), &mut |m| {
            sink(m)
        });
        if !proc.0.is_null() {
            unsafe { WaitForSingleObject(proc.0, 5000) };
        }
        r
    }

    /// Entry point of `--elevated-run <job> --pipe <name>`. Returns the process exit code.
    pub fn run_helper(job_path: &Path, pipe: &str) -> i32 {
        let h = unsafe {
            CreateFileW(
                wide(pipe.as_ref()).as_ptr(),
                GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            return EXIT_REJECTED;
        }
        let out = Handle(h);
        let mut w = PipeWriter(out.0);
        let job = match read_job(job_path) {
            Ok(j) => j,
            Err(_) => {
                let _ = write_wire(
                    &mut w,
                    &Wire::End {
                        code: EXIT_REJECTED,
                    },
                );
                return EXIT_REJECTED;
            }
        };
        let dir: PathBuf = job_path
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let journal = Journal::open(&dir, &job.id);
        let mut sink = |m: Wire| {
            if let Wire::Item { id, state, .. } = &m {
                journal.append(&format!("{state:?} {id}").to_lowercase());
            }
            let _ = write_wire(&mut w, &m);
        };
        let code = execute_job(
            &job,
            &all_adapters(),
            &mut RealRunner,
            &CancelToken::new(),
            &mut sink,
        );
        journal.finish();
        code
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::SwpError;
    use crate::exec::{CancelToken, RunOutput, Stream};
    use crate::job::{Job, JobItem};
    use crate::model::{Manager, Scope, UpdatePackage};
    use std::path::{Path, PathBuf};

    struct Counting {
        calls: Vec<Vec<String>>,
        fail_first_with: Option<i32>,
    }
    impl JobRunner for Counting {
        fn run(
            &mut self,
            _program: &Path,
            args: &[String],
            _cancel: &CancelToken,
            on_line: &mut dyn FnMut(Stream, &str),
        ) -> std::io::Result<RunOutput> {
            on_line(Stream::Stdout, "working");
            self.calls.push(args.to_vec());
            let code = if self.calls.len() == 1 {
                self.fail_first_with.unwrap_or(0)
            } else {
                0
            };
            Ok(RunOutput {
                exit_code: Some(code),
                ..Default::default()
            })
        }
    }

    struct FakeChoco;
    impl crate::adapter::PackageManager for FakeChoco {
        fn manager(&self) -> Manager {
            Manager::Choco
        }
        fn detect(&self) -> Option<PathBuf> {
            Some(PathBuf::from("/fake/choco"))
        }
        fn discovery_args(&self) -> Vec<String> {
            vec![]
        }
        fn parse(&self, _: &str) -> Result<Vec<UpdatePackage>, SwpError> {
            Ok(vec![])
        }
        fn upgrade_args(&self, p: &UpdatePackage) -> Result<Vec<String>, SwpError> {
            Ok(vec!["upgrade".into(), p.id.clone(), "-y".into()])
        }
    }

    fn job(ids: &[&str]) -> Job {
        Job {
            id: "j1".into(),
            items: ids
                .iter()
                .map(|i| JobItem {
                    manager: Manager::Choco,
                    id: (*i).into(),
                    scope: Scope::Machine,
                })
                .collect(),
        }
    }

    fn exec(j: &Job, r: &mut Counting) -> (i32, Vec<Wire>) {
        let adapters: Vec<Box<dyn crate::adapter::PackageManager>> = vec![Box::new(FakeChoco)];
        let mut msgs = vec![];
        let code = execute_job(j, &adapters, r, &CancelToken::new(), &mut |m| msgs.push(m));
        (code, msgs)
    }

    #[test]
    fn hostile_id_job_runs_nothing() {
        let mut r = Counting {
            calls: vec![],
            fail_first_with: None,
        };
        let (code, msgs) = exec(&job(&["git", "foo & calc"]), &mut r);
        assert_eq!(code, 3);
        assert!(r.calls.is_empty());
        assert!(matches!(msgs.last(), Some(Wire::End { code: 3 })));
    }

    #[test]
    fn each_item_runs_once_and_reports() {
        let mut r = Counting {
            calls: vec![],
            fail_first_with: None,
        };
        let (code, msgs) = exec(&job(&["git", "7zip"]), &mut r);
        assert_eq!(code, 0);
        assert_eq!(r.calls.len(), 2);
        let done = msgs
            .iter()
            .filter(|m| {
                matches!(
                    m,
                    Wire::Item {
                        state: ItemState::Done,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(done, 2);
        assert!(matches!(msgs.last(), Some(Wire::End { code: 0 })));
    }

    #[test]
    fn failed_item_continues_and_sets_exit_1() {
        let mut r = Counting {
            calls: vec![],
            fail_first_with: Some(1603),
        };
        let (code, msgs) = exec(&job(&["git", "7zip"]), &mut r);
        assert_eq!(code, 1);
        assert_eq!(r.calls.len(), 2);
        assert!(msgs.iter().any(|m| matches!(
            m,
            Wire::Item {
                state: ItemState::Failed,
                code: Some(1603),
                ..
            }
        )));
    }

    #[test]
    fn wire_roundtrip_and_end_returns_code() {
        let mut buf = Vec::new();
        write_wire(
            &mut buf,
            &Wire::Log {
                manager: Manager::Choco,
                stderr: false,
                line: "hi".into(),
            },
        )
        .unwrap();
        write_wire(&mut buf, &Wire::End { code: 0 }).unwrap();
        let mut seen = 0;
        let code = read_wire(std::io::Cursor::new(buf), &mut |_| seen += 1).unwrap();
        assert_eq!((code, seen), (0, 2));
    }

    #[test]
    fn pipe_loss_midway_yields_202_and_no_rerun() {
        let d = std::env::temp_dir().join(format!("sweepr-pipeloss-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let j = job(&["git"]);
        crate::job::write_job(&d, &j).unwrap();
        let mut buf = Vec::new();
        write_wire(
            &mut buf,
            &Wire::Item {
                id: "git".into(),
                state: ItemState::Started,
                code: None,
            },
        )
        .unwrap();
        let e = read_wire(std::io::Cursor::new(buf), &mut |_| {}).unwrap_err();
        assert_eq!(e.code(), "SWP-202");
        assert_eq!(crate::job::unfinished(&d), vec!["j1".to_string()]);
    }

    #[test]
    fn launch_error_maps_1223_to_uac_declined() {
        assert_eq!(launch_error(1223).code(), "SWP-201");
        assert_eq!(launch_error(5).code(), "SWP-203");
    }
}
