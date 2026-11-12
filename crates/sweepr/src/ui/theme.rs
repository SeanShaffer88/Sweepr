//! TesselGUI revision 2 tokens (GitHub preset, the default): surfaces, text, edge, action,
//! selection, focus and status colors, with the Compact type scale and row height.
//! Colors are 0xRRGGBB. Values are copied from TesselGUI `docs/tokens.json` (v1.2.x) and the
//! contrast contract of `docs/materials.md` is re-checked in the tests below.

pub const DENSE_PX: i32 = 12;
pub const LABEL_PX: i32 = 13;
#[cfg(test)]
pub const CAPTION_PX: i32 = 11;
pub const ROW_HEIGHT_PX: i32 = 28;
/// Compact control height and the 8 px default gap between controls.
pub const CONTROL_PX: i32 = 32;
pub const GAP_PX: i32 = 8;
/// Panel header height and the 2 px active-region rule.
pub const PANEL_HEADER_PX: i32 = 28;
pub const ACTIVE_RULE_PX: i32 = 2;
/// Width of the selected-row marker bar.
pub const SELECT_BAR_PX: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub background: u32,
    pub surface: u32,
    pub raised: u32,
    pub selected: u32,
    pub fg: u32,
    pub muted: u32,
    pub edge: u32,
    pub border: u32,
    pub grid: u32,
    pub action: u32,
    pub action_hover: u32,
    pub action_pressed: u32,
    pub action_fg: u32,
    pub selection: u32,
    pub focus: u32,
    pub info: u32,
    pub warning: u32,
    pub success: u32,
    pub danger: u32,
}

pub fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            background: 0x0D1117,
            surface: 0x0E1218,
            raised: 0x0F1319,
            selected: 0x1F2630,
            fg: 0xF0F6FC,
            muted: 0x9198A1,
            edge: 0x3D444D,
            border: 0x848D97,
            grid: 0x1C2128,
            action: 0x1A7F37,
            action_hover: 0x238636,
            action_pressed: 0x116329,
            action_fg: 0xFFFFFF,
            selection: 0x2F81F7,
            focus: 0x79B8FF,
            info: 0xA5D6FF,
            warning: 0xD29922,
            success: 0x3FB950,
            danger: 0xFFA198,
        }
    } else {
        Palette {
            background: 0xEDEDF0,
            surface: 0xFFFFFF,
            raised: 0xE4E4E9,
            selected: 0xDDDDE4,
            fg: 0x202023,
            muted: 0x50505B,
            edge: 0xB8B8BD,
            border: 0x676772,
            grid: 0xF2F2F5,
            action: 0x1A7F37,
            action_hover: 0x238636,
            action_pressed: 0x116329,
            action_fg: 0xFFFFFF,
            selection: 0x0969DA,
            focus: 0x0550AE,
            info: 0x174E85,
            warning: 0x735100,
            success: 0x236340,
            danger: 0x9D2B1F,
        }
    }
}

/// Status severity. Every message carries an icon glyph and a word, never color alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Success,
    Warning,
    Error,
}

impl Severity {
    pub fn glyph(self) -> &'static str {
        match self {
            Severity::Info => "\u{2139}",
            Severity::Success => "\u{2713}",
            Severity::Warning => "\u{26A0}",
            Severity::Error => "\u{2716}",
        }
    }
    pub fn word(self) -> &'static str {
        match self {
            Severity::Info => "Info",
            Severity::Success => "Done",
            Severity::Warning => "Warning",
            Severity::Error => "Error",
        }
    }
    pub fn color(self, p: &Palette) -> u32 {
        match self {
            Severity::Info => p.info,
            Severity::Success => p.success,
            Severity::Warning => p.warning,
            Severity::Error => p.danger,
        }
    }
}

#[cfg(test)]
fn channel(v: u32) -> f64 {
    let c = v as f64 / 255.0;
    if c <= 0.03928 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
fn luminance(rgb: u32) -> f64 {
    0.2126 * channel((rgb >> 16) & 255)
        + 0.7152 * channel((rgb >> 8) & 255)
        + 0.0722 * channel(rgb & 255)
}

/// WCAG contrast ratio between two 0xRRGGBB colors.
#[cfg(test)]
pub fn contrast(a: u32, b: u32) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    let (hi, lo) = if x > y { (x, y) } else { (y, x) };
    (hi + 0.05) / (lo + 0.05)
}

/// 0xRRGGBB to a Win32 COLORREF (0x00BBGGRR).
pub fn colorref(rgb: u32) -> u32 {
    ((rgb & 0xFF) << 16) | (rgb & 0xFF00) | ((rgb >> 16) & 0xFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surfaces(p: &Palette) -> [u32; 4] {
        [p.background, p.surface, p.raised, p.selected]
    }

    #[test]
    fn text_and_status_text_meet_4_5_on_all_four_surfaces() {
        for dark in [true, false] {
            let p = palette(dark);
            for bg in surfaces(&p) {
                for (name, c) in [
                    ("fg", p.fg),
                    ("muted", p.muted),
                    ("info", p.info),
                    ("warning", p.warning),
                    ("success", p.success),
                    ("danger", p.danger),
                ] {
                    assert!(contrast(c, bg) >= 4.5, "{name} dark={dark} bg={bg:06X}");
                }
            }
        }
    }

    #[test]
    fn primary_action_contract() {
        for dark in [true, false] {
            let p = palette(dark);
            assert!(contrast(p.action_fg, p.action) >= 5.0, "rest dark={dark}");
            assert!(contrast(p.action_fg, p.action_pressed) >= 5.0);
            assert!(contrast(p.action_fg, p.action_hover) >= 4.5);
            for bg in [p.background, p.surface, p.raised] {
                assert!(
                    contrast(p.action, bg) >= 3.0,
                    "fill dark={dark} bg={bg:06X}"
                );
            }
            assert!(contrast(p.action_hover, p.action) >= 1.05);
            assert!(contrast(p.action_pressed, p.action) >= 1.05);
        }
    }

    #[test]
    fn structure_and_indicators_meet_their_ratios() {
        for dark in [true, false] {
            let p = palette(dark);
            for bg in [p.background, p.surface, p.raised] {
                assert!(contrast(p.edge, bg) >= 1.4, "edge dark={dark}");
            }
            for bg in surfaces(&p) {
                assert!(contrast(p.border, bg) >= 3.0, "border dark={dark}");
                assert!(contrast(p.selection, bg) >= 3.0, "selection dark={dark}");
                assert!(contrast(p.focus, bg) >= 3.0, "focus dark={dark}");
                let g = contrast(p.grid, bg);
                assert!(g <= 1.30, "grid dark={dark} bg={bg:06X} {g}");
            }
            assert!(contrast(p.selected, p.surface) >= 1.10, "selected fill");
            assert!(contrast(p.focus, p.selection) >= 1.30, "focus vs selection");
        }
    }

    #[test]
    fn colorref_swaps_to_bgr() {
        assert_eq!(colorref(0x1A7F37), 0x377F1A);
    }

    #[test]
    fn type_scale_and_density_match_tesselgui() {
        assert_eq!((DENSE_PX, LABEL_PX, CAPTION_PX), (12, 13, 11));
        assert_eq!((ROW_HEIGHT_PX, CONTROL_PX, GAP_PX), (28, 32, 8));
        assert_eq!((PANEL_HEADER_PX, ACTIVE_RULE_PX, SELECT_BAR_PX), (28, 2, 3));
    }

    #[test]
    fn severity_has_glyph_and_word() {
        for s in [
            Severity::Info,
            Severity::Success,
            Severity::Warning,
            Severity::Error,
        ] {
            assert!(!s.glyph().is_empty() && !s.word().is_empty());
        }
    }
}
