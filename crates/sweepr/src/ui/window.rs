//! Main window: pure Win32 + Common Controls v6. Windows only.
//! Runtime behavior is Unverified until run on a real machine (see AUDIT.md).

use super::geometry::{default_window, restore, to_device, to_saved};
use super::model::{status_line, Col, ListModel, MgrState, Sort};
use super::theme::{colorref, palette, Palette, DENSE_PX, ROW_HEIGHT_PX};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use sweepr_core::adapter::all_adapters;
use sweepr_core::errors::SwpError;
use sweepr_core::exec::CancelToken;
use sweepr_core::model::Manager;
use sweepr_core::orchestrator::{discover, Event, Status};
use sweepr_core::settings::{Rect, Settings, Theme};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{SetFocus, VK_F5, VK_SPACE};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WM_APP_EVENT: u32 = WM_APP + 1;
const CLASS: &str = "SweeprMainWindow";
const ID_LIST: isize = 101;
const ID_STATUS: isize = 102;
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const SS_LEFTNOWORDWRAP: u32 = 0x0000_000C;
const SS_CENTERIMAGE: u32 = 0x0000_0200;
const MANAGERS: [Manager; 3] = [Manager::Winget, Manager::Choco, Manager::Scoop];
const COLS: [(Col, &str, i32); 6] = [
    (Col::Name, "Name", 260),
    (Col::Id, "Id", 220),
    (Col::Current, "Current", 110),
    (Col::Available, "Available", 110),
    (Col::Source, "Source", 80),
    (Col::Scope, "Scope", 80),
];

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct App {
    hwnd: HWND,
    list: HWND,
    status: HWND,
    settings: Settings,
    notice: Option<SwpError>,
    data_dir: PathBuf,
    model: ListModel,
    queue: Arc<Mutex<VecDeque<Event>>>,
    states: Vec<(Manager, MgrState)>,
    dark: bool,
    dpi: u32,
    font: HFONT,
    bg0: HBRUSH,
    cancel: CancelToken,
    scratch: Vec<u16>,
}

unsafe fn app_of(hwnd: HWND) -> Option<&'static mut App> {
    let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
    p.as_mut()
}

fn system_prefers_dark() -> bool {
    unsafe {
        let mut v: u32 = 1;
        let mut n = std::mem::size_of::<u32>() as u32;
        let key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
        let val = wide("AppsUseLightTheme");
        let r = RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            val.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut v as *mut u32 as *mut _,
            &mut n,
        );
        r == 0 && v == 0
    }
}

fn is_dark(s: &Settings) -> bool {
    match s.theme {
        Theme::Dark => true,
        Theme::Light => false,
        Theme::System => system_prefers_dark(),
    }
}

unsafe extern "system" fn monitor_cb(_: HMONITOR, _: HDC, r: *mut RECT, data: LPARAM) -> BOOL {
    let v = &mut *(data as *mut Vec<Rect>);
    let r = &*r;
    v.push(Rect {
        x: r.left,
        y: r.top,
        w: r.right - r.left,
        h: r.bottom - r.top,
    });
    1
}

fn monitors() -> Vec<Rect> {
    let mut v: Vec<Rect> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(monitor_cb),
            &mut v as *mut _ as LPARAM,
        );
    }
    v
}

/// Creates the window and runs the message loop. Returns the process exit code.
pub fn run_gui(settings: Settings, notice: Option<SwpError>, data_dir: PathBuf) -> i32 {
    unsafe {
        let mutex_name = wide("Local\\{6F3A2B1C-8D4E-4A7B-9C5D-2E1F0A3B4C5D}-Sweepr");
        let m = CreateMutexW(std::ptr::null(), 0, mutex_name.as_ptr());
        if !m.is_null() && GetLastError() == ERROR_ALREADY_EXISTS {
            let existing = FindWindowW(wide(CLASS).as_ptr(), std::ptr::null());
            if !existing.is_null() {
                ShowWindow(existing, SW_RESTORE);
                SetForegroundWindow(existing);
            }
            return 0;
        }
        let icc = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LISTVIEW_CLASSES,
        };
        InitCommonControlsEx(&icc);

        let hinst = GetModuleHandleW(std::ptr::null());
        let class = wide(CLASS);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: std::ptr::null_mut(),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
            hIconSm: std::ptr::null_mut(),
        };
        RegisterClassExW(&wc);

        let dpi = GetDpiForSystem();
        let mons = monitors();
        let ws = match settings.window {
            Some(w) => restore(w, dpi, &mons),
            None => default_window(dpi, &mons),
        };
        let dark = is_dark(&settings);
        let app = Box::new(App {
            hwnd: std::ptr::null_mut(),
            list: std::ptr::null_mut(),
            status: std::ptr::null_mut(),
            settings,
            notice,
            data_dir,
            model: ListModel::default(),
            queue: Arc::new(Mutex::new(VecDeque::new())),
            states: Vec::new(),
            dark,
            dpi,
            font: std::ptr::null_mut(),
            bg0: std::ptr::null_mut(),
            cancel: CancelToken::new(),
            scratch: Vec::new(),
        });
        let app_ptr = Box::into_raw(app);
        let title = wide("Sweepr");
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            ws.x,
            ws.y,
            ws.w,
            ws.h,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinst,
            app_ptr as *const _,
        );
        if hwnd.is_null() {
            drop(Box::from_raw(app_ptr));
            return 1;
        }
        ShowWindow(
            hwnd,
            if ws.maximized {
                SW_SHOWMAXIMIZED
            } else {
                SW_SHOW
            },
        );
        UpdateWindow(hwnd);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if IsDialogMessageW(hwnd, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        drop(Box::from_raw(app_ptr));
        msg.wParam as i32
    }
}

unsafe fn rebuild_font(app: &mut App) {
    if !app.font.is_null() {
        DeleteObject(app.font);
    }
    let h = -((DENSE_PX * app.dpi as i32 + 48) / 96);
    app.font = CreateFontW(
        h,
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
    );
    for w in [app.list, app.status] {
        SendMessageW(w, WM_SETFONT, app.font as WPARAM, 1);
    }
    let hdr = SendMessageW(app.list, LVM_GETHEADER, 0, 0) as HWND;
    if !hdr.is_null() {
        SendMessageW(hdr, WM_SETFONT, app.font as WPARAM, 1);
    }
}

/// An empty image list sets the report-view row height (TesselGUI Compact = 28 px).
unsafe fn set_row_height(app: &mut App) {
    let il = ImageList_Create(1, to_device(ROW_HEIGHT_PX, app.dpi), ILC_COLOR32, 1, 1);
    SendMessageW(
        app.list,
        LVM_SETIMAGELIST,
        LVSIL_SMALL as WPARAM,
        il as LPARAM,
    );
}

unsafe fn apply_theme(app: &mut App) {
    let p: Palette = palette(app.dark);
    if !app.bg0.is_null() {
        DeleteObject(app.bg0);
    }
    app.bg0 = CreateSolidBrush(colorref(p.bg0));
    let on: BOOL = app.dark as BOOL;
    DwmSetWindowAttribute(
        app.hwnd,
        DWMWA_USE_IMMERSIVE_DARK_MODE,
        &on as *const _ as *const _,
        4,
    );
    let theme = if app.dark {
        "DarkMode_Explorer"
    } else {
        "Explorer"
    };
    SetWindowTheme(app.list, wide(theme).as_ptr(), std::ptr::null());
    SendMessageW(app.list, LVM_SETBKCOLOR, 0, colorref(p.bg1) as LPARAM);
    SendMessageW(app.list, LVM_SETTEXTBKCOLOR, 0, colorref(p.bg1) as LPARAM);
    SendMessageW(app.list, LVM_SETTEXTCOLOR, 0, colorref(p.text) as LPARAM);
    InvalidateRect(app.hwnd, std::ptr::null(), 1);
}

unsafe fn insert_columns(app: &App) {
    for (i, (_, name, w)) in COLS.iter().enumerate() {
        let text = wide(name);
        let mut col: LVCOLUMNW = std::mem::zeroed();
        col.mask = LVCF_TEXT | LVCF_WIDTH | LVCF_FMT | LVCF_SUBITEM;
        col.fmt = LVCFMT_LEFT;
        col.cx = to_device(*w, app.dpi);
        col.pszText = text.as_ptr() as *mut u16;
        col.iSubItem = i as i32;
        SendMessageW(app.list, LVM_INSERTCOLUMNW, i, &col as *const _ as LPARAM);
    }
}

unsafe fn update_sort_markers(app: &App) {
    let s = app.model.sort();
    for (i, (c, name, _)) in COLS.iter().enumerate() {
        let marker = if *c == s.col {
            if s.asc {
                " \u{25B2}"
            } else {
                " \u{25BC}"
            }
        } else {
            ""
        };
        let text = wide(&format!("{name}{marker}"));
        let mut col: LVCOLUMNW = std::mem::zeroed();
        col.mask = LVCF_TEXT;
        col.pszText = text.as_ptr() as *mut u16;
        SendMessageW(app.list, LVM_SETCOLUMNW, i, &col as *const _ as LPARAM);
    }
}

unsafe fn create_controls(app: &mut App) {
    let hinst = GetModuleHandleW(std::ptr::null());
    app.list = CreateWindowExW(
        0,
        WC_LISTVIEWW,
        std::ptr::null(),
        WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | LVS_REPORT
            | LVS_OWNERDATA
            | LVS_SHOWSELALWAYS
            | LVS_SINGLESEL,
        0,
        0,
        10,
        10,
        app.hwnd,
        ID_LIST as HMENU,
        hinst,
        std::ptr::null(),
    );
    app.status = CreateWindowExW(
        0,
        wide("STATIC").as_ptr(),
        std::ptr::null(),
        WS_CHILD | WS_VISIBLE | SS_LEFTNOWORDWRAP | SS_CENTERIMAGE,
        0,
        0,
        10,
        10,
        app.hwnd,
        ID_STATUS as HMENU,
        hinst,
        std::ptr::null(),
    );
    SendMessageW(
        app.list,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        0,
        (LVS_EX_FULLROWSELECT
            | LVS_EX_GRIDLINES
            | LVS_EX_CHECKBOXES
            | LVS_EX_DOUBLEBUFFER
            | LVS_EX_LABELTIP) as LPARAM,
    );
    insert_columns(app);
    rebuild_font(app);
    set_row_height(app);
    apply_theme(app);
    update_sort_markers(app);
    layout(app);
}

unsafe fn layout(app: &App) {
    let mut r: RECT = std::mem::zeroed();
    GetClientRect(app.hwnd, &mut r);
    let sh = to_device(24, app.dpi);
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    MoveWindow(app.list, 0, 0, w, (h - sh).max(0), 1);
    MoveWindow(
        app.status,
        to_device(8, app.dpi),
        (h - sh).max(0),
        (w - to_device(16, app.dpi)).max(0),
        sh,
        1,
    );
}

unsafe fn refresh_status(app: &App) {
    let t = status_line(&app.states, app.model.total(), app.model.selected().len());
    SetWindowTextW(app.status, wide(&t).as_ptr());
}

fn set_state(app: &mut App, m: Manager, s: MgrState) {
    match app.states.iter_mut().find(|(mm, _)| *mm == m) {
        Some(e) => e.1 = s,
        None => app.states.push((m, s)),
    }
}

/// Starts one discovery run. A forwarder thread queues events and wakes the window.
unsafe fn start_discovery(app: &mut App) {
    app.cancel.cancel();
    app.cancel = CancelToken::new();
    app.queue.lock().unwrap().clear();
    app.states.clear();
    for m in MANAGERS {
        if enabled(&app.settings, m) {
            set_state(app, m, MgrState::Checking);
        }
    }
    let (tx, rx) = std::sync::mpsc::channel();
    discover(all_adapters(), &app.settings, app.cancel.clone(), tx);
    let queue = app.queue.clone();
    let hwnd = app.hwnd as usize;
    std::thread::spawn(move || {
        for ev in rx {
            let done = matches!(ev, Event::Done);
            queue.lock().unwrap().push_back(ev);
            unsafe { PostMessageW(hwnd as HWND, WM_APP_EVENT, 0, 0) };
            if done {
                break;
            }
        }
    });
    refresh_status(app);
}

fn enabled(s: &Settings, m: Manager) -> bool {
    match m {
        Manager::Winget => s.sources.winget,
        Manager::Choco => s.sources.choco,
        Manager::Scoop => s.sources.scoop,
    }
}

unsafe fn drain_events(app: &mut App) {
    let events: Vec<Event> = app.queue.lock().unwrap().drain(..).collect();
    for ev in events {
        match ev {
            Event::Found(m, pkgs) => {
                app.model.replace_manager(m, pkgs);
                SendMessageW(
                    app.list,
                    LVM_SETITEMCOUNT,
                    app.model.visible_len(),
                    LVSICF_NOSCROLL as LPARAM,
                );
                InvalidateRect(app.list, std::ptr::null(), 0);
            }
            Event::ManagerStatus(m, st) => {
                let s = match st {
                    Status::Running => MgrState::Checking,
                    Status::Ok(n) => MgrState::Found(n),
                    Status::Cancelled => MgrState::Cancelled,
                    Status::Failed(SwpError::ManagerMissing(_)) => MgrState::Missing,
                    Status::Failed(e) => {
                        MgrState::Failed(e.code().to_string() + " " + failure_word(&e))
                    }
                };
                set_state(app, m, s);
            }
            _ => {}
        }
    }
    refresh_status(app);
}

fn failure_word(e: &SwpError) -> &'static str {
    match e {
        SwpError::Timeout(_) => "timeout",
        SwpError::ParseFailed { .. } => "parse failed",
        _ => "failed",
    }
}

unsafe fn toggle_row(app: &mut App, i: i32) {
    if i >= 0 && (i as usize) < app.model.visible_len() {
        app.model.toggle(i as usize);
        SendMessageW(app.list, LVM_REDRAWITEMS, i as WPARAM, i as LPARAM);
        refresh_status(app);
    }
}

unsafe fn save_geometry(app: &mut App) {
    let mut wp: WINDOWPLACEMENT = std::mem::zeroed();
    wp.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
    if GetWindowPlacement(app.hwnd, &mut wp) == 0 {
        return;
    }
    let dpi = GetDpiForWindow(app.hwnd).max(96);
    let r = wp.rcNormalPosition;
    let prev_split = app.settings.window.map(|w| w.splitter).unwrap_or(180);
    let ws = sweepr_core::settings::WindowState {
        x: r.left,
        y: r.top,
        w: r.right - r.left,
        h: r.bottom - r.top,
        maximized: wp.showCmd == SW_SHOWMAXIMIZED as u32,
        splitter: prev_split,
    };
    app.settings.window = Some(to_saved(ws, dpi));
    let _ = app.settings.save(&app.data_dir);
}

unsafe fn on_notify(app: &mut App, lparam: LPARAM) -> LRESULT {
    let hdr = &*(lparam as *const NMHDR);
    if hdr.hwndFrom != app.list {
        return 0;
    }
    match hdr.code {
        LVN_GETDISPINFOW => {
            let di = &mut *(lparam as *mut NMLVDISPINFOW);
            let i = di.item.iItem;
            if i >= 0 && (i as usize) < app.model.visible_len() {
                if di.item.mask & LVIF_TEXT != 0 {
                    let col = COLS
                        .get(di.item.iSubItem as usize)
                        .map(|c| c.0)
                        .unwrap_or(Col::Name);
                    app.scratch = wide(&app.model.text(i as usize, col));
                    di.item.pszText = app.scratch.as_mut_ptr();
                }
                if di.item.mask & LVIF_STATE != 0 && di.item.stateMask & LVIS_STATEIMAGEMASK != 0 {
                    let idx: u32 = if app.model.is_checked(i as usize) {
                        2
                    } else {
                        1
                    };
                    di.item.state = (di.item.state & !LVIS_STATEIMAGEMASK) | (idx << 12);
                }
            }
            0
        }
        LVN_COLUMNCLICK => {
            let nm = &*(lparam as *const NMLISTVIEW);
            if let Some((c, _, _)) = COLS.get(nm.iSubItem as usize) {
                let cur = app.model.sort();
                let asc = if cur.col == *c { !cur.asc } else { true };
                app.model.set_sort(Sort { col: *c, asc });
                update_sort_markers(app);
                InvalidateRect(app.list, std::ptr::null(), 0);
            }
            0
        }
        NM_CLICK => {
            let nm = &*(lparam as *const NMITEMACTIVATE);
            let mut hti: LVHITTESTINFO = std::mem::zeroed();
            hti.pt = nm.ptAction;
            SendMessageW(app.list, LVM_HITTEST, 0, &mut hti as *mut _ as LPARAM);
            if hti.flags & LVHT_ONITEMSTATEICON != 0 {
                toggle_row(app, hti.iItem);
            }
            0
        }
        LVN_KEYDOWN => {
            let kd = &*(lparam as *const NMLVKEYDOWN);
            if kd.wVKey == VK_SPACE {
                let sel = SendMessageW(
                    app.list,
                    LVM_GETNEXTITEM,
                    usize::MAX,
                    LVNI_SELECTED as LPARAM,
                ) as i32;
                toggle_row(app, sel);
            } else if kd.wVKey == VK_F5 {
                start_discovery(app);
            }
            0
        }
        NM_CUSTOMDRAW => {
            let cd = &mut *(lparam as *mut NMLVCUSTOMDRAW);
            match cd.nmcd.dwDrawStage {
                CDDS_PREPAINT => CDRF_NOTIFYITEMDRAW as LRESULT,
                CDDS_ITEMPREPAINT => {
                    let p = palette(app.dark);
                    let i = cd.nmcd.dwItemSpec;
                    let st =
                        SendMessageW(app.list, LVM_GETITEMSTATE, i, LVIS_SELECTED as LPARAM) as u32;
                    if st & LVIS_SELECTED != 0 {
                        cd.clrTextBk = colorref(p.selection);
                        cd.clrText = colorref(0xFFFFFF);
                    } else {
                        cd.clrTextBk = colorref(p.bg1);
                        let dim =
                            i < app.model.visible_len() && app.model.visible(i).pinned_by_manager;
                        cd.clrText = colorref(if dim { p.text_dim } else { p.text });
                    }
                    CDRF_DODEFAULT as LRESULT
                }
                _ => CDRF_DODEFAULT as LRESULT,
            }
        }
        _ => 0,
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = &*(lparam as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
        if let Some(app) = app_of(hwnd) {
            app.hwnd = hwnd;
        }
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let Some(app) = app_of(hwnd) else {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    };
    match msg {
        WM_CREATE => {
            create_controls(app);
            start_discovery(app);
            0
        }
        WM_SIZE => {
            layout(app);
            0
        }
        WM_SETFOCUS => {
            SetFocus(app.list);
            0
        }
        WM_ERASEBKGND => {
            let mut r: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut r);
            FillRect(wparam as HDC, &r, app.bg0);
            1
        }
        WM_CTLCOLORSTATIC => {
            let p = palette(app.dark);
            SetTextColor(wparam as HDC, colorref(p.text_dim));
            SetBkColor(wparam as HDC, colorref(p.bg0));
            app.bg0 as LRESULT
        }
        WM_NOTIFY => on_notify(app, lparam),
        WM_APP_EVENT => {
            drain_events(app);
            0
        }
        WM_DPICHANGED => {
            app.dpi = (wparam & 0xFFFF) as u32;
            let r = &*(lparam as *const RECT);
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            rebuild_font(app);
            set_row_height(app);
            for (i, (_, _, w)) in COLS.iter().enumerate() {
                SendMessageW(
                    app.list,
                    LVM_SETCOLUMNWIDTH,
                    i,
                    to_device(*w, app.dpi) as LPARAM,
                );
            }
            layout(app);
            0
        }
        WM_SETTINGCHANGE => {
            if lparam != 0 && app.settings.theme == Theme::System {
                let name: Vec<u16> = wide("ImmersiveColorSet");
                let mut i = 0;
                let p = lparam as *const u16;
                while i < name.len() && *p.add(i) == name[i] {
                    i += 1;
                }
                if i == name.len() {
                    app.dark = is_dark(&app.settings);
                    apply_theme(app);
                }
            }
            0
        }
        WM_EXITSIZEMOVE => {
            save_geometry(app);
            0
        }
        WM_CLOSE => {
            app.cancel.cancel();
            save_geometry(app);
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
