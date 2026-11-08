//! Modal windows: settings and the Ctrl+K command palette. Windows only; Unverified at
//! runtime until run on a real machine.

use super::actions::{filter_commands, Cmd, COMMANDS};
use super::geometry::to_device;
use super::theme::{colorref, palette};
use std::path::Path;
use sweepr_core::settings::{Settings, Theme};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{SetWindowTheme, BST_CHECKED, BST_UNCHECKED};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, SetFocus, VK_DOWN, VK_ESCAPE, VK_RETURN, VK_UP,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct Dlg {
    dark: bool,
    dpi: u32,
    brush: HBRUSH,
    done: bool,
    ok: bool,
    font: HFONT,
    // settings controls
    chk: [HWND; 3],
    timeout: HWND,
    theme: HWND,
    // palette controls
    edit: HWND,
    list: HWND,
    choice: Option<Cmd>,
    shown: Vec<Cmd>,
    data_dir: String,
}

const ID_OK: isize = 1;
const ID_CANCEL: isize = 2;
const ID_OPEN_DIR: isize = 210;
const ID_EDIT: isize = 220;
const ID_LIST: isize = 221;
const EN_CHANGE_CODE: u32 = 0x0300;
const LBN_DBLCLK_CODE: u32 = 2;

unsafe fn dlg_of(h: HWND) -> Option<&'static mut Dlg> {
    (GetWindowLongPtrW(h, GWLP_USERDATA) as *mut Dlg).as_mut()
}

unsafe fn child(
    parent: HWND,
    class: &str,
    text: &str,
    style: u32,
    r: (i32, i32, i32, i32),
    id: isize,
    d: &Dlg,
) -> HWND {
    let h = CreateWindowExW(
        0,
        wide(class).as_ptr(),
        wide(text).as_ptr(),
        WS_CHILD | WS_VISIBLE | style,
        to_device(r.0, d.dpi),
        to_device(r.1, d.dpi),
        to_device(r.2, d.dpi),
        to_device(r.3, d.dpi),
        parent,
        id as HMENU,
        GetModuleHandleW(std::ptr::null()),
        std::ptr::null(),
    );
    SendMessageW(h, WM_SETFONT, d.font as WPARAM, 1);
    if d.dark {
        SetWindowTheme(h, wide("DarkMode_Explorer").as_ptr(), std::ptr::null());
    }
    h
}

unsafe fn make_font(dpi: u32) -> HFONT {
    CreateFontW(
        -((13 * dpi as i32 + 48) / 96),
        0,
        0,
        0,
        FW_NORMAL as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET as u32,
        OUT_DEFAULT_PRECIS as u32,
        CLIP_DEFAULT_PRECIS as u32,
        CLEARTYPE_QUALITY as u32,
        (DEFAULT_PITCH | FF_DONTCARE) as u32,
        wide("Segoe UI").as_ptr(),
    )
}

unsafe fn register(name: &str, proc_: WNDPROC) {
    let cname = wide(name);
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: proc_,
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: GetModuleHandleW(std::ptr::null()),
        hIcon: std::ptr::null_mut(),
        hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: cname.as_ptr(),
        hIconSm: std::ptr::null_mut(),
    };
    RegisterClassExW(&wc);
}

unsafe extern "system" fn dlg_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = &*(lparam as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let Some(d) = dlg_of(hwnd) else {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    };
    match msg {
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as isize;
            let code = ((wparam >> 16) & 0xFFFF) as u32;
            match id {
                ID_OK => {
                    d.ok = true;
                    d.done = true;
                }
                ID_CANCEL => d.done = true,
                ID_OPEN_DIR => {
                    ShellExecuteW(
                        std::ptr::null_mut(),
                        wide("open").as_ptr(),
                        wide(&d.data_dir).as_ptr(),
                        std::ptr::null(),
                        std::ptr::null(),
                        SW_SHOWNORMAL,
                    );
                }
                ID_EDIT if code == EN_CHANGE_CODE => refill(d),
                ID_LIST if code == LBN_DBLCLK_CODE => choose(d),
                _ => {}
            }
            0
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX | WM_CTLCOLORBTN
        | WM_CTLCOLORDLG => {
            let p = palette(d.dark);
            SetTextColor(wparam as HDC, colorref(p.text));
            SetBkColor(wparam as HDC, colorref(p.bg0));
            d.brush as LRESULT
        }
        WM_ERASEBKGND => {
            let mut r: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut r);
            FillRect(wparam as HDC, &r, d.brush);
            1
        }
        WM_CLOSE => {
            d.done = true;
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn refill(d: &mut Dlg) {
    let mut buf = [0u16; 128];
    let n = GetWindowTextW(d.edit, buf.as_mut_ptr(), 128) as usize;
    let q = String::from_utf16_lossy(&buf[..n]);
    d.shown = filter_commands(&q);
    SendMessageW(d.list, LB_RESETCONTENT, 0, 0);
    for c in &d.shown {
        let (_, title, key) = COMMANDS.iter().find(|(cc, _, _)| cc == c).unwrap();
        let line = if key.is_empty() {
            title.to_string()
        } else {
            format!("{title}    ({key})")
        };
        SendMessageW(d.list, LB_ADDSTRING, 0, wide(&line).as_ptr() as LPARAM);
    }
    SendMessageW(d.list, LB_SETCURSEL, 0, 0);
}

unsafe fn choose(d: &mut Dlg) {
    let i = SendMessageW(d.list, LB_GETCURSEL, 0, 0);
    if i >= 0 {
        d.choice = d.shown.get(i as usize).copied();
        d.done = true;
    }
}

unsafe fn run_modal(parent: HWND, hwnd: HWND, d: *mut Dlg, palette_mode: bool) {
    EnableWindow(parent, 0);
    ShowWindow(hwnd, SW_SHOW);
    let mut msg: MSG = std::mem::zeroed();
    while !(*d).done && GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
        if palette_mode && msg.message == WM_KEYDOWN {
            let k = msg.wParam as u16;
            if k == VK_RETURN {
                choose(&mut *d);
                continue;
            }
            if k == VK_ESCAPE {
                (*d).done = true;
                continue;
            }
            if k == VK_DOWN || k == VK_UP {
                let cur = SendMessageW((*d).list, LB_GETCURSEL, 0, 0);
                let n = (*d).shown.len() as isize;
                if n > 0 {
                    let next = if k == VK_DOWN {
                        (cur + 1).min(n - 1)
                    } else {
                        (cur - 1).max(0)
                    };
                    SendMessageW((*d).list, LB_SETCURSEL, next as usize, 0);
                }
                continue;
            }
        }
        if IsDialogMessageW(hwnd, &msg) == 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    EnableWindow(parent, 1);
}

fn new_dlg(dark: bool, dpi: u32, data_dir: &Path) -> Box<Dlg> {
    unsafe {
        Box::new(Dlg {
            dark,
            dpi,
            brush: CreateSolidBrush(colorref(palette(dark).bg0)),
            done: false,
            ok: false,
            font: make_font(dpi),
            chk: [std::ptr::null_mut(); 3],
            timeout: std::ptr::null_mut(),
            theme: std::ptr::null_mut(),
            edit: std::ptr::null_mut(),
            list: std::ptr::null_mut(),
            choice: None,
            shown: Vec::new(),
            data_dir: data_dir.display().to_string(),
        })
    }
}

unsafe fn free_dlg(d: &Dlg) {
    DeleteObject(d.brush);
    DeleteObject(d.font);
}

unsafe fn center(parent: HWND, w: i32, h: i32) -> (i32, i32) {
    let mut r: RECT = std::mem::zeroed();
    GetWindowRect(parent, &mut r);
    (
        r.left + ((r.right - r.left) - w) / 2,
        r.top + ((r.bottom - r.top) - h) / 3,
    )
}

/// Settings modal. Returns the edited settings on OK, `None` on cancel.
pub fn settings_dialog(
    parent: HWND,
    s: &Settings,
    dpi: u32,
    dark: bool,
    data_dir: &Path,
) -> Option<Settings> {
    unsafe {
        register("SweeprSettings", Some(dlg_proc));
        let mut d = new_dlg(dark, dpi, data_dir);
        let (w, h) = (to_device(440, dpi), to_device(300, dpi));
        let (x, y) = center(parent, w, h);
        let hwnd = CreateWindowExW(
            WS_EX_DLGMODALFRAME,
            wide("SweeprSettings").as_ptr(),
            wide("Sweepr settings").as_ptr(),
            WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN,
            x,
            y,
            w,
            h,
            parent,
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            &mut *d as *mut Dlg as *const _,
        );
        let dp = &mut *d as *mut Dlg;
        child(
            hwnd,
            "STATIC",
            "Check these package managers:",
            0,
            (16, 14, 400, 20),
            0,
            &d,
        );
        let names = ["WinGet", "Chocolatey", "Scoop"];
        let on = [s.sources.winget, s.sources.choco, s.sources.scoop];
        for i in 0..3 {
            d.chk[i] = child(
                hwnd,
                "BUTTON",
                names[i],
                (BS_AUTOCHECKBOX as u32) | WS_TABSTOP,
                (16 + 130 * i as i32, 38, 120, 22),
                300 + i as isize,
                &d,
            );
            SendMessageW(
                d.chk[i],
                BM_SETCHECK,
                if on[i] { BST_CHECKED } else { BST_UNCHECKED } as WPARAM,
                0,
            );
        }
        child(
            hwnd,
            "STATIC",
            "Seconds to wait for each manager (5 to 3600):",
            0,
            (16, 76, 400, 20),
            0,
            &d,
        );
        d.timeout = child(
            hwnd,
            "EDIT",
            &s.timeout_secs.to_string(),
            (ES_NUMBER as u32) | WS_BORDER | WS_TABSTOP,
            (16, 98, 80, 24),
            301,
            &d,
        );
        child(hwnd, "STATIC", "Theme:", 0, (16, 138, 80, 20), 0, &d);
        d.theme = child(
            hwnd,
            "COMBOBOX",
            "",
            (CBS_DROPDOWNLIST as u32) | WS_TABSTOP,
            (16, 160, 160, 120),
            302,
            &d,
        );
        for t in ["System", "Dark", "Light"] {
            SendMessageW(d.theme, CB_ADDSTRING, 0, wide(t).as_ptr() as LPARAM);
        }
        let sel = match s.theme {
            Theme::System => 0,
            Theme::Dark => 1,
            Theme::Light => 2,
        };
        SendMessageW(d.theme, CB_SETCURSEL, sel, 0);
        let loc = format!("Data folder: {}", data_dir.display());
        child(hwnd, "STATIC", &loc, 0, (16, 198, 400, 20), 0, &d);
        child(
            hwnd,
            "BUTTON",
            "Open data folder",
            WS_TABSTOP,
            (16, 228, 130, 28),
            ID_OPEN_DIR,
            &d,
        );
        child(
            hwnd,
            "BUTTON",
            "OK",
            (BS_DEFPUSHBUTTON as u32) | WS_TABSTOP,
            (250, 228, 80, 28),
            ID_OK,
            &d,
        );
        child(
            hwnd,
            "BUTTON",
            "Cancel",
            WS_TABSTOP,
            (340, 228, 80, 28),
            ID_CANCEL,
            &d,
        );
        SetFocus(d.chk[0]);
        run_modal(parent, hwnd, dp, false);
        let mut out = None;
        if d.ok {
            let mut n = s.clone();
            n.sources.winget = SendMessageW(d.chk[0], BM_GETCHECK, 0, 0) == BST_CHECKED as isize;
            n.sources.choco = SendMessageW(d.chk[1], BM_GETCHECK, 0, 0) == BST_CHECKED as isize;
            n.sources.scoop = SendMessageW(d.chk[2], BM_GETCHECK, 0, 0) == BST_CHECKED as isize;
            let mut buf = [0u16; 16];
            let len = GetWindowTextW(d.timeout, buf.as_mut_ptr(), 16) as usize;
            if let Ok(v) = String::from_utf16_lossy(&buf[..len]).trim().parse::<u64>() {
                n.timeout_secs = v.clamp(5, 3600);
            }
            n.theme = match SendMessageW(d.theme, CB_GETCURSEL, 0, 0) {
                1 => Theme::Dark,
                2 => Theme::Light,
                _ => Theme::System,
            };
            out = Some(n);
        }
        DestroyWindow(hwnd);
        free_dlg(&d);
        out
    }
}

/// Ctrl+K palette. Returns the chosen command, or `None` if dismissed.
pub fn palette_dialog(parent: HWND, dpi: u32, dark: bool) -> Option<Cmd> {
    unsafe {
        register("SweeprPalette", Some(dlg_proc));
        let mut d = new_dlg(dark, dpi, Path::new(""));
        let (w, h) = (to_device(480, dpi), to_device(280, dpi));
        let (x, y) = center(parent, w, h);
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            wide("SweeprPalette").as_ptr(),
            wide("Commands").as_ptr(),
            WS_POPUP | WS_BORDER | WS_CLIPCHILDREN,
            x,
            y,
            w,
            h,
            parent,
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            &mut *d as *mut Dlg as *const _,
        );
        let dp = &mut *d as *mut Dlg;
        d.edit = child(
            hwnd,
            "EDIT",
            "",
            WS_BORDER | WS_TABSTOP,
            (8, 8, 462, 26),
            ID_EDIT,
            &d,
        );
        d.list = child(
            hwnd,
            "LISTBOX",
            "",
            (LBS_NOTIFY as u32) | WS_TABSTOP,
            (8, 42, 462, 226),
            ID_LIST,
            &d,
        );
        refill(&mut d);
        SetFocus(d.edit);
        run_modal(parent, hwnd, dp, true);
        let out = d.choice;
        DestroyWindow(hwnd);
        free_dlg(&d);
        out
    }
}
