//! Main window: pure Win32 + Common Controls v6. Windows only.
//! Runtime behavior is Unverified until run on a real machine (see AUDIT.md).

use super::actions::{build_jobs, empty_state, redact, uac_prompts_needed, Cmd};
use super::dialogs::{palette_dialog, settings_dialog};
use super::geometry::{default_window, restore, to_device, to_saved};
use super::model::{status_line, Col, ListModel, MgrState, Sort};
use super::theme::{colorref, palette, Palette, DENSE_PX, ROW_HEIGHT_PX};
use super::tips::{column_tooltip, tooltip, Ctl};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use sweepr_core::adapter::all_adapters;
use sweepr_core::elevation::{execute_job, win, ItemState, RealRunner, Wire};
use sweepr_core::errors::SwpError;
use sweepr_core::exec::CancelToken;
use sweepr_core::model::{Manager, UpdatePackage};
use sweepr_core::orchestrator::{discover, Event, Status};
use sweepr_core::settings::{Rect, Settings, Theme};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Controls::Dialogs::{
    GetSaveFileNameW, OFN_OVERWRITEPROMPT, OPENFILENAMEW,
};
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, SetFocus, VK_F5, VK_RETURN, VK_SPACE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WM_APP_EVENT: u32 = WM_APP + 1;
const CLASS: &str = "SweeprMainWindow";
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const SS_LEFTNOWORDWRAP: u32 = 0x0000_000C;
const SS_CENTERIMAGE: u32 = 0x0000_0200;
const SS_CENTER: u32 = 0x0000_0001;
const CF_UNICODETEXT: u32 = 13;

const ID_LIST: isize = 101;
const ID_STATUS: isize = 102;
const ID_EMPTY: isize = 103;
const ID_BTN_CHECK: isize = 110;
const ID_BTN_UPDATE: isize = 111;
const ID_BTN_UPDATEALL: isize = 112;
const ID_FILTER: isize = 113;
const ID_SRC_WINGET: isize = 114;
const ID_SRC_CHOCO: isize = 115;
const ID_SRC_SCOOP: isize = 116;
const ID_SHOW_IGN: isize = 117;
const ID_BTN_SETTINGS: isize = 118;
const ID_BTN_PALETTE: isize = 119;
const ID_LOG: isize = 120;
const ID_LOG_TOGGLE: isize = 121;
const ID_LOG_COPY: isize = 122;
const ID_LOG_SAVE: isize = 123;
const ID_BTN_CANCEL: isize = 124;
const IDCANCEL_ID: isize = 2;

const MANAGERS: [Manager; 3] = [Manager::Winget, Manager::Choco, Manager::Scoop];
const COLS: [(Col, &str, i32); 6] = [
    (Col::Name, "Name", 260),
    (Col::Id, "Id", 220),
    (Col::Current, "Current", 110),
    (Col::Available, "Available", 110),
    (Col::Source, "Source", 80),
    (Col::Scope, "Scope", 80),
];
const LOG_CAP: usize = 20_000;
const TOOLBAR_H: i32 = 36;
const STATUS_H: i32 = 28;
const LOG_H: i32 = 160;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

enum UiMsg {
    Disc(Event),
    Wire(Wire),
    Note(String),
    UpdatesDone,
}

type Queue = Arc<Mutex<VecDeque<UiMsg>>>;

struct App {
    hwnd: HWND,
    list: HWND,
    header: HWND,
    status: HWND,
    empty: HWND,
    filter: HWND,
    log_edit: HWND,
    btn_cancel: HWND,
    btn_update: HWND,
    show_ign: HWND,
    src: [HWND; 3],
    tip: HWND,
    ctrls: Vec<HWND>,
    tip_texts: Vec<Vec<u16>>,
    tip_scratch: Vec<u16>,
    settings: Settings,
    data_dir: PathBuf,
    model: ListModel,
    queue: Queue,
    states: Vec<(Manager, MgrState)>,
    dark: bool,
    dpi: u32,
    font: HFONT,
    bg0: HBRUSH,
    bg1: HBRUSH,
    cancel: CancelToken,
    update_cancel: CancelToken,
    checking: bool,
    updating: bool,
    running: String,
    log_open: bool,
    log: Vec<String>,
    scratch: Vec<u16>,
}

unsafe fn app_of(hwnd: HWND) -> Option<&'static mut App> {
    (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App).as_mut()
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

fn accelerators() -> Vec<ACCEL> {
    const V: u8 = 1; // FVIRTKEY
    const SH: u8 = 4;
    const CT: u8 = 8;
    let k = |fv: u8, key: u16, id: isize| ACCEL {
        fVirt: fv,
        key,
        cmd: id as u16,
    };
    vec![
        k(V, VK_F5, ID_BTN_CHECK),
        k(V | CT, b'U' as u16, ID_BTN_UPDATE),
        k(V | CT | SH, b'U' as u16, ID_BTN_UPDATEALL),
        k(V | CT, b'L' as u16, ID_LOG_TOGGLE),
        k(V | CT, 0xBC, ID_BTN_SETTINGS), // VK_OEM_COMMA
        k(V | CT, b'I' as u16, ID_SHOW_IGN),
        k(V | CT, b'K' as u16, ID_BTN_PALETTE),
    ]
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
            dwICC: ICC_LISTVIEW_CLASSES | ICC_BAR_CLASSES | ICC_WIN95_CLASSES,
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
        let mut log = Vec::new();
        if let Some(n) = notice {
            log.push(format!("[sweepr] {n}"));
        }
        let app = Box::new(App {
            hwnd: std::ptr::null_mut(),
            list: std::ptr::null_mut(),
            header: std::ptr::null_mut(),
            status: std::ptr::null_mut(),
            empty: std::ptr::null_mut(),
            filter: std::ptr::null_mut(),
            log_edit: std::ptr::null_mut(),
            btn_cancel: std::ptr::null_mut(),
            btn_update: std::ptr::null_mut(),
            show_ign: std::ptr::null_mut(),
            src: [std::ptr::null_mut(); 3],
            tip: std::ptr::null_mut(),
            ctrls: Vec::new(),
            tip_texts: Vec::new(),
            tip_scratch: Vec::new(),
            settings,
            data_dir,
            model: ListModel::default(),
            queue: Arc::new(Mutex::new(VecDeque::new())),
            states: Vec::new(),
            dark,
            dpi,
            font: std::ptr::null_mut(),
            bg0: std::ptr::null_mut(),
            bg1: std::ptr::null_mut(),
            cancel: CancelToken::new(),
            update_cancel: CancelToken::new(),
            checking: false,
            updating: false,
            running: String::new(),
            log_open: false,
            log,
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
        let acc = accelerators();
        let table = CreateAcceleratorTableW(acc.as_ptr(), acc.len() as i32);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if TranslateAcceleratorW(hwnd, table, &msg) != 0 {
                continue;
            }
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
    for w in app.ctrls.iter().copied().chain([app.header]) {
        SendMessageW(w, WM_SETFONT, app.font as WPARAM, 1);
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
    for b in [app.bg0, app.bg1] {
        if !b.is_null() {
            DeleteObject(b);
        }
    }
    app.bg0 = CreateSolidBrush(colorref(p.bg0));
    app.bg1 = CreateSolidBrush(colorref(p.bg1));
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
    for w in app.ctrls.iter().copied().chain([app.header]) {
        SetWindowTheme(w, wide(theme).as_ptr(), std::ptr::null());
    }
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

unsafe fn mk(app: &mut App, class: &str, text: &str, style: u32, ex: u32, id: isize) -> HWND {
    let h = CreateWindowExW(
        ex,
        wide(class).as_ptr(),
        wide(text).as_ptr(),
        WS_CHILD | WS_VISIBLE | style,
        0,
        0,
        10,
        10,
        app.hwnd,
        id as HMENU,
        GetModuleHandleW(std::ptr::null()),
        std::ptr::null(),
    );
    app.ctrls.push(h);
    h
}

unsafe fn add_tip(app: &mut App, h: HWND, text: &str) {
    app.tip_texts.push(wide(text));
    let ptr = app.tip_texts.last().unwrap().as_ptr() as *mut u16;
    let mut ti: TTTOOLINFOW = std::mem::zeroed();
    ti.cbSize = std::mem::size_of::<TTTOOLINFOW>() as u32;
    ti.uFlags = TTF_IDISHWND | TTF_SUBCLASS;
    ti.hwnd = app.hwnd;
    ti.uId = h as usize;
    ti.lpszText = ptr;
    SendMessageW(app.tip, TTM_ADDTOOLW, 0, &ti as *const _ as LPARAM);
}

unsafe fn create_controls(app: &mut App) {
    let hinst = GetModuleHandleW(std::ptr::null());
    let btn = BS_PUSHBUTTON as u32 | WS_TABSTOP;
    app.btn_update = mk(
        app,
        "BUTTON",
        "Update selected",
        BS_OWNERDRAW as u32 | WS_TABSTOP,
        0,
        ID_BTN_UPDATE,
    );
    let b_check = mk(app, "BUTTON", "Check  F5", btn, 0, ID_BTN_CHECK);
    let b_all = mk(app, "BUTTON", "Update all", btn, 0, ID_BTN_UPDATEALL);
    app.filter = mk(
        app,
        "EDIT",
        "",
        ES_AUTOHSCROLL as u32 | WS_BORDER | WS_TABSTOP,
        0,
        ID_FILTER,
    );
    let cue = wide("Filter by name or id");
    SendMessageW(app.filter, EM_SETCUEBANNER, 1, cue.as_ptr() as LPARAM);
    let ck = BS_AUTOCHECKBOX as u32 | WS_TABSTOP;
    app.src[0] = mk(app, "BUTTON", "WinGet", ck, 0, ID_SRC_WINGET);
    app.src[1] = mk(app, "BUTTON", "Choco", ck, 0, ID_SRC_CHOCO);
    app.src[2] = mk(app, "BUTTON", "Scoop", ck, 0, ID_SRC_SCOOP);
    let on = [
        app.settings.sources.winget,
        app.settings.sources.choco,
        app.settings.sources.scoop,
    ];
    for (h, v) in app.src.iter().zip(on) {
        SendMessageW(
            *h,
            BM_SETCHECK,
            if v { BST_CHECKED } else { BST_UNCHECKED } as WPARAM,
            0,
        );
    }
    app.show_ign = mk(app, "BUTTON", "Show ignored", ck, 0, ID_SHOW_IGN);
    let b_set = mk(app, "BUTTON", "Settings", btn, 0, ID_BTN_SETTINGS);
    let b_pal = mk(app, "BUTTON", "Commands  Ctrl+K", btn, 0, ID_BTN_PALETTE);
    app.list = mk(
        app,
        "SysListView32",
        "",
        LVS_REPORT | LVS_OWNERDATA | LVS_SHOWSELALWAYS | LVS_SINGLESEL | WS_TABSTOP,
        0,
        ID_LIST,
    );
    app.empty = mk(app, "STATIC", "", SS_CENTER, 0, ID_EMPTY);
    app.log_edit = mk(
        app,
        "EDIT",
        "",
        ES_MULTILINE as u32
            | ES_READONLY as u32
            | ES_AUTOVSCROLL as u32
            | WS_VSCROLL
            | WS_BORDER
            | WS_TABSTOP,
        0,
        ID_LOG,
    );
    SendMessageW(app.log_edit, EM_SETLIMITTEXT, 0x7FFF_FFF0, 0);
    let b_copy = mk(app, "BUTTON", "Copy log", btn, 0, ID_LOG_COPY);
    let b_save = mk(app, "BUTTON", "Save log", btn, 0, ID_LOG_SAVE);
    app.status = mk(
        app,
        "STATIC",
        "",
        SS_LEFTNOWORDWRAP | SS_CENTERIMAGE,
        0,
        ID_STATUS,
    );
    let b_log = mk(app, "BUTTON", "Log  Ctrl+L", btn, 0, ID_LOG_TOGGLE);
    app.btn_cancel = mk(app, "BUTTON", "Cancel  Esc", btn, 0, ID_BTN_CANCEL);
    EnableWindow(app.btn_cancel, 0);

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
    app.header = SendMessageW(app.list, LVM_GETHEADER, 0, 0) as HWND;
    insert_columns(app);

    app.tip = CreateWindowExW(
        WS_EX_TOPMOST,
        TOOLTIPS_CLASSW,
        std::ptr::null(),
        WS_POPUP | TTS_ALWAYSTIP,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        app.hwnd,
        std::ptr::null_mut(),
        hinst,
        std::ptr::null(),
    );
    SendMessageW(app.tip, TTM_SETMAXTIPWIDTH, 0, 420);
    let tips = [
        (b_check, Ctl::Check),
        (app.btn_update, Ctl::UpdateSelected),
        (b_all, Ctl::UpdateAll),
        (app.filter, Ctl::Filter),
        (app.src[0], Ctl::SrcWinget),
        (app.src[1], Ctl::SrcChoco),
        (app.src[2], Ctl::SrcScoop),
        (app.show_ign, Ctl::ShowIgnored),
        (b_set, Ctl::Settings),
        (b_pal, Ctl::Palette),
        (app.list, Ctl::List),
        (b_log, Ctl::LogToggle),
        (b_copy, Ctl::LogCopy),
        (b_save, Ctl::LogSave),
        (app.btn_cancel, Ctl::Cancel),
    ];
    for (h, c) in tips {
        add_tip(app, h, tooltip(c));
    }
    // Header tool uses a callback so each column shows its own text.
    let mut ti: TTTOOLINFOW = std::mem::zeroed();
    ti.cbSize = std::mem::size_of::<TTTOOLINFOW>() as u32;
    ti.uFlags = TTF_IDISHWND | TTF_SUBCLASS;
    ti.hwnd = app.hwnd;
    ti.uId = app.header as usize;
    ti.lpszText = usize::MAX as *mut u16; // LPSTR_TEXTCALLBACKW
    SendMessageW(app.tip, TTM_ADDTOOLW, 0, &ti as *const _ as LPARAM);

    rebuild_font(app);
    set_row_height(app);
    apply_theme(app);
    update_sort_markers(app);
    for h in [app.log_edit, b_copy, b_save] {
        ShowWindow(h, SW_HIDE);
    }
    layout(app);
}

/// Left-to-right toolbar packing at the current DPI.
unsafe fn layout(app: &App) {
    let mut r: RECT = std::mem::zeroed();
    GetClientRect(app.hwnd, &mut r);
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    let d = |v: i32| to_device(v, app.dpi);
    let by = d(5);
    let bh = d(26);
    let place = |id: isize, x: i32, wd: i32| {
        let c = GetDlgItem(app.hwnd, id as i32);
        MoveWindow(c, x, by, wd, bh, 1);
    };
    let mut x = d(8);
    for (id, wd) in [
        (ID_BTN_CHECK, 84),
        (ID_BTN_UPDATE, 130),
        (ID_BTN_UPDATEALL, 96),
    ] {
        place(id, x, d(wd));
        x += d(wd) + d(6);
    }
    let mut rx = w - d(8);
    for (id, wd) in [
        (ID_BTN_PALETTE, 130),
        (ID_BTN_SETTINGS, 74),
        (ID_SHOW_IGN, 104),
        (ID_SRC_SCOOP, 64),
        (ID_SRC_CHOCO, 68),
        (ID_SRC_WINGET, 72),
    ] {
        rx -= d(wd);
        place(id, rx, d(wd));
        rx -= d(4);
    }
    place(ID_FILTER, x + d(6), (rx - x - d(12)).max(d(80)));

    let status_h = d(STATUS_H);
    let log_h = if app.log_open { d(LOG_H) } else { 0 };
    let list_top = d(TOOLBAR_H);
    let list_h = (h - list_top - status_h - log_h).max(0);
    MoveWindow(app.list, 0, list_top, w, list_h, 1);
    MoveWindow(app.empty, w / 6, list_top + list_h / 3, w * 2 / 3, d(60), 1);
    if app.log_open {
        let top = list_top + list_h;
        let bar = d(30);
        MoveWindow(
            GetDlgItem(app.hwnd, ID_LOG_COPY as i32),
            d(8),
            top + d(2),
            d(90),
            d(26),
            1,
        );
        MoveWindow(
            GetDlgItem(app.hwnd, ID_LOG_SAVE as i32),
            d(104),
            top + d(2),
            d(90),
            d(26),
            1,
        );
        MoveWindow(app.log_edit, 0, top + bar, w, log_h - bar, 1);
    }
    let sy = h - status_h;
    let (cw, lw) = (d(104), d(104));
    MoveWindow(
        app.btn_cancel,
        w - cw - d(8),
        sy + d(1),
        cw,
        status_h - d(2),
        1,
    );
    MoveWindow(
        GetDlgItem(app.hwnd, ID_LOG_TOGGLE as i32),
        w - cw - lw - d(14),
        sy + d(1),
        lw,
        status_h - d(2),
        1,
    );
    MoveWindow(
        app.status,
        d(8),
        sy,
        (w - cw - lw - d(28)).max(0),
        status_h,
        1,
    );
}

unsafe fn set_log_open(app: &mut App, open: bool) {
    app.log_open = open;
    for id in [ID_LOG, ID_LOG_COPY, ID_LOG_SAVE] {
        ShowWindow(
            GetDlgItem(app.hwnd, id as i32),
            if open { SW_SHOW } else { SW_HIDE },
        );
    }
    layout(app);
    let mut s = String::new();
    for l in &app.log {
        s.push_str(l);
        s.push_str("\r\n");
    }
    SetWindowTextW(app.log_edit, wide(&s).as_ptr());
}

unsafe fn append_log(app: &mut App, line: String) {
    if app.log.len() >= LOG_CAP {
        app.log.drain(..1000);
    }
    if app.log_open {
        let t = wide(&format!("{line}\r\n"));
        SendMessageW(app.log_edit, EM_SETSEL, usize::MAX, -1);
        SendMessageW(app.log_edit, EM_REPLACESEL, 0, t.as_ptr() as LPARAM);
    }
    app.log.push(line);
}

fn elevated_now() -> bool {
    win::is_elevated()
}

unsafe fn refresh_status(app: &mut App) {
    let sel = app.model.selected();
    let mut t = status_line(&app.states, app.model.total(), sel.len());
    let n = uac_prompts_needed(&sel, elevated_now());
    t.push_str(&format!(
        " | {n} UAC prompt{}",
        if n == 1 { "" } else { "s" }
    ));
    if !app.running.is_empty() {
        t.push_str(" | ");
        t.push_str(&app.running);
    }
    SetWindowTextW(app.status, wide(&t).as_ptr());
    let busy = app.checking || app.updating;
    EnableWindow(app.btn_cancel, busy as BOOL);
    match empty_state(&app.states, app.model.total(), app.checking) {
        Some((title, hint)) => {
            SetWindowTextW(app.empty, wide(&format!("{title}\r\n{hint}")).as_ptr());
            ShowWindow(app.empty, SW_SHOW);
        }
        None => {
            ShowWindow(app.empty, SW_HIDE);
        }
    }
}

fn set_state(app: &mut App, m: Manager, s: MgrState) {
    match app.states.iter_mut().find(|(mm, _)| *mm == m) {
        Some(e) => e.1 = s,
        None => app.states.push((m, s)),
    }
}

fn enabled(s: &Settings, m: Manager) -> bool {
    match m {
        Manager::Winget => s.sources.winget,
        Manager::Choco => s.sources.choco,
        Manager::Scoop => s.sources.scoop,
    }
}

fn post(queue: &Queue, hwnd: usize, m: UiMsg) {
    queue.lock().unwrap().push_back(m);
    unsafe { PostMessageW(hwnd as HWND, WM_APP_EVENT, 0, 0) };
}

/// Starts one discovery run. A forwarder thread queues events and wakes the window.
unsafe fn start_discovery(app: &mut App) {
    app.cancel.cancel();
    app.cancel = CancelToken::new();
    app.states.clear();
    for m in MANAGERS {
        if enabled(&app.settings, m) {
            set_state(app, m, MgrState::Checking);
        }
    }
    app.checking = true;
    let (tx, rx) = std::sync::mpsc::channel();
    discover(all_adapters(), &app.settings, app.cancel.clone(), tx);
    let queue = app.queue.clone();
    let hwnd = app.hwnd as usize;
    std::thread::spawn(move || {
        for ev in rx {
            let done = matches!(ev, Event::Done);
            post(&queue, hwnd, UiMsg::Disc(ev));
            if done {
                break;
            }
        }
    });
    refresh_status(app);
}

unsafe fn start_update(app: &mut App, sel: Vec<UpdatePackage>) {
    if app.updating {
        return;
    }
    if sel.is_empty() {
        append_log(app, "[sweepr] nothing to update".into());
        return;
    }
    let elevated = elevated_now();
    let (user, sys) = build_jobs(&sel, elevated);
    let n = usize::from(user.is_some()) + usize::from(sys.is_some());
    if n == 0 {
        return;
    }
    app.updating = true;
    app.update_cancel = CancelToken::new();
    app.running = format!("updating {} package(s)", sel.len());
    append_log(
        app,
        format!(
            "[sweepr] updating {} package(s), UAC prompts: {}",
            sel.len(),
            uac_prompts_needed(&sel, elevated)
        ),
    );
    let pending = Arc::new(AtomicUsize::new(n));
    let hwnd = app.hwnd as usize;
    for (job, is_sys) in [(user, false), (sys, true)] {
        let Some(job) = job else { continue };
        let (queue, pending, cancel, dir) = (
            app.queue.clone(),
            pending.clone(),
            app.update_cancel.clone(),
            app.data_dir.clone(),
        );
        std::thread::spawn(move || {
            if !is_sys || elevated {
                execute_job(&job, &all_adapters(), &mut RealRunner, &cancel, &mut |w| {
                    post(&queue, hwnd, UiMsg::Wire(w))
                });
            } else {
                let r = win::run_elevated(&dir, &job, &cancel, &mut |w| {
                    post(&queue, hwnd, UiMsg::Wire(w.clone()))
                });
                if let Err(e) = r {
                    post(&queue, hwnd, UiMsg::Note(format!("[sweepr] {e}")));
                }
            }
            if pending.fetch_sub(1, Ordering::SeqCst) == 1 {
                post(&queue, hwnd, UiMsg::UpdatesDone);
            }
        });
    }
    refresh_status(app);
}

fn mgr_name(m: Manager) -> &'static str {
    m.as_str()
}

unsafe fn drain_events(app: &mut App) {
    let msgs: Vec<UiMsg> = app.queue.lock().unwrap().drain(..).collect();
    for m in msgs {
        match m {
            UiMsg::Disc(ev) => match ev {
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
                            append_log(app, format!("[{}] {e}", mgr_name(m)));
                            MgrState::Failed(format!("{} {}", e.code(), failure_word(&e)))
                        }
                    };
                    set_state(app, m, s);
                }
                Event::Log(m, st, line) => {
                    let tag = if st == sweepr_core::exec::Stream::Stderr {
                        "err"
                    } else {
                        "out"
                    };
                    append_log(app, format!("[{} {tag}] {line}", mgr_name(m)));
                }
                Event::Done => app.checking = false,
                _ => {}
            },
            UiMsg::Wire(w) => match w {
                Wire::Log {
                    manager,
                    stderr,
                    line,
                } => {
                    append_log(
                        app,
                        format!(
                            "[{} {}] {line}",
                            mgr_name(manager),
                            if stderr { "err" } else { "out" }
                        ),
                    );
                }
                Wire::Item { id, state, code } => {
                    let s = match state {
                        ItemState::Started => "started",
                        ItemState::Done => "done",
                        ItemState::Failed => "FAILED",
                    };
                    app.running = format!("{id}: {s}");
                    append_log(
                        app,
                        format!(
                            "[sweepr] {id}: {s}{}",
                            code.map(|c| format!(" (exit code {c})"))
                                .unwrap_or_default()
                        ),
                    );
                }
                Wire::End { code } => {
                    append_log(app, format!("[sweepr] batch ended with code {code}"))
                }
            },
            UiMsg::Note(s) => append_log(app, s),
            UiMsg::UpdatesDone => {
                app.updating = false;
                app.running.clear();
                append_log(app, "[sweepr] updates finished; checking again".into());
                start_discovery(app);
            }
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

unsafe fn copy_to_clipboard(app: &App, text: &str) {
    if OpenClipboard(app.hwnd) == 0 {
        return;
    }
    EmptyClipboard();
    let w = wide(text);
    let bytes = w.len() * 2;
    let h = GlobalAlloc(GMEM_MOVEABLE, bytes);
    if !h.is_null() {
        let p = GlobalLock(h) as *mut u16;
        if !p.is_null() {
            std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
            GlobalUnlock(h);
            SetClipboardData(CF_UNICODETEXT, h as HANDLE);
        }
    }
    CloseClipboard();
}

fn log_text(app: &App) -> String {
    app.log.join("\r\n")
}

unsafe fn save_log(app: &mut App) {
    let mut file = [0u16; 260];
    for (i, c) in "sweepr-log.txt".encode_utf16().enumerate() {
        file[i] = c;
    }
    let filter = wide("Text files\0*.txt\0All files\0*.*\0");
    let mut ofn: OPENFILENAMEW = std::mem::zeroed();
    ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    ofn.hwndOwner = app.hwnd;
    ofn.lpstrFilter = filter.as_ptr();
    ofn.lpstrFile = file.as_mut_ptr();
    ofn.nMaxFile = 260;
    ofn.Flags = OFN_OVERWRITEPROMPT;
    if GetSaveFileNameW(&mut ofn) == 0 {
        return;
    }
    let len = file.iter().position(|c| *c == 0).unwrap_or(0);
    let path = String::from_utf16_lossy(&file[..len]);
    let profile = std::env::var("USERPROFILE").unwrap_or_default();
    match std::fs::write(&path, redact(&log_text(app), &profile)) {
        Ok(()) => append_log(
            app,
            format!("[sweepr] log saved to {path} (profile path redacted)"),
        ),
        Err(e) => append_log(app, format!("[sweepr] could not save log: {e}")),
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
    app.settings.log_drawer_open = app.log_open;
    let _ = app.settings.save(&app.data_dir);
}

unsafe fn save_settings(app: &mut App) {
    if let Err(e) = app.settings.save(&app.data_dir) {
        append_log(app, format!("[sweepr] could not save settings: {e}"));
    }
}

unsafe fn run_cmd(app: &mut App, cmd: Cmd) {
    match cmd {
        Cmd::Check => start_discovery(app),
        Cmd::UpdateSelected => {
            let s = app.model.selected();
            start_update(app, s);
        }
        Cmd::UpdateAll => {
            let s = app.model.all_active();
            start_update(app, s);
        }
        Cmd::ToggleLog => set_log_open(app, !app.log_open),
        Cmd::Settings => open_settings(app),
        Cmd::ShowIgnored => {
            let on = SendMessageW(app.show_ign, BM_GETCHECK, 0, 0) != BST_CHECKED as isize;
            SendMessageW(
                app.show_ign,
                BM_SETCHECK,
                if on { BST_CHECKED } else { BST_UNCHECKED } as WPARAM,
                0,
            );
            app.model.set_show_ignored(on);
            SendMessageW(
                app.list,
                LVM_SETITEMCOUNT,
                app.model.visible_len(),
                LVSICF_NOSCROLL as LPARAM,
            );
            InvalidateRect(app.list, std::ptr::null(), 0);
        }
        Cmd::SelectAll | Cmd::SelectNone => {
            app.model.select_all(cmd == Cmd::SelectAll);
            InvalidateRect(app.list, std::ptr::null(), 0);
            refresh_status(app);
        }
    }
}

unsafe fn open_settings(app: &mut App) {
    if let Some(n) = settings_dialog(app.hwnd, &app.settings, app.dpi, app.dark, &app.data_dir) {
        let theme_changed = n.theme != app.settings.theme;
        let mut n = n;
        n.window = app.settings.window;
        n.log_drawer_open = app.settings.log_drawer_open;
        app.settings = n;
        let on = [
            app.settings.sources.winget,
            app.settings.sources.choco,
            app.settings.sources.scoop,
        ];
        for (h, v) in app.src.iter().zip(on) {
            SendMessageW(
                *h,
                BM_SETCHECK,
                if v { BST_CHECKED } else { BST_UNCHECKED } as WPARAM,
                0,
            );
        }
        save_settings(app);
        if theme_changed {
            app.dark = is_dark(&app.settings);
            apply_theme(app);
        }
        append_log(
            app,
            "[sweepr] settings saved; they apply on the next check".into(),
        );
    }
}

unsafe fn row_menu(app: &mut App, item: i32) {
    if item < 0 || (item as usize) >= app.model.visible_len() {
        return;
    }
    let p = app.model.visible(item as usize).clone();
    let ignored = app.settings.ignore.iter().any(|i| i == &p.id);
    let pinned = app.settings.pins.contains_key(&p.id);
    let menu = CreatePopupMenu();
    let items: [(usize, String); 4] = [
        (
            1,
            if ignored {
                "Stop ignoring".into()
            } else {
                "Ignore".into()
            },
        ),
        (
            2,
            if pinned {
                "Remove pin".into()
            } else {
                format!("Pin: skip version {}", p.available_version)
            },
        ),
        (3, "Update only this".into()),
        (4, "Copy id".into()),
    ];
    for (id, text) in &items {
        AppendMenuW(menu, MF_STRING, *id, wide(text).as_ptr());
    }
    let mut pt: POINT = std::mem::zeroed();
    GetCursorPos(&mut pt);
    let cmd = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON,
        pt.x,
        pt.y,
        0,
        app.hwnd,
        std::ptr::null(),
    );
    DestroyMenu(menu);
    match cmd {
        1 => {
            if ignored {
                app.settings.ignore.retain(|i| i != &p.id);
            } else {
                app.settings.ignore.push(p.id.clone());
            }
            apply_rules_now(app);
        }
        2 => {
            if pinned {
                app.settings.pins.remove(&p.id);
            } else {
                app.settings
                    .pins
                    .insert(p.id.clone(), p.available_version.clone());
            }
            apply_rules_now(app);
        }
        3 => start_update(app, vec![p]),
        4 => copy_to_clipboard(app, &p.id),
        _ => {}
    }
}

unsafe fn apply_rules_now(app: &mut App) {
    save_settings(app);
    app.model.apply_settings(&app.settings);
    SendMessageW(
        app.list,
        LVM_SETITEMCOUNT,
        app.model.visible_len(),
        LVSICF_NOSCROLL as LPARAM,
    );
    InvalidateRect(app.list, std::ptr::null(), 0);
    refresh_status(app);
}

unsafe fn on_notify(app: &mut App, lparam: LPARAM) -> LRESULT {
    let hdr = &*(lparam as *const NMHDR);
    if hdr.hwndFrom == app.tip && hdr.code == TTN_GETDISPINFOW {
        let di = &mut *(lparam as *mut NMTTDISPINFOW);
        if hdr.idFrom == app.header as usize {
            let mut pt: POINT = std::mem::zeroed();
            GetCursorPos(&mut pt);
            ScreenToClient(app.header, &mut pt);
            let mut hti: HDHITTESTINFO = std::mem::zeroed();
            hti.pt = pt;
            SendMessageW(app.header, HDM_HITTEST, 0, &mut hti as *mut _ as LPARAM);
            if let Some((c, _, _)) = COLS.get(hti.iItem.max(0) as usize) {
                app.tip_scratch = wide(column_tooltip(*c));
                di.lpszText = app.tip_scratch.as_mut_ptr();
            }
        }
        return 0;
    }
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
        NM_CLICK | NM_RCLICK => {
            let nm = &*(lparam as *const NMITEMACTIVATE);
            let mut hti: LVHITTESTINFO = std::mem::zeroed();
            hti.pt = nm.ptAction;
            SendMessageW(app.list, LVM_HITTEST, 0, &mut hti as *mut _ as LPARAM);
            if hdr.code == NM_CLICK {
                if hti.flags & LVHT_ONITEMSTATEICON != 0 {
                    toggle_row(app, hti.iItem);
                }
            } else {
                row_menu(app, hti.iItem);
            }
            0
        }
        LVN_KEYDOWN => {
            let kd = &*(lparam as *const NMLVKEYDOWN);
            if kd.wVKey == VK_SPACE || kd.wVKey == VK_RETURN {
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
                        cd.clrText = colorref(p.text);
                    }
                    CDRF_DODEFAULT as LRESULT
                }
                _ => CDRF_DODEFAULT as LRESULT,
            }
        }
        _ => 0,
    }
}

unsafe fn draw_primary(app: &App, di: &DRAWITEMSTRUCT) {
    let p = palette(app.dark);
    let pressed = di.itemState & ODS_SELECTED != 0;
    let brush = CreateSolidBrush(colorref(if pressed { p.selection } else { p.primary }));
    FillRect(di.hDC, &di.rcItem, brush);
    DeleteObject(brush);
    SetTextColor(di.hDC, colorref(0xFFFFFF));
    SetBkMode(di.hDC, TRANSPARENT as i32);
    let old = SelectObject(di.hDC, app.font as HGDIOBJ);
    let mut buf = [0u16; 64];
    let n = GetWindowTextW(di.hwndItem, buf.as_mut_ptr(), 64);
    let mut r = di.rcItem;
    DrawTextW(
        di.hDC,
        buf.as_ptr(),
        n,
        &mut r,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
    SelectObject(di.hDC, old);
    if di.itemState & ODS_FOCUS != 0 {
        let mut f = di.rcItem;
        InflateRect(&mut f, -3, -3);
        DrawFocusRect(di.hDC, &f);
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
            if app.settings.log_drawer_open {
                set_log_open(app, true);
            }
            start_discovery(app);
            0
        }
        WM_SIZE => {
            layout(app);
            0
        }
        WM_GETMINMAXINFO => {
            let mm = &mut *(lparam as *mut MINMAXINFO);
            mm.ptMinTrackSize.x = to_device(900, app.dpi);
            mm.ptMinTrackSize.y = to_device(480, app.dpi);
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
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN | WM_CTLCOLOREDIT => {
            let p = palette(app.dark);
            let on_list = lparam as HWND == app.empty;
            let edit = msg == WM_CTLCOLOREDIT;
            SetTextColor(
                wparam as HDC,
                colorref(if on_list { p.text_dim } else { p.text }),
            );
            let bg = if on_list || edit { p.bg1 } else { p.bg0 };
            SetBkColor(wparam as HDC, colorref(bg));
            if on_list || edit {
                app.bg1 as LRESULT
            } else {
                app.bg0 as LRESULT
            }
        }
        WM_DRAWITEM => {
            let di = &*(lparam as *const DRAWITEMSTRUCT);
            if di.CtlID as isize == ID_BTN_UPDATE {
                draw_primary(app, di);
                return 1;
            }
            0
        }
        WM_NOTIFY => on_notify(app, lparam),
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as isize;
            let code = ((wparam >> 16) & 0xFFFF) as u32;
            match id {
                ID_BTN_CHECK => run_cmd(app, Cmd::Check),
                ID_BTN_UPDATE => run_cmd(app, Cmd::UpdateSelected),
                ID_BTN_UPDATEALL => run_cmd(app, Cmd::UpdateAll),
                ID_BTN_SETTINGS => run_cmd(app, Cmd::Settings),
                ID_LOG_TOGGLE => run_cmd(app, Cmd::ToggleLog),
                // code 1 + null lparam = accelerator; a checkbox click sends BN_CLICKED (0) with a handle.
                ID_SHOW_IGN if lparam == 0 => run_cmd(app, Cmd::ShowIgnored),
                ID_SHOW_IGN => {
                    let on = SendMessageW(app.show_ign, BM_GETCHECK, 0, 0) == BST_CHECKED as isize;
                    app.model.set_show_ignored(on);
                    SendMessageW(
                        app.list,
                        LVM_SETITEMCOUNT,
                        app.model.visible_len(),
                        LVSICF_NOSCROLL as LPARAM,
                    );
                    InvalidateRect(app.list, std::ptr::null(), 0);
                }
                ID_BTN_PALETTE => {
                    if let Some(c) = palette_dialog(hwnd, app.dpi, app.dark) {
                        run_cmd(app, c);
                    }
                }
                ID_LOG_COPY => copy_to_clipboard(app, &log_text(app)),
                ID_LOG_SAVE => save_log(app),
                ID_BTN_CANCEL | IDCANCEL_ID => {
                    app.cancel.cancel();
                    app.update_cancel.cancel();
                    append_log(app, "[sweepr] cancel requested".into());
                }
                ID_FILTER if code == EN_CHANGE => {
                    let mut buf = [0u16; 256];
                    let n = GetWindowTextW(app.filter, buf.as_mut_ptr(), 256) as usize;
                    app.model.set_filter(&String::from_utf16_lossy(&buf[..n]));
                    SendMessageW(
                        app.list,
                        LVM_SETITEMCOUNT,
                        app.model.visible_len(),
                        LVSICF_NOSCROLL as LPARAM,
                    );
                    InvalidateRect(app.list, std::ptr::null(), 0);
                }
                ID_SRC_WINGET | ID_SRC_CHOCO | ID_SRC_SCOOP => {
                    let on = |h: HWND| SendMessageW(h, BM_GETCHECK, 0, 0) == BST_CHECKED as isize;
                    app.settings.sources.winget = on(app.src[0]);
                    app.settings.sources.choco = on(app.src[1]);
                    app.settings.sources.scoop = on(app.src[2]);
                    save_settings(app);
                }
                _ => {}
            }
            0
        }
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
            app.update_cancel.cancel();
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
