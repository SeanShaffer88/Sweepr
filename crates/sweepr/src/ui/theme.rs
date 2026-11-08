//! TesselGUI tokens used by Sweepr: charcoal neutrals, one crimson primary, text scale.
//! Colors are 0xRRGGBB. Hex values for neutrals are Sweepr's choice within TesselGUI's
//! rules (3 charcoal levels, 4.5:1 text, 3:1 primary with white label); tested below.

pub const DENSE_PX: i32 = 12;
pub const LABEL_PX: i32 = 13;
pub const CAPTION_PX: i32 = 11;
pub const ROW_HEIGHT_PX: i32 = 28;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub bg0: u32,
    pub bg1: u32,
    pub bg2: u32,
    pub edge: u32,
    pub text: u32,
    pub text_dim: u32,
    pub primary: u32,
    pub selection: u32,
}

pub fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            bg0: 0x17171B,
            bg1: 0x1F1F25,
            bg2: 0x2A2A32,
            edge: 0x3A3A44,
            text: 0xE8E8EE,
            text_dim: 0xA6A6B2,
            primary: 0xD02946,
            selection: 0x76172A,
        }
    } else {
        Palette {
            bg0: 0xF3F3F6,
            bg1: 0xFFFFFF,
            bg2: 0xE6E6EC,
            edge: 0xC9C9D2,
            text: 0x1B1B20,
            text_dim: 0x55555F,
            primary: 0xD02946,
            selection: 0x76172A,
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

    #[test]
    fn primary_with_white_label_meets_3_to_1() {
        for dark in [true, false] {
            let p = palette(dark);
            assert!(contrast(p.primary, 0xFFFFFF) >= 3.0, "dark={dark}");
        }
    }

    #[test]
    fn body_text_meets_4_5_to_1_on_every_surface() {
        for dark in [true, false] {
            let p = palette(dark);
            for bg in [p.bg0, p.bg1, p.bg2] {
                assert!(contrast(p.text, bg) >= 4.5, "text dark={dark} bg={bg:06X}");
                assert!(
                    contrast(p.text_dim, bg) >= 4.5,
                    "dim dark={dark} bg={bg:06X}"
                );
            }
            assert!(
                contrast(0xFFFFFF, p.selection) >= 4.5,
                "selection dark={dark}"
            );
        }
    }

    #[test]
    fn colorref_swaps_to_bgr() {
        assert_eq!(colorref(0xD02946), 0x4629D0);
    }

    #[test]
    fn type_scale_matches_tesselgui() {
        assert_eq!((DENSE_PX, LABEL_PX, CAPTION_PX), (12, 13, 11));
        assert_eq!(ROW_HEIGHT_PX, 28);
    }
}
