//! Main window: pure Win32 + Common Controls v6. Windows only.
//! Runtime behavior is Unverified until run on a real machine (see AUDIT.md).

use super::actions::{
    build_jobs, disabled_reason, empty_action, empty_state, redact, uac_prompts_needed, Cmd,
    COMMANDS,
};
use super::dialogs::{palette_dialog, settings_dialog, text_dialog};
use super::geometry::{default_window, restore, to_device, to_saved};
use super::model::{Col, ListModel, MgrState, Sort};
use super::present::{
    details_text, failure_text, hints, inspector_width, next_region, progress_text, segments,
    summary, Region,
};
use super::theme::{
    colorref, palette, Palette, Severity, ACTIVE_RULE_PX, CONTROL_PX, DENSE_PX, GAP_PX, LABEL_PX,
    PANEL_HEADER_PX, ROW_HEIGHT_PX, SELECT_BAR_PX,
};
use super::tips::{column_tooltip, tooltip, Ctl};
use super::updater::{self, Ready};
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
use windows_sys::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows_sys::Win32::UI::Controls::Dialogs::{
    GetSaveFileNameW, OFN_OVERWRITEPROMPT, OPENFILENAMEW,
};
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetFocus, GetKeyState, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
    VK_CONTROL, VK_F1, VK_F5, VK_F6, VK_RETURN, VK_SPACE,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WM_APP_EVENT: u32 = WM_APP + 1;
const CLASS: &str = "SweeprMainWindow";
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const SS_LEFTNOWORDWRAP: u32 = 0x0000_000C;
const SS_CENTERIMAGE: u32 = 0x0000_0200;
const CF_UNICODETEXT: u32 = 13;
const SS_OWNERDRAW_STYLE: u32 = 0x0000_000D;

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
const ID_DETAILS: isize = 125;
const ID_BTN_DETAILS: isize = 126;
const ID_EMPTY_BTN: isize = 127;
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
const HINT_H: i32 = 28;
const STATUS_H: i32 = 40;
/// Log panel: header, a strip for the Copy/Save buttons, and the text area.
const LOG_STRIP_H: i32 = 40;
const LOG_H: i32 = PANEL_HEADER_PX + LOG_STRIP_H + 120;
const CHECK_PX: i32 = 14;
const CHECK_ZONE_PX: i32 = 28;
const RECENT_MAX: usize = 8;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

enum UiMsg {
    Disc(Event),
    Wire(Wire),
    Note(String),
    UpdatesDone,
    SelfUpdate {
        manual: bool,
        result: Result<Option<Ready>, String>,
    },
}

type Queue = Arc<Mutex<VecDeque<UiMsg>>>;

struct App {
    hwnd: HWND,
    list: HWND,
    header: HWND,
    status: HWND,
    empty: HWND,
    details: HWND,
    btn_details: HWND,
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
    font_label: HFONT,
    bg: HBRUSH,
    sf: HBRUSH,
    hc: bool,
    hover: bool,
    region: Region,
    hint_rects: Vec<(RECT, Cmd)>,
    rc_list_hdr: RECT,
    rc_det_hdr: RECT,
    rc_log_hdr: RECT,
    rc_hint: RECT,
    rc_prog: RECT,
    recent: Vec<Cmd>,
    sev: Severity,
    prog_done: usize,
    prog_total: usize,
    details_shown: String,
    batch: Vec<UpdatePackage>,
    pending_update: Option<Ready>,
    empty_btn: HWND,
    empty_cmd: Cmd,
    checking_self: bool,
    cancel: CancelToken,
    update_cancel: CancelToken,
    checking: bool,
    updating: bool,
    running: String,
    log_open: bool,
    log: Vec<String>,
    scratch: Vec<u16>,
}

const ZERO_RECT: RECT = RECT {
    left: 0,
    top: 0,
    right: 0,
    bottom: 0,
};

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

const ID_ACC_FIND: isize = 130;
const ID_ACC_SELALL: isize = 131;
const ID_ACC_HELP: isize = 132;
const ID_ACC_REGION: isize = 133;
const ID_ACC_HISTORY: isize = 134;
const WM_MOUSELEAVE_MSG: u32 = 0x02A3;

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
        k(V, VK_F1, ID_ACC_HELP),
        k(V, VK_F6, ID_ACC_REGION),
        k(V | CT, b'U' as u16, ID_BTN_UPDATE),
        k(V | CT | SH, b'U' as u16, ID_BTN_UPDATEALL),
        k(V | CT, b'L' as u16, ID_LOG_TOGGLE),
        k(V | CT, b'D' as u16, ID_BTN_DETAILS),
        k(V | CT, b'H' as u16, ID_ACC_HISTORY),
        k(V | CT, b'F' as u16, ID_ACC_FIND),
        k(V | CT, b'A' as u16, ID_ACC_SELALL),
        k(V | CT, b'S' as u16, ID_LOG_SAVE),
        k(V | CT, 0xBC, ID_BTN_SETTINGS), // VK_OEM_COMMA
        k(V | CT, b'I' as u16, ID_SHOW_IGN),
        k(V | CT, b'K' as u16, ID_BTN_PALETTE),
    ]
}

pub(super) fn high_contrast() -> bool {
    unsafe {
        let mut hc: HIGHCONTRASTW = std::mem::zeroed();
        hc.cbSize = std::mem::size_of::<HIGHCONTRASTW>() as u32;
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            hc.cbSize,
            &mut hc as *mut _ as *mut _,
            0,
        );
        hc.dwFlags & HCF_HIGHCONTRASTON != 0
    }
}

fn sys_rgb(i: SYS_COLOR_INDEX) -> u32 {
    let c = unsafe { GetSysColor(i) };
    ((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF)
}

/// The active palette: TesselGUI GitHub tokens, or system colors in Windows high contrast.
pub(super) fn palette_for(dark: bool, hc: bool) -> Palette {
    if !hc {
        return palette(dark);
    }
    let (win, text, face) = (
        sys_rgb(COLOR_WINDOW),
        sys_rgb(COLOR_WINDOWTEXT),
        sys_rgb(COLOR_BTNFACE),
    );
    let (hi, hit) = (sys_rgb(COLOR_HIGHLIGHT), sys_rgb(COLOR_HIGHLIGHTTEXT));
    Palette {
        background: win,
        surface: win,
        raised: face,
        selected: hi,
        fg: text,
        muted: text,
        edge: text,
        border: text,
        grid: win,
        action: hi,
        action_hover: hi,
        action_pressed: hi,
        action_fg: hit,
        selection: hi,
        focus: text,
        info: text,
        warning: text,
        success: text,
        danger: text,
    }
}

fn pal(app: &App) -> Palette {
    palette_for(app.dark, app.hc)
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
        // Resource id 1 is the embedded application icon (see sweepr.rc);
        // LoadIconW returns null when it is absent, which falls back cleanly.
        let icon = LoadIconW(hinst, std::ptr::without_provenance::<u16>(1)); // MAKEINTRESOURCE(1)
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: icon,
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
            hIconSm: icon,
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
            details: std::ptr::null_mut(),
            btn_details: std::ptr::null_mut(),
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
            font_label: std::ptr::null_mut(),
            bg: std::ptr::null_mut(),
            sf: std::ptr::null_mut(),
            hc: high_contrast(),
            hover: false,
            region: Region::List,
            hint_rects: Vec::new(),
            rc_list_hdr: ZERO_RECT,
            rc_det_hdr: ZERO_RECT,
            rc_log_hdr: ZERO_RECT,
            rc_hint: ZERO_RECT,
            rc_prog: ZERO_RECT,
            recent: Vec::new(),
            sev: Severity::Info,
            prog_done: 0,
            prog_total: 0,
            details_shown: String::new(),
            batch: Vec::new(),
            pending_update: None,
            empty_btn: std::ptr::null_mut(),
            empty_cmd: Cmd::Check,
            checking_self: false,
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
            if msg.message == WM_KEYDOWN
                && msg.wParam == b'A' as usize
                && GetKeyState(VK_CONTROL as i32) < 0
            {
                // Ctrl+A selects text inside an edit; elsewhere it selects all packages.
                let f = GetFocus();
                let mut cls = [0u16; 8];
                let n = GetClassNameW(f, cls.as_mut_ptr(), 8) as usize;
                if String::from_utf16_lossy(&cls[..n]).eq_ignore_ascii_case("edit") {
                    SendMessageW(f, EM_SETSEL, 0, -1);
                    continue;
                }
            }
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

unsafe fn make_font(dpi: u32, px: i32, weight: i32) -> HFONT {
    CreateFontW(
        -((px * dpi as i32 + 48) / 96),
        0,
        0,
        0,
        weight,
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

unsafe fn rebuild_font(app: &mut App) {
    for f in [app.font, app.font_label] {
        if !f.is_null() {
            DeleteObject(f);
        }
    }
    // TesselGUI Compact: dense 12 px for controls and rows, label 13 px for panel headers.
    app.font = make_font(app.dpi, DENSE_PX, FW_NORMAL as i32);
    app.font_label = make_font(app.dpi, LABEL_PX, FW_SEMIBOLD as i32);
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
    app.hc = high_contrast();
    let p: Palette = pal(app);
    for b in [app.bg, app.sf] {
        if !b.is_null() {
            DeleteObject(b);
        }
    }
    app.bg = CreateSolidBrush(colorref(p.background));
    app.sf = CreateSolidBrush(colorref(p.surface));
    let on: BOOL = app.dark as BOOL;
    DwmSetWindowAttribute(
        app.hwnd,
        DWMWA_USE_IMMERSIVE_DARK_MODE,
        &on as *const _ as *const _,
        4,
    );
    let theme = if app.hc {
        // Leave visual styles on the system's high-contrast scheme.
        ""
    } else if app.dark {
        "DarkMode_Explorer"
    } else {
        "Explorer"
    };
    for w in app.ctrls.iter().copied().chain([app.header]) {
        if app.hc {
            SetWindowTheme(w, std::ptr::null(), std::ptr::null());
        } else {
            SetWindowTheme(w, wide(theme).as_ptr(), std::ptr::null());
        }
    }
    SendMessageW(app.list, LVM_SETBKCOLOR, 0, colorref(p.surface) as LPARAM);
    SendMessageW(
        app.list,
        LVM_SETTEXTBKCOLOR,
        0,
        colorref(p.surface) as LPARAM,
    );
    SendMessageW(app.list, LVM_SETTEXTCOLOR, 0, colorref(p.fg) as LPARAM);
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
    let b_check = mk(app, "BUTTON", "Check", btn, 0, ID_BTN_CHECK);
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
    let b_pal = mk(app, "BUTTON", "Commands", btn, 0, ID_BTN_PALETTE);
    app.btn_details = mk(app, "BUTTON", "Details", btn, 0, ID_BTN_DETAILS);
    let b_det = app.btn_details;
    app.list = mk(
        app,
        "SysListView32",
        "",
        LVS_REPORT | LVS_OWNERDATA | LVS_SHOWSELALWAYS | LVS_SINGLESEL | WS_TABSTOP,
        0,
        ID_LIST,
    );
    app.empty = mk(app, "STATIC", "", SS_OWNERDRAW_STYLE, 0, ID_EMPTY);
    app.empty_btn = mk(
        app,
        "BUTTON",
        "Check again",
        BS_PUSHBUTTON as u32 | WS_TABSTOP,
        0,
        ID_EMPTY_BTN,
    );
    ShowWindow(app.empty_btn, SW_HIDE);
    app.details = mk(
        app,
        "EDIT",
        "",
        ES_MULTILINE as u32 | ES_READONLY as u32 | ES_AUTOVSCROLL as u32 | WS_VSCROLL | WS_TABSTOP,
        0,
        ID_DETAILS,
    );
    app.log_edit = mk(
        app,
        "EDIT",
        "",
        ES_MULTILINE as u32 | ES_READONLY as u32 | ES_AUTOVSCROLL as u32 | WS_VSCROLL | WS_TABSTOP,
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
        // Rows, gridlines and the checkbox are drawn by `draw_row` (TesselGUI edge-colour grid).
        (LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER | LVS_EX_LABELTIP) as LPARAM,
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
    // TesselGUI: tooltips appear after 500 ms.
    SendMessageW(app.tip, TTM_SETDELAYTIME, TTDT_INITIAL as WPARAM, 500);
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
        (b_det, Ctl::DetailsToggle),
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

    SetWindowSubclass(
        app.btn_update,
        Some(primary_proc),
        1,
        app as *mut App as usize,
    );
    rebuild_font(app);
    set_row_height(app);
    apply_theme(app);
    update_sort_markers(app);
    for h in [app.log_edit, b_copy, b_save] {
        ShowWindow(h, SW_HIDE);
    }
    SendMessageW(app.btn_details, BM_SETCHECK, 0, 0);
    layout(app);
    update_details(app);
}

/// Primary-button subclass: tracks hover so the owner-draw button can show `action_hover`.
unsafe extern "system" fn primary_proc(
    h: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    let app = &mut *(data as *mut App);
    match msg {
        WM_MOUSEMOVE if !app.hover => {
            app.hover = true;
            let mut t = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: h,
                dwHoverTime: 0,
            };
            TrackMouseEvent(&mut t);
            InvalidateRect(h, std::ptr::null(), 0);
        }
        WM_MOUSELEAVE_MSG => {
            app.hover = false;
            InvalidateRect(h, std::ptr::null(), 0);
        }
        _ => {}
    }
    DefSubclassProc(h, msg, w, l)
}

/// Panel layout at the current DPI (TesselGUI Compact: 32 px controls, 8 px gaps,
/// 28 px panel headers). Top to bottom: toolbar, list | details, log drawer, hint bar, status.
unsafe fn layout(app: &mut App) {
    let mut r: RECT = std::mem::zeroed();
    GetClientRect(app.hwnd, &mut r);
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    let dpi = app.dpi;
    let d = |v: i32| to_device(v, dpi);
    let (gap, ch) = (d(GAP_PX), d(CONTROL_PX));
    let hwnd = app.hwnd;
    let place = |id: isize, x: i32, y: i32, wd: i32| {
        MoveWindow(GetDlgItem(hwnd, id as i32), x, y, wd, ch, 1);
    };
    let mut x = gap;
    for (id, wd) in [
        (ID_BTN_CHECK, 72),
        (ID_BTN_UPDATE, 128),
        (ID_BTN_UPDATEALL, 100),
    ] {
        place(id, x, gap, d(wd));
        x += d(wd) + gap;
    }
    let mut rx = w - gap;
    for (id, wd) in [
        (ID_BTN_PALETTE, 96),
        (ID_BTN_SETTINGS, 80),
        (ID_BTN_DETAILS, 80),
        (ID_SHOW_IGN, 112),
        (ID_SRC_SCOOP, 68),
        (ID_SRC_CHOCO, 72),
        (ID_SRC_WINGET, 76),
    ] {
        rx -= d(wd);
        place(id, rx, gap, d(wd));
        rx -= gap;
    }
    place(ID_FILTER, x, gap, (rx - x).max(d(80)));
    // Center the text (and the placeholder) vertically in the 32 px box; the default formatting
    // rectangle hugs the top and clipped the placeholder in the first field capture.
    let mut fr: RECT = std::mem::zeroed();
    GetClientRect(app.filter, &mut fr);
    let line_h = d(DENSE_PX) * 4 / 3;
    let inset = ((fr.bottom - fr.top - line_h) / 2).max(0);
    fr.top += inset;
    fr.bottom -= inset;
    fr.left += d(6);
    fr.right -= d(6);
    SendMessageW(app.filter, EM_SETRECT, 0, &fr as *const _ as LPARAM);

    let toolbar_h = ch + 2 * gap;
    let (hint_h, status_h, ph) = (d(HINT_H), d(STATUS_H), d(PANEL_HEADER_PX));
    let log_h = if app.log_open { d(LOG_H) } else { 0 };
    let body_top = toolbar_h;
    let body_h = (h - body_top - hint_h - status_h - log_h).max(0);
    let det_w = if app.settings.details_open {
        inspector_width(w, dpi)
    } else {
        0
    };
    let list_w = w - det_w;
    let rc = |l: i32, t: i32, rr: i32, b: i32| RECT {
        left: l,
        top: t,
        right: rr,
        bottom: b,
    };
    app.rc_list_hdr = rc(0, body_top, list_w, body_top + ph);
    MoveWindow(app.list, 0, body_top + ph, list_w, (body_h - ph).max(0), 1);
    let mut hr: RECT = std::mem::zeroed();
    GetWindowRect(app.header, &mut hr);
    let hh = hr.bottom - hr.top;
    MoveWindow(
        app.empty,
        0,
        body_top + ph + hh,
        list_w,
        (body_h - ph - hh).max(0),
        1,
    );
    // Empty-state action sits under the hint that `draw_empty` paints (title, then hint).
    let (et, eh) = (body_top + ph + hh, (body_h - ph - hh).max(0));
    let bw = d(160);
    MoveWindow(
        app.empty_btn,
        (list_w - bw) / 2,
        et + eh / 3 + d(28) * 2 + gap,
        bw,
        ch,
        1,
    );
    if det_w > 0 {
        app.rc_det_hdr = rc(list_w, body_top, w, body_top + ph);
        MoveWindow(
            app.details,
            list_w,
            body_top + ph,
            det_w,
            (body_h - ph).max(0),
            1,
        );
        ShowWindow(app.details, SW_SHOW);
    } else {
        app.rc_det_hdr = ZERO_RECT;
        ShowWindow(app.details, SW_HIDE);
    }
    if app.log_open {
        let top = body_top + body_h;
        app.rc_log_hdr = rc(0, top, w, top + ph);
        let by = top + ph + (d(LOG_STRIP_H) - ch) / 2;
        MoveWindow(GetDlgItem(hwnd, ID_LOG_COPY as i32), gap, by, d(96), ch, 1);
        MoveWindow(
            GetDlgItem(hwnd, ID_LOG_SAVE as i32),
            gap * 2 + d(96),
            by,
            d(96),
            ch,
            1,
        );
        let ey = top + ph + d(LOG_STRIP_H);
        MoveWindow(
            app.log_edit,
            0,
            ey,
            w,
            (log_h - ph - d(LOG_STRIP_H)).max(0),
            1,
        );
    } else {
        app.rc_log_hdr = ZERO_RECT;
    }
    let hy = h - status_h - hint_h;
    app.rc_hint = rc(0, hy, w, hy + hint_h);
    let sy = h - status_h;
    let by = sy + (status_h - ch) / 2;
    let (cw, lw) = (d(112), d(104));
    MoveWindow(app.btn_cancel, w - cw - gap, by, cw, ch, 1);
    MoveWindow(
        GetDlgItem(hwnd, ID_LOG_TOGGLE as i32),
        w - cw - lw - 2 * gap,
        by,
        lw,
        ch,
        1,
    );
    let pw = d(120);
    let px = w - cw - lw - 3 * gap - pw;
    app.rc_prog = rc(
        px,
        sy + (status_h - d(10)) / 2,
        px + pw,
        sy + (status_h + d(10)) / 2,
    );
    MoveWindow(app.status, gap, sy, (px - 2 * gap).max(0), status_h, 1);
    InvalidateRect(hwnd, std::ptr::null(), 1);
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

/// Derives which region holds the keyboard focus, for the active rule and the hint bar.
unsafe fn update_region(app: &mut App) {
    let f = GetFocus();
    let r = if f == app.list || f == app.header {
        Region::List
    } else if f == app.details {
        Region::Details
    } else if f == app.log_edit
        || f == GetDlgItem(app.hwnd, ID_LOG_COPY as i32)
        || f == GetDlgItem(app.hwnd, ID_LOG_SAVE as i32)
    {
        Region::Log
    } else if f.is_null() {
        app.region
    } else {
        Region::Toolbar
    };
    if r != app.region {
        app.region = r;
        InvalidateRect(app.hwnd, std::ptr::null(), 0);
    }
}

unsafe fn selected_index(app: &App) -> i32 {
    SendMessageW(
        app.list,
        LVM_GETNEXTITEM,
        usize::MAX,
        LVNI_SELECTED as LPARAM,
    ) as i32
}

/// Fills the details panel from the highlighted row.
unsafe fn update_details(app: &mut App) {
    let i = selected_index(app);
    let pkg = (i >= 0 && (i as usize) < app.model.visible_len())
        .then(|| app.model.visible(i as usize).clone());
    let text = details_text(pkg.as_ref());
    if text != app.details_shown {
        SetWindowTextW(app.details, wide(&text).as_ptr());
        app.details_shown = text;
    }
}

fn busy(app: &App) -> bool {
    app.checking || app.updating
}

unsafe fn refresh_status(app: &mut App) {
    let sel = app.model.selected();
    let (sev, mut t) = summary(&app.states, app.model.total(), sel.len());
    let n = uac_prompts_needed(&sel, elevated_now());
    t.push_str(&format!(
        " | {n} UAC prompt{}",
        if n == 1 { "" } else { "s" }
    ));
    if app.checking {
        app.prog_total = app.states.len();
        app.prog_done = app
            .states
            .iter()
            .filter(|(_, s)| *s != MgrState::Checking)
            .count();
    }
    if busy(app) && app.prog_total > 0 {
        let unit = if app.updating { "packages" } else { "managers" };
        t.push_str(&format!(
            " | {}",
            progress_text(app.prog_done, app.prog_total, unit)
        ));
    }
    if !app.running.is_empty() {
        t.push_str(" | ");
        t.push_str(&app.running);
    }
    // Severity is never color alone: glyph and word come first.
    app.sev = sev;
    let line = format!("{} {}: {t}", sev.glyph(), sev.word());
    SetWindowTextW(app.status, wide(&line).as_ptr());
    EnableWindow(app.btn_cancel, busy(app) as BOOL);
    match empty_state(&app.states, app.model.total(), app.checking) {
        Some((title, hint)) => {
            SetWindowTextW(app.empty, wide(&format!("{title}\r\n{hint}")).as_ptr());
            let (label, cmd) = empty_action(title);
            app.empty_cmd = cmd;
            SetWindowTextW(app.empty_btn, wide(label).as_ptr());
            ShowWindow(app.empty, SW_SHOW);
            ShowWindow(app.empty_btn, SW_SHOW);
        }
        None => {
            ShowWindow(app.empty, SW_HIDE);
            ShowWindow(app.empty_btn, SW_HIDE);
        }
    }
    update_details(app);
    update_region(app);
    InvalidateRect(app.hwnd, &app.rc_prog, 1);
    InvalidateRect(app.hwnd, &app.rc_list_hdr, 0);
    InvalidateRect(app.hwnd, &app.rc_hint, 0);
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
    app.batch = sel.clone();
    app.update_cancel = CancelToken::new();
    app.running = String::new();
    app.prog_done = 0;
    app.prog_total = sel.len();
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
                            MgrState::Failed(failure_text(&e))
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
                    if state != ItemState::Started {
                        app.prog_done += 1;
                        record_history(app, &id, state == ItemState::Done, code);
                    }
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
            UiMsg::SelfUpdate { manual, result } => {
                app.checking_self = false;
                match result {
                    Ok(None) => {
                        if manual {
                            append_log(app, "[sweepr] Sweepr is up to date".into());
                            info_box(app, "Sweepr is up to date.");
                        }
                    }
                    Ok(Some(r)) => offer_install(app, r),
                    Err(e) => {
                        append_log(app, format!("[sweepr] update check failed: {e}"));
                        if manual {
                            info_box(app, &format!("Could not check for updates: {e}"));
                        }
                    }
                }
            }
            UiMsg::UpdatesDone => {
                app.updating = false;
                app.running.clear();
                app.prog_done = 0;
                app.prog_total = 0;
                append_log(app, "[sweepr] updates finished; checking again".into());
                start_discovery(app);
            }
        }
    }
    refresh_status(app);
}

/// Writes one history line for a finished item. Failure to write is logged, never fatal.
unsafe fn record_history(app: &mut App, id: &str, ok: bool, code: Option<i32>) {
    let Some(p) = app.batch.iter().find(|p| p.id == id) else {
        return;
    };
    let e = sweepr_core::history::Entry {
        ts: sweepr_core::history::now(),
        manager: p.manager,
        id: p.id.clone(),
        from: p.current_version.clone(),
        to: p.available_version.clone(),
        ok,
        code,
    };
    if let Err(err) = sweepr_core::history::append(&app.data_dir, &[e]) {
        append_log(app, format!("[sweepr] could not write history: {err}"));
    }
}

unsafe fn show_history(app: &App) {
    use sweepr_core::history::{line, load, undo_hint};
    let all = load(&app.data_dir);
    let mut t = String::new();
    if all.is_empty() {
        t.push_str(
            "No updates recorded yet.\nUpdates you run from Sweepr appear here, newest first.\n",
        );
    } else {
        t.push_str("Newest first. Undo lines are text to copy and run yourself; downgrades may not work for every package.\n\n");
    }
    for e in all.iter().take(200) {
        t.push_str(&line(e));
        t.push('\n');
        if let Some(h) = undo_hint(e) {
            t.push_str(&format!("    undo: {h}\n"));
        }
    }
    text_dialog(app.hwnd, app.dpi, app.dark, "Update history", &t);
}

fn exe_path() -> PathBuf {
    std::env::current_exe().unwrap_or_default()
}

fn is_portable() -> bool {
    exe_path()
        .parent()
        .is_some_and(|d| d.join("portable.flag").exists())
}

unsafe fn info_box(app: &App, text: &str) {
    MessageBoxW(
        app.hwnd,
        wide(text).as_ptr(),
        wide("Sweepr").as_ptr(),
        MB_OK | MB_ICONINFORMATION,
    );
}

/// Checks GitHub for a newer Sweepr on a worker thread. Only network use in Sweepr itself.
unsafe fn start_self_update_check(app: &mut App, manual: bool) {
    if let Some(r) = app.pending_update.clone() {
        offer_install(app, r);
        return;
    }
    if app.checking_self {
        return;
    }
    app.checking_self = true;
    app.settings.last_update_check = sweepr_core::history::now();
    save_settings(app);
    let (queue, hwnd, dir, portable) = (
        app.queue.clone(),
        app.hwnd as usize,
        app.data_dir.clone(),
        is_portable(),
    );
    std::thread::spawn(move || {
        let result = updater::check_and_fetch(&dir, portable);
        post(&queue, hwnd, UiMsg::SelfUpdate { manual, result });
    });
}

/// A verified download is ready: ask, then install and close, or keep it for later.
unsafe fn offer_install(app: &mut App, r: Ready) {
    let text = format!(
        "Sweepr {} is ready (checksum verified).\n\nInstall now? Sweepr will close and restart.",
        r.version
    );
    let ans = MessageBoxW(
        app.hwnd,
        wide(&text).as_ptr(),
        wide("Sweepr update").as_ptr(),
        MB_YESNO | MB_ICONQUESTION,
    );
    if ans != IDYES {
        append_log(
            app,
            format!(
                "[sweepr] version {} downloaded; use Check for Sweepr updates to install it",
                r.version
            ),
        );
        app.pending_update = Some(r);
        return;
    }
    match updater::install(&r, &exe_path()) {
        Ok(()) => {
            app.pending_update = None;
            SendMessageW(app.hwnd, WM_CLOSE, 0, 0);
        }
        Err(e) => {
            append_log(app, format!("[sweepr] update failed: {e}"));
            info_box(app, &format!("Update failed: {e}"));
        }
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
    // details_open is already kept current by `Cmd::ToggleDetails`.
    let _ = app.settings.save(&app.data_dir);
}

unsafe fn save_settings(app: &mut App) {
    if let Err(e) = app.settings.save(&app.data_dir) {
        append_log(app, format!("[sweepr] could not save settings: {e}"));
    }
}

fn cmd_label(c: Cmd) -> &'static str {
    COMMANDS
        .iter()
        .find(|(cc, _, _, _)| *cc == c)
        .map(|(_, l, _, _)| *l)
        .unwrap_or("Command")
}

fn palette_reasons(app: &App) -> Vec<(Cmd, &'static str)> {
    let (sel, total, b) = (app.model.selected().len(), app.model.total(), busy(app));
    COMMANDS
        .iter()
        .filter_map(|(c, _, _, _)| disabled_reason(*c, sel, total, b).map(|r| (*c, r)))
        .collect()
}

unsafe fn focus_region(app: &mut App, r: Region) {
    let target = match r {
        Region::Toolbar => app.filter,
        Region::List => app.list,
        Region::Details => app.details,
        Region::Log => app.log_edit,
    };
    SetFocus(target);
    update_region(app);
}

unsafe fn show_help(app: &App) {
    let mut t = String::from("Keyboard shortcuts\r\n\r\n");
    for (_, label, key, _) in COMMANDS {
        if !key.is_empty() {
            t.push_str(&format!("{key}\t{label}\r\n"));
        }
    }
    t.push_str("Space\tCheck the highlighted row\r\nCtrl+K\tOpen the command palette\r\n");
    MessageBoxW(
        app.hwnd,
        wide(&t).as_ptr(),
        wide("Sweepr help").as_ptr(),
        MB_OK | MB_ICONINFORMATION,
    );
}

unsafe fn reset_layout(app: &mut App) {
    let ws = default_window(app.dpi, &monitors());
    ShowWindow(app.hwnd, SW_RESTORE);
    SetWindowPos(
        app.hwnd,
        std::ptr::null_mut(),
        ws.x,
        ws.y,
        ws.w,
        ws.h,
        SWP_NOZORDER | SWP_NOACTIVATE,
    );
    for (i, (_, _, w)) in COLS.iter().enumerate() {
        SendMessageW(
            app.list,
            LVM_SETCOLUMNWIDTH,
            i,
            to_device(*w, app.dpi) as LPARAM,
        );
    }
    app.settings.details_open = true;
    set_log_open(app, false);
    save_geometry(app);
    append_log(app, "[sweepr] layout reset".into());
}

unsafe fn run_cmd(app: &mut App, cmd: Cmd) {
    if let Some(why) = disabled_reason(
        cmd,
        app.model.selected().len(),
        app.model.total(),
        busy(app),
    ) {
        append_log(
            app,
            format!("[sweepr] {} unavailable: {why}", cmd_label(cmd)),
        );
        return;
    }
    if !matches!(cmd, Cmd::ToggleRow | Cmd::Palette) {
        app.recent.retain(|c| *c != cmd);
        app.recent.insert(0, cmd);
        app.recent.truncate(RECENT_MAX);
    }
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
        Cmd::ToggleDetails => {
            app.settings.details_open = !app.settings.details_open;
            layout(app);
            save_settings(app);
        }
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
        Cmd::FocusFilter => {
            SetFocus(app.filter);
            SendMessageW(app.filter, EM_SETSEL, 0, -1);
            update_region(app);
        }
        Cmd::SaveLog => save_log(app),
        Cmd::History => show_history(app),
        Cmd::CheckUpdate => start_self_update_check(app, true),
        Cmd::ShowHelp => show_help(app),
        Cmd::ResetLayout => reset_layout(app),
        Cmd::NextRegion => {
            let r = next_region(app.region, app.settings.details_open, app.log_open);
            focus_region(app, r);
        }
        Cmd::Cancel => {
            app.cancel.cancel();
            app.update_cancel.cancel();
            append_log(app, "[sweepr] cancel requested".into());
        }
        Cmd::ToggleRow => {
            let i = selected_index(app);
            toggle_row(app, i);
        }
        Cmd::Palette => {
            let reasons = palette_reasons(app);
            let recent = app.recent.clone();
            if let Some(c) = palette_dialog(app.hwnd, app.dpi, app.dark, &recent, reasons) {
                run_cmd(app, c);
            }
        }
    }
}

/// Creates or removes the Task Scheduler entry when the setting changed.
unsafe fn apply_schedule(app: &mut App, n: &mut Settings) {
    use sweepr_core::schedule::{create_args, delete_args, run_schtasks};
    let (old_on, old_time) = (
        app.settings.scheduled_check,
        app.settings.schedule_time.clone(),
    );
    if n.scheduled_check == old_on && n.schedule_time == old_time {
        return;
    }
    let r = if n.scheduled_check {
        create_args(&exe_path().display().to_string(), &n.schedule_time)
            .and_then(|a| run_schtasks(&a))
    } else {
        run_schtasks(&delete_args())
    };
    match r {
        Ok(()) => append_log(
            app,
            if n.scheduled_check {
                format!("[sweepr] daily check scheduled at {}", n.schedule_time)
            } else {
                "[sweepr] daily check removed".into()
            },
        ),
        Err(e) => {
            append_log(app, format!("[sweepr] scheduled check not changed: {e}"));
            n.scheduled_check = old_on;
            n.schedule_time = old_time;
        }
    }
}

unsafe fn open_settings(app: &mut App) {
    if let Some(n) = settings_dialog(app.hwnd, &app.settings, app.dpi, app.dark, &app.data_dir) {
        let theme_changed = n.theme != app.settings.theme;
        let mut n = n;
        n.window = app.settings.window;
        n.log_drawer_open = app.settings.log_drawer_open;
        n.details_open = app.settings.details_open;
        n.last_update_check = app.settings.last_update_check;
        apply_schedule(app, &mut n);
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
            if i >= 0 && (i as usize) < app.model.visible_len() && di.item.mask & LVIF_TEXT != 0 {
                let col = COLS
                    .get(di.item.iSubItem as usize)
                    .map(|c| c.0)
                    .unwrap_or(Col::Name);
                app.scratch = wide(&app.model.text(i as usize, col));
                di.item.pszText = app.scratch.as_mut_ptr();
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
                // The checkbox is drawn by `draw_row`; a click in its zone toggles the row.
                if hti.iItem >= 0 {
                    let mut rc: RECT = std::mem::zeroed();
                    rc.left = LVIR_BOUNDS as i32;
                    SendMessageW(
                        app.list,
                        LVM_GETITEMRECT,
                        hti.iItem as usize,
                        &mut rc as *mut _ as LPARAM,
                    );
                    if nm.ptAction.x - rc.left < to_device(CHECK_ZONE_PX, app.dpi) {
                        toggle_row(app, hti.iItem);
                    }
                }
            } else {
                row_menu(app, hti.iItem);
            }
            0
        }
        LVN_KEYDOWN => {
            let kd = &*(lparam as *const NMLVKEYDOWN);
            if kd.wVKey == VK_SPACE || kd.wVKey == VK_RETURN {
                run_cmd(app, Cmd::ToggleRow);
            } else if kd.wVKey == VK_F5 {
                run_cmd(app, Cmd::Check);
            }
            0
        }
        LVN_ITEMCHANGED => {
            let nm = &*(lparam as *const NMLISTVIEW);
            if nm.uChanged & LVIF_STATE != 0 {
                update_details(app);
                InvalidateRect(app.hwnd, &app.rc_list_hdr, 0);
            }
            0
        }
        NM_SETFOCUS | NM_KILLFOCUS => {
            update_region(app);
            InvalidateRect(app.list, std::ptr::null(), 0);
            0
        }
        NM_CUSTOMDRAW => {
            let cd = &mut *(lparam as *mut NMLVCUSTOMDRAW);
            match cd.nmcd.dwDrawStage {
                CDDS_PREPAINT => CDRF_NOTIFYITEMDRAW as LRESULT,
                CDDS_ITEMPREPAINT => {
                    draw_row(app, cd.nmcd.hdc, cd.nmcd.dwItemSpec);
                    CDRF_SKIPDEFAULT as LRESULT
                }
                _ => CDRF_DODEFAULT as LRESULT,
            }
        }
        _ => 0,
    }
}

fn fill(hdc: HDC, r: &RECT, rgb: u32) {
    unsafe {
        let b = CreateSolidBrush(colorref(rgb));
        FillRect(hdc, r, b);
        DeleteObject(b);
    }
}

fn line(hdc: HDC, x0: i32, y0: i32, x1: i32, y1: i32, rgb: u32) {
    fill(
        hdc,
        &RECT {
            left: x0,
            top: y0,
            right: x1.max(x0 + 1),
            bottom: y1.max(y0 + 1),
        },
        rgb,
    );
}

unsafe fn text_in(hdc: HDC, font: HFONT, rgb: u32, r: &RECT, text: &str, flags: u32) {
    let old = SelectObject(hdc, font as HGDIOBJ);
    SetTextColor(hdc, colorref(rgb));
    SetBkMode(hdc, TRANSPARENT as i32);
    let mut rc = *r;
    DrawTextW(hdc, wide(text).as_ptr(), -1, &mut rc, flags);
    SelectObject(hdc, old);
}

const TXT: u32 = DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX;

/// One list row: surface or `selected` fill, 3 px selection bar, edge-colour row and column
/// gridlines, own checkbox, and a 2 px focus ring on the focused row of the focused list.
unsafe fn draw_row(app: &App, hdc: HDC, i: usize) {
    let p = pal(app);
    let d = |v: i32| to_device(v, app.dpi);
    if i >= app.model.visible_len() {
        return;
    }
    let mut rc: RECT = std::mem::zeroed();
    rc.left = LVIR_BOUNDS as i32;
    SendMessageW(app.list, LVM_GETITEMRECT, i, &mut rc as *mut _ as LPARAM);
    let mut cr: RECT = std::mem::zeroed();
    GetClientRect(app.list, &mut cr);
    rc.right = rc.right.max(cr.right);
    let st = SendMessageW(
        app.list,
        LVM_GETITEMSTATE,
        i,
        (LVIS_SELECTED | LVIS_FOCUSED) as LPARAM,
    ) as u32;
    let sel = st & LVIS_SELECTED != 0;
    fill(hdc, &rc, if sel { p.selected } else { p.surface });
    if sel {
        let mut bar = rc;
        bar.right = bar.left + d(SELECT_BAR_PX);
        fill(hdc, &bar, p.selection);
    }
    line(hdc, rc.left, rc.bottom - 1, rc.right, rc.bottom, p.edge);
    for (c, (col, _, _)) in COLS.iter().enumerate() {
        let mut hr: RECT = std::mem::zeroed();
        SendMessageW(app.header, HDM_GETITEMRECT, c, &mut hr as *mut _ as LPARAM);
        line(hdc, hr.right - 1, rc.top, hr.right, rc.bottom, p.edge);
        let mut cell = RECT {
            left: hr.left + d(8),
            top: rc.top,
            right: hr.right - d(8),
            bottom: rc.bottom - 1,
        };
        if c == 0 {
            let bx = hr.left + d(10);
            let by = rc.top + (rc.bottom - rc.top - d(CHECK_PX)) / 2;
            let bxr = RECT {
                left: bx,
                top: by,
                right: bx + d(CHECK_PX),
                bottom: by + d(CHECK_PX),
            };
            let on = app.model.is_checked(i);
            fill(hdc, &bxr, if on { p.action } else { p.border });
            if !on {
                let inner = RECT {
                    left: bxr.left + 1,
                    top: bxr.top + 1,
                    right: bxr.right - 1,
                    bottom: bxr.bottom - 1,
                };
                fill(hdc, &inner, if sel { p.selected } else { p.surface });
            } else {
                text_in(
                    hdc,
                    app.font,
                    p.action_fg,
                    &bxr,
                    "\u{2713}",
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );
            }
            cell.left = hr.left + d(CHECK_ZONE_PX);
        }
        let strong = matches!(col, Col::Name | Col::Available);
        text_in(
            hdc,
            app.font,
            if strong || sel { p.fg } else { p.muted },
            &cell,
            &app.model.text(i, *col),
            TXT,
        );
    }
    if st & LVIS_FOCUSED != 0 && GetFocus() == app.list {
        for k in 0..2 {
            let r = RECT {
                left: rc.left + k,
                top: rc.top + k,
                right: rc.right - k,
                bottom: rc.bottom - k,
            };
            let b = CreateSolidBrush(colorref(p.focus));
            FrameRect(hdc, &r, b);
            DeleteObject(b);
        }
    }
}

/// Panel headers, active-region rule, hint bar and ten-segment progress.
unsafe fn paint(app: &mut App, hdc: HDC) {
    let p = pal(app);
    let d = |v: i32| to_device(v, app.dpi);
    let gap = d(GAP_PX);
    let count = format!("Updates ({})", app.model.visible_len());
    let panels = [
        (app.rc_list_hdr, count.as_str(), Region::List),
        (app.rc_det_hdr, "Details", Region::Details),
        (app.rc_log_hdr, "Log", Region::Log),
    ];
    for (rc, title, region) in panels {
        if rc.right <= rc.left {
            continue;
        }
        fill(hdc, &rc, p.raised);
        line(hdc, rc.left, rc.bottom - 1, rc.right, rc.bottom, p.edge);
        line(hdc, rc.left, rc.top, rc.right, rc.top + 1, p.edge);
        if app.region == region {
            line(
                hdc,
                rc.left,
                rc.top,
                rc.right,
                rc.top + d(ACTIVE_RULE_PX),
                p.selection,
            );
        }
        let mut t = rc;
        t.left += gap;
        text_in(hdc, app.font_label, p.fg, &t, title, TXT);
    }
    if app.rc_det_hdr.right > app.rc_det_hdr.left {
        let x = app.rc_det_hdr.left;
        line(hdc, x, app.rc_det_hdr.top, x + 1, app.rc_hint.top, p.edge);
    }
    // Hint bar: up to six clickable hints for the focused region.
    let hr = app.rc_hint;
    fill(hdc, &hr, p.background);
    line(hdc, hr.left, hr.top, hr.right, hr.top + 1, p.edge);
    app.hint_rects.clear();
    let mut x = hr.left + gap;
    for h in hints(app.region, busy(app)) {
        let mut tw: RECT = std::mem::zeroed();
        let old = SelectObject(hdc, app.font_label as HGDIOBJ);
        let k = wide(h.key);
        DrawTextW(hdc, k.as_ptr(), -1, &mut tw, DT_CALCRECT | DT_SINGLELINE);
        let a = wide(h.action);
        let mut aw: RECT = std::mem::zeroed();
        DrawTextW(hdc, a.as_ptr(), -1, &mut aw, DT_CALCRECT | DT_SINGLELINE);
        SelectObject(hdc, old);
        let key_w = tw.right + d(12);
        let bh = d(20);
        let top = hr.top + (hr.bottom - hr.top - bh) / 2;
        let kb = RECT {
            left: x,
            top,
            right: x + key_w,
            bottom: top + bh,
        };
        let b = CreateSolidBrush(colorref(p.border));
        FrameRect(hdc, &kb, b);
        DeleteObject(b);
        text_in(
            hdc,
            app.font_label,
            p.fg,
            &kb,
            h.key,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        let ar = RECT {
            left: kb.right + d(6),
            top,
            right: kb.right + d(6) + aw.right + d(4),
            bottom: top + bh,
        };
        text_in(hdc, app.font_label, p.muted, &ar, h.action, TXT);
        app.hint_rects.push((
            RECT {
                left: kb.left,
                top: hr.top,
                right: ar.right,
                bottom: hr.bottom,
            },
            h.cmd,
        ));
        x = ar.right + gap * 2;
    }
    // Ten-segment determinate progress.
    if busy(app) && app.prog_total > 0 {
        let pr = app.rc_prog;
        let seg_w = (pr.right - pr.left - 9 * d(2)) / 10;
        let on = segments(app.prog_done, app.prog_total);
        for k in 0..10 {
            let l = pr.left + k * (seg_w + d(2));
            fill(
                hdc,
                &RECT {
                    left: l,
                    top: pr.top,
                    right: l + seg_w,
                    bottom: pr.bottom,
                },
                if (k as usize) < on { p.action } else { p.edge },
            );
        }
    }
}

/// Owner-drawn empty state: 24 px grid motif with the message centred on it.
unsafe fn draw_empty(app: &App, di: &DRAWITEMSTRUCT) {
    let p = pal(app);
    let d = |v: i32| to_device(v, app.dpi);
    let r = di.rcItem;
    fill(di.hDC, &r, p.surface);
    let step = d(24).max(1);
    let mut x = r.left;
    while x < r.right {
        line(di.hDC, x, r.top, x + 1, r.bottom, p.grid);
        x += step;
    }
    let mut y = r.top;
    while y < r.bottom {
        line(di.hDC, r.left, y, r.right, y + 1, p.grid);
        y += step;
    }
    let mut buf = [0u16; 512];
    let n = GetWindowTextW(di.hwndItem, buf.as_mut_ptr(), 512) as usize;
    let text = String::from_utf16_lossy(&buf[..n]);
    let (title, hint) = text.split_once("\r\n").unwrap_or((&text, ""));
    let mid = r.top + (r.bottom - r.top) / 3;
    let tr = RECT {
        left: r.left + d(16),
        top: mid,
        right: r.right - d(16),
        bottom: mid + d(28),
    };
    text_in(
        di.hDC,
        app.font_label,
        p.fg,
        &tr,
        title,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
    );
    let hr = RECT {
        top: tr.bottom,
        bottom: tr.bottom + d(28),
        ..tr
    };
    text_in(
        di.hDC,
        app.font,
        p.muted,
        &hr,
        hint,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
    );
}

unsafe fn draw_primary(app: &App, di: &DRAWITEMSTRUCT) {
    let p = pal(app);
    let pressed = di.itemState & ODS_SELECTED != 0;
    let bg = if pressed {
        p.action_pressed
    } else if app.hover {
        p.action_hover
    } else {
        p.action
    };
    fill(di.hDC, &di.rcItem, bg);
    let mut buf = [0u16; 64];
    let n = GetWindowTextW(di.hwndItem, buf.as_mut_ptr(), 64) as usize;
    text_in(
        di.hDC,
        app.font,
        p.action_fg,
        &di.rcItem,
        &String::from_utf16_lossy(&buf[..n]),
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
    );
    if di.itemState & ODS_FOCUS != 0 {
        for k in 0..2 {
            let r = RECT {
                left: di.rcItem.left + k,
                top: di.rcItem.top + k,
                right: di.rcItem.right - k,
                bottom: di.rcItem.bottom - k,
            };
            let b = CreateSolidBrush(colorref(p.focus));
            FrameRect(di.hDC, &r, b);
            DeleteObject(b);
        }
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
            app.hc = high_contrast();
            if app.settings.log_drawer_open {
                set_log_open(app, true);
            }
            if let Some(l) = sweepr_core::schedule::load_last(&app.data_dir) {
                append_log(
                    app,
                    format!(
                        "[sweepr] last scheduled check: {} update(s) at {} UTC",
                        l.count,
                        sweepr_core::history::utc(l.ts)
                    ),
                );
            }
            updater::cleanup(&app.data_dir, &exe_path());
            let due = sweepr_core::history::now().saturating_sub(app.settings.last_update_check)
                > 24 * 3600;
            if app.settings.auto_update && due {
                start_self_update_check(app, false);
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
            mm.ptMinTrackSize.x = to_device(1040, app.dpi);
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
            FillRect(wparam as HDC, &r, app.bg);
            1
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            paint(app, hdc);
            EndPaint(hwnd, &ps);
            0
        }
        WM_LBUTTONDOWN => {
            let (x, y) = (
                (lparam & 0xFFFF) as i16 as i32,
                ((lparam >> 16) & 0xFFFF) as i16 as i32,
            );
            let hit = app
                .hint_rects
                .iter()
                .find(|(r, _)| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
                .map(|(_, c)| *c);
            if let Some(c) = hit {
                run_cmd(app, c);
            }
            0
        }
        WM_SYSCOLORCHANGE => {
            apply_theme(app);
            0
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN | WM_CTLCOLOREDIT => {
            let p = pal(app);
            let h = lparam as HWND;
            let edit = msg == WM_CTLCOLOREDIT || h == app.details || h == app.log_edit;
            let fg = if h == app.status {
                app.sev.color(&p)
            } else {
                p.fg
            };
            SetTextColor(wparam as HDC, colorref(fg));
            SetBkColor(
                wparam as HDC,
                colorref(if edit { p.surface } else { p.background }),
            );
            if edit {
                app.sf as LRESULT
            } else {
                app.bg as LRESULT
            }
        }
        WM_DRAWITEM => {
            let di = &*(lparam as *const DRAWITEMSTRUCT);
            if di.CtlID as isize == ID_BTN_UPDATE {
                draw_primary(app, di);
                return 1;
            }
            if di.CtlID as isize == ID_EMPTY {
                draw_empty(app, di);
                return 1;
            }
            0
        }
        WM_NOTIFY => on_notify(app, lparam),
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as isize;
            let code = ((wparam >> 16) & 0xFFFF) as u32;
            if code == EN_SETFOCUS {
                update_region(app);
            }
            match id {
                ID_BTN_CHECK => run_cmd(app, Cmd::Check),
                ID_BTN_UPDATE => run_cmd(app, Cmd::UpdateSelected),
                ID_BTN_UPDATEALL => run_cmd(app, Cmd::UpdateAll),
                ID_BTN_SETTINGS => run_cmd(app, Cmd::Settings),
                ID_BTN_DETAILS => run_cmd(app, Cmd::ToggleDetails),
                ID_EMPTY_BTN => {
                    let c = app.empty_cmd;
                    run_cmd(app, c)
                }
                ID_LOG_TOGGLE => run_cmd(app, Cmd::ToggleLog),
                ID_ACC_FIND => run_cmd(app, Cmd::FocusFilter),
                ID_ACC_SELALL => run_cmd(app, Cmd::SelectAll),
                ID_ACC_HELP => run_cmd(app, Cmd::ShowHelp),
                ID_ACC_REGION => run_cmd(app, Cmd::NextRegion),
                ID_ACC_HISTORY => run_cmd(app, Cmd::History),
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
                ID_BTN_PALETTE => run_cmd(app, Cmd::Palette),
                ID_LOG_COPY => copy_to_clipboard(app, &log_text(app)),
                ID_LOG_SAVE if lparam == 0 => run_cmd(app, Cmd::SaveLog),
                ID_LOG_SAVE => save_log(app),
                ID_BTN_CANCEL | IDCANCEL_ID => run_cmd(app, Cmd::Cancel),
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
            apply_theme(app);
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
