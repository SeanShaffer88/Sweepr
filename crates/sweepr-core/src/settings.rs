use crate::errors::SwpError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const SETTINGS_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub maximized: bool,
    pub splitter: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sources {
    pub winget: bool,
    pub choco: bool,
    pub scoop: bool,
}

impl Default for Sources {
    fn default() -> Self {
        Sources {
            winget: true,
            choco: true,
            scoop: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    /// Package ids excluded from "Update all".
    pub ignore: Vec<String>,
    /// id -> available version the user chose to skip.
    pub pins: BTreeMap<String, String>,
    pub sources: Sources,
    pub timeout_secs: u64,
    pub theme: Theme,
    pub window: Option<WindowState>,
    pub log_drawer_open: bool,
    /// Set when the file came from a newer Sweepr; `save` then does nothing.
    #[serde(skip)]
    pub read_only: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: SETTINGS_VERSION,
            ignore: vec![],
            pins: BTreeMap::new(),
            sources: Sources::default(),
            timeout_secs: 60,
            theme: Theme::System,
            window: None,
            log_drawer_open: false,
            read_only: false,
        }
    }
}

pub struct LoadResult {
    pub settings: Settings,
    pub notice: Option<SwpError>,
}

impl Settings {
    pub fn load(dir: &Path) -> LoadResult {
        let path = dir.join("settings.json");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return LoadResult {
                    settings: Settings::default(),
                    notice: None,
                }
            }
            Err(e) => return corrupt(&path, format!("unreadable: {e}")),
        };
        let value: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => return corrupt(&path, e.to_string()),
        };
        let ver = value.get("version").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
        if ver > SETTINGS_VERSION {
            return LoadResult {
                settings: Settings { read_only: true, ..Settings::default() },
                notice: Some(SwpError::SettingsCorrupt(format!(
                    "settings.json is version {ver}, newer than this build; using defaults read-only"
                ))),
            };
        }
        match serde_json::from_value::<Settings>(value) {
            Ok(s) => LoadResult {
                settings: s,
                notice: None,
            },
            Err(e) => corrupt(&path, e.to_string()),
        }
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        if self.read_only {
            return Ok(());
        }
        std::fs::create_dir_all(dir)?;
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        atomic_write(&dir.join("settings.json"), &json)
    }
}

fn corrupt(path: &Path, detail: String) -> LoadResult {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut bad = path.as_os_str().to_owned();
    bad.push(format!(".bad-{ts}"));
    let _ = std::fs::rename(path, PathBuf::from(bad));
    LoadResult {
        settings: Settings::default(),
        notice: Some(SwpError::SettingsCorrupt(detail)),
    }
}

/// temp file -> flush to disk -> replace target.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    replace(&tmp, path)
}

#[cfg(windows)]
fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    if !to.exists() {
        return std::fs::rename(from, to);
    }
    let w = |p: &Path| -> Vec<u16> { p.as_os_str().encode_wide().chain(Some(0)).collect() };
    let (dst, src) = (w(to), w(from));
    // SAFETY: both buffers are NUL-terminated and outlive the call.
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::ReplaceFileW(
            dst.as_ptr(),
            src.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

/// Keep the window on a monitor and no larger than it.
pub fn clamp_window(w: WindowState, monitors: &[Rect]) -> WindowState {
    let Some(first) = monitors.first() else {
        return w;
    };
    let overlap = |m: &Rect| {
        let ix = (w.x + w.w).min(m.x + m.w) - w.x.max(m.x);
        let iy = (w.y + w.h).min(m.y + m.h) - w.y.max(m.y);
        if ix > 0 && iy > 0 {
            ix as i64 * iy as i64
        } else {
            0
        }
    };
    let m = monitors
        .iter()
        .max_by_key(|m| overlap(m))
        .filter(|m| overlap(m) > 0)
        .unwrap_or(first);
    let ww = w.w.min(m.w).max(1);
    let hh = w.h.min(m.h).max(1);
    WindowState {
        x: w.x.clamp(m.x, m.x + m.w - ww),
        y: w.y.clamp(m.y, m.y + m.h - hh),
        w: ww,
        h: hh,
        ..w
    }
}

/// Portable (`portable.flag` beside the exe) uses `data\`; installed uses `%LOCALAPPDATA%\Sweepr`.
pub fn data_dir(exe_dir: &Path) -> PathBuf {
    if exe_dir.join("portable.flag").exists() {
        return exe_dir.join("data");
    }
    match std::env::var_os("LOCALAPPDATA") {
        Some(p) => PathBuf::from(p).join("Sweepr"),
        None => exe_dir.join("data"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sweepr-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn save_load_roundtrip() {
        let d = tmp("rt");
        let mut s = Settings::default();
        s.ignore.push("Foo.Bar".into());
        s.pins.insert("Baz".into(), "1.2".into());
        s.timeout_secs = 90;
        s.window = Some(WindowState {
            x: 10,
            y: 20,
            w: 800,
            h: 600,
            maximized: false,
            splitter: 300,
        });
        s.save(&d).unwrap();
        let r = Settings::load(&d);
        assert!(r.notice.is_none());
        assert_eq!(r.settings, s);
    }

    #[test]
    fn defaults_when_missing() {
        let r = Settings::load(&tmp("missing"));
        assert_eq!(r.settings.timeout_secs, 60);
        assert!(r.settings.sources.winget && r.settings.sources.choco && r.settings.sources.scoop);
        assert!(r.notice.is_none());
    }

    #[test]
    fn corrupt_file_is_quarantined_and_defaults_loaded() {
        let d = tmp("corrupt");
        fs::write(d.join("settings.json"), "{ not json").unwrap();
        let r = Settings::load(&d);
        assert_eq!(r.notice.as_ref().unwrap().code(), "SWP-301");
        assert_eq!(r.settings.timeout_secs, 60);
        assert!(!d.join("settings.json").exists());
        let bad = fs::read_dir(&d).unwrap().filter_map(|e| e.ok()).any(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("settings.json.bad-")
        });
        assert!(bad);
    }

    #[test]
    fn newer_version_is_not_overwritten() {
        let d = tmp("newer");
        let original = r#"{"version":99,"ignore":["x"],"future_field":true}"#;
        fs::write(d.join("settings.json"), original).unwrap();
        let r = Settings::load(&d);
        assert!(r.settings.read_only);
        r.settings.save(&d).unwrap();
        assert_eq!(
            fs::read_to_string(d.join("settings.json")).unwrap(),
            original
        );
    }

    #[test]
    fn window_on_removed_monitor_is_clamped() {
        let w = WindowState {
            x: 5000,
            y: 4000,
            w: 1000,
            h: 700,
            maximized: false,
            splitter: 0,
        };
        let m = [Rect {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        }];
        let c = clamp_window(w, &m);
        assert!(c.x >= 0 && c.x + c.w <= 1920);
        assert!(c.y >= 0 && c.y + c.h <= 1080);
    }

    #[test]
    fn oversized_window_shrinks_to_monitor() {
        let w = WindowState {
            x: 0,
            y: 0,
            w: 4000,
            h: 3000,
            maximized: false,
            splitter: 0,
        };
        let c = clamp_window(
            w,
            &[Rect {
                x: 0,
                y: 0,
                w: 1366,
                h: 768,
            }],
        );
        assert_eq!((c.w, c.h), (1366, 768));
    }

    #[test]
    fn portable_flag_selects_adjacent_data_dir() {
        let d = tmp("portable");
        if std::env::var_os("LOCALAPPDATA").is_some() {
            assert_ne!(data_dir(&d), d.join("data"));
        }
        fs::write(d.join("portable.flag"), "").unwrap();
        assert_eq!(data_dir(&d), d.join("data"));
    }
}
