//! Window geometry in 96-dpi logical pixels, so a saved size survives DPI changes.

use sweepr_core::settings::{clamp_window, Rect, WindowState};

pub fn to_logical(px: i32, dpi: u32) -> i32 {
    ((px as i64 * 96 + dpi as i64 / 2) / dpi.max(1) as i64) as i32
}

pub fn to_device(logical: i32, dpi: u32) -> i32 {
    ((logical as i64 * dpi as i64 + 48) / 96) as i32
}

/// Device pixels at `dpi` to the logical form that is stored in settings.
pub fn to_saved(ws: WindowState, dpi: u32) -> WindowState {
    WindowState {
        x: to_logical(ws.x, dpi),
        y: to_logical(ws.y, dpi),
        w: to_logical(ws.w, dpi),
        h: to_logical(ws.h, dpi),
        ..ws
    }
}

/// Saved logical state to device pixels, then clamped onto the monitors that exist now.
pub fn restore(saved: WindowState, dpi: u32, monitors: &[Rect]) -> WindowState {
    let dev = WindowState {
        x: to_device(saved.x, dpi),
        y: to_device(saved.y, dpi),
        w: to_device(saved.w, dpi),
        h: to_device(saved.h, dpi),
        ..saved
    };
    clamp_window(dev, monitors)
}

/// First-run window: 1000x640 logical, centered, never larger than 90% of the first monitor.
pub fn default_window(dpi: u32, monitors: &[Rect]) -> WindowState {
    let (mut w, mut h) = (to_device(1000, dpi), to_device(640, dpi));
    let (mut x, mut y) = (100, 100);
    if let Some(m) = monitors.first() {
        w = w.min(m.w * 9 / 10);
        h = h.min(m.h * 9 / 10);
        x = m.x + (m.w - w) / 2;
        y = m.y + (m.h - h) / 2;
    }
    WindowState {
        x,
        y,
        w,
        h,
        maximized: false,
        splitter: to_logical(180, 96),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sweepr_core::settings::{Rect, WindowState};

    #[test]
    fn geometry_roundtrip_dpi_scale() {
        assert_eq!(to_logical(1500, 144), 1000);
        assert_eq!(to_device(1000, 144), 1500);
        // logical storage is whole pixels, so arbitrary sizes round-trip within 1 px
        assert!((to_device(to_logical(777, 120), 120) - 777).abs() <= 1);
    }

    #[test]
    fn saved_state_restores_on_new_dpi() {
        let ws = WindowState {
            x: 100,
            y: 50,
            w: 1000,
            h: 600,
            maximized: false,
            splitter: 180,
        };
        let monitors = [Rect {
            x: 0,
            y: 0,
            w: 3840,
            h: 2160,
        }];
        let r = restore(ws, 192, &monitors);
        assert_eq!((r.w, r.h), (2000, 1200));
        assert!(r.x >= 0 && r.x + r.w <= 3840);
    }

    #[test]
    fn restore_clamps_when_monitor_removed() {
        let ws = WindowState {
            x: 5000,
            y: 100,
            w: 1000,
            h: 600,
            maximized: false,
            splitter: 180,
        };
        let monitors = [Rect {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        }];
        let r = restore(ws, 96, &monitors);
        assert!(r.x + r.w <= 1920 && r.x >= 0);
    }

    #[test]
    fn default_state_fits_small_screen() {
        let monitors = [Rect {
            x: 0,
            y: 0,
            w: 1366,
            h: 768,
        }];
        let r = default_window(96, &monitors);
        assert!(r.w <= 1366 && r.h <= 768);
    }
}
