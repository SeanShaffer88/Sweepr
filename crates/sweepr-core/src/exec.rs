//! Run a manager command: no shell, line streaming, timeout, cancel, tree kill.

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, Default)]
pub struct RunOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub cancelled: bool,
}

const POLL: Duration = Duration::from_millis(10);
const DRAIN_AFTER_EXIT: Duration = Duration::from_millis(300);

fn pump<R: Read + Send + 'static>(r: R, s: Stream, tx: Sender<(Stream, String)>) {
    std::thread::spawn(move || {
        for chunk in BufReader::new(r).split(b'\n') {
            let Ok(bytes) = chunk else { break };
            let line = String::from_utf8_lossy(&bytes)
                .trim_end_matches('\r')
                .to_string();
            if tx.send((s, line)).is_err() {
                break;
            }
        }
    });
}

pub fn run(
    program: &Path,
    args: &[String],
    timeout: Duration,
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(Stream, &str),
) -> std::io::Result<RunOutput> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    sys::prepare(&mut cmd);
    let mut child = cmd.spawn()?;
    let guard = sys::TreeGuard::new(&child);

    let (tx, rx) = channel();
    pump(
        child.stdout.take().expect("piped"),
        Stream::Stdout,
        tx.clone(),
    );
    pump(child.stderr.take().expect("piped"), Stream::Stderr, tx);

    let mut out = RunOutput::default();
    let push = |out: &mut RunOutput, s: Stream, l: String, f: &mut dyn FnMut(Stream, &str)| {
        f(s, &l);
        let buf = if s == Stream::Stdout {
            &mut out.stdout
        } else {
            &mut out.stderr
        };
        buf.push_str(&l);
        buf.push('\n');
    };
    let start = Instant::now();
    let mut exited_at: Option<Instant> = None;
    let mut eof = false;
    loop {
        match rx.recv_timeout(POLL) {
            Ok((s, l)) => push(&mut out, s, l, on_line),
            Err(RecvTimeoutError::Disconnected) => {
                eof = true;
                std::thread::sleep(POLL);
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        if exited_at.is_none() {
            if cancel.is_cancelled() {
                out.cancelled = true;
            } else if start.elapsed() >= timeout {
                out.timed_out = true;
            }
            if out.cancelled || out.timed_out {
                guard.kill_tree(&mut child);
                let _ = child.wait();
                break;
            }
            if let Some(status) = child.try_wait()? {
                out.exit_code = status.code();
                exited_at = Some(Instant::now());
            }
        } else if eof || exited_at.is_some_and(|t| t.elapsed() > DRAIN_AFTER_EXIT) {
            break; // a grandchild may still hold the pipes open
        }
    }
    // pick up lines already queued
    while let Ok((s, l)) = rx.try_recv() {
        push(&mut out, s, l, on_line);
    }
    Ok(out)
}

#[cfg(unix)]
mod sys {
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};

    pub fn prepare(cmd: &mut Command) {
        cmd.process_group(0);
    }

    pub struct TreeGuard;
    impl TreeGuard {
        pub fn new(_: &Child) -> Self {
            TreeGuard
        }
        pub fn kill_tree(&self, child: &mut Child) {
            let _ = Command::new("kill")
                .arg("-KILL")
                .arg("--")
                .arg(format!("-{}", child.id()))
                .status();
            let _ = child.kill();
        }
    }
}

#[cfg(windows)]
mod sys {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn prepare(cmd: &mut Command) {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    /// Job object with KILL_ON_JOB_CLOSE: closing it ends the whole process tree.
    /// On a normal exit the flag is cleared first, so detached helpers survive.
    pub struct TreeGuard(std::cell::Cell<HANDLE>);

    impl TreeGuard {
        pub fn new(child: &Child) -> Self {
            // SAFETY: plain Win32 calls on a handle we own. If job creation fails the
            // guard is null and only the direct child is killed on timeout.
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return TreeGuard(std::cell::Cell::new(std::ptr::null_mut()));
                }
                set_flags(job, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE);
                AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE);
                TreeGuard(std::cell::Cell::new(job))
            }
        }

        /// Close the job now (kills the tree), then make sure the direct child is gone.
        pub fn kill_tree(&self, child: &mut Child) {
            let h = self.0.replace(std::ptr::null_mut());
            if !h.is_null() {
                // SAFETY: handle came from `new` and is closed exactly once.
                unsafe { CloseHandle(h) };
            }
            let _ = child.kill();
        }
    }

    unsafe fn set_flags(job: HANDLE, flags: u32) {
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = flags;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
    }

    impl Drop for TreeGuard {
        fn drop(&mut self) {
            let h = self.0.replace(std::ptr::null_mut());
            if !h.is_null() {
                // SAFETY: still-open handle from `new`; clearing the flag before closing
                // means a normal exit does not kill leftover helper processes.
                unsafe {
                    set_flags(h, 0);
                    CloseHandle(h);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Instant;

    fn sh(script: &str) -> (PathBuf, Vec<String>) {
        if cfg!(windows) {
            (PathBuf::from("cmd"), vec!["/C".into(), script.into()])
        } else {
            (PathBuf::from("sh"), vec!["-c".into(), script.into()])
        }
    }

    fn quiet() -> impl FnMut(Stream, &str) {
        |_, _| {}
    }

    #[test]
    fn captures_stdout_stderr_and_exit_code() {
        let script = if cfg!(windows) {
            "echo out& echo err 1>&2& exit 3"
        } else {
            "echo out; echo err >&2; exit 3"
        };
        let (p, a) = sh(script);
        let r = run(
            &p,
            &a,
            Duration::from_secs(10),
            &CancelToken::new(),
            &mut quiet(),
        )
        .unwrap();
        assert_eq!(r.stdout.trim(), "out");
        assert_eq!(r.stderr.trim(), "err");
        assert_eq!(r.exit_code, Some(3));
        assert!(!r.timed_out && !r.cancelled);
    }

    #[test]
    fn timeout_kills_and_flags() {
        let (p, a) = sh(if cfg!(windows) {
            "ping -n 30 127.0.0.1 >nul"
        } else {
            "sleep 30"
        });
        let t = Instant::now();
        let r = run(
            &p,
            &a,
            Duration::from_millis(200),
            &CancelToken::new(),
            &mut quiet(),
        )
        .unwrap();
        assert!(r.timed_out);
        assert!(
            t.elapsed() < Duration::from_secs(2),
            "took {:?}",
            t.elapsed()
        );
    }

    #[test]
    fn cancel_returns_quickly() {
        let (p, a) = sh(if cfg!(windows) {
            "ping -n 30 127.0.0.1 >nul"
        } else {
            "sleep 30"
        });
        let c = CancelToken::new();
        let c2 = c.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            c2.cancel();
        });
        let t = Instant::now();
        let r = run(&p, &a, Duration::from_secs(30), &c, &mut quiet()).unwrap();
        assert!(r.cancelled);
        assert!(
            t.elapsed() < Duration::from_millis(500),
            "took {:?}",
            t.elapsed()
        );
    }

    #[test]
    fn on_line_streams_before_exit() {
        let (p, a) = sh(if cfg!(windows) {
            "echo first& ping -n 3 127.0.0.1 >nul"
        } else {
            "echo first; sleep 1"
        });
        let t = Instant::now();
        let mut first_at = None;
        let r = run(
            &p,
            &a,
            Duration::from_secs(10),
            &CancelToken::new(),
            &mut |_, l| {
                if l == "first" && first_at.is_none() {
                    first_at = Some(t.elapsed());
                }
            },
        )
        .unwrap();
        assert!(
            first_at.unwrap() < Duration::from_millis(700),
            "{first_at:?}"
        );
        assert!(t.elapsed() >= Duration::from_millis(900));
        assert_eq!(r.exit_code, Some(0));
    }

    #[cfg(unix)]
    #[test]
    fn args_are_not_shell_interpreted() {
        let r = run(
            Path::new("echo"),
            &["a & b; $(x)".to_string()],
            Duration::from_secs(5),
            &CancelToken::new(),
            &mut quiet(),
        )
        .unwrap();
        assert_eq!(r.stdout.trim(), "a & b; $(x)");
    }

    #[test]
    fn missing_program_is_io_error() {
        let e = run(
            Path::new("definitely-not-a-real-program-xyz"),
            &[],
            Duration::from_secs(1),
            &CancelToken::new(),
            &mut quiet(),
        );
        assert!(e.is_err());
    }

    #[cfg(windows)]
    #[test]
    fn hung_child_tree_dies_on_timeout() {
        // grandchild ping must not outlive the timeout
        let (p, a) = sh("start /b ping -n 60 -w 1000 127.0.0.1 >nul & ping -n 60 127.0.0.1 >nul");
        let r = run(
            &p,
            &a,
            Duration::from_millis(300),
            &CancelToken::new(),
            &mut quiet(),
        )
        .unwrap();
        assert!(r.timed_out);
        std::thread::sleep(Duration::from_millis(500));
        let out = std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq ping.exe", "/NH"])
            .output()
            .unwrap();
        assert!(!String::from_utf8_lossy(&out.stdout).contains("ping.exe"));
    }
}
