#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod cli;
// Task 11 wires the remaining model/theme items; drop this allow then.
#[cfg_attr(not(windows), allow(dead_code))]
// window code is Windows-only; logic is unit-tested on any host
mod ui;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(windows)]
    if !args.is_empty() {
        attach_console();
    }
    let (mut out, mut err) = (std::io::stdout(), std::io::stderr());
    if let Some(code) = cli::run(&args, &mut out, &mut err) {
        std::process::exit(code);
    }
    start_gui();
}

#[cfg(windows)]
fn start_gui() {
    use sweepr_core::settings::{data_dir, Settings};
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_default();
    let dir = data_dir(&exe_dir);
    let loaded = Settings::load(&dir);
    std::process::exit(ui::window::run_gui(loaded.settings, loaded.notice, dir));
}

#[cfg(not(windows))]
fn start_gui() {
    eprintln!("sweepr: the window is Windows only. Try: sweepr --headless");
    std::process::exit(2);
}

/// A GUI-subsystem exe has no console; attach to the parent's so CLI output shows.
#[cfg(windows)]
fn attach_console() {
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows_sys::Win32::System::Console::{
        AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE,
        STD_OUTPUT_HANDLE,
    };
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return;
        }
        let conout: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
        for which in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            let h = GetStdHandle(which);
            if h.is_null() || h == INVALID_HANDLE_VALUE {
                let n = CreateFileW(
                    conout.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_WRITE,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    0,
                    std::ptr::null_mut(),
                );
                if n != INVALID_HANDLE_VALUE {
                    SetStdHandle(which, n);
                }
            }
        }
    }
}
