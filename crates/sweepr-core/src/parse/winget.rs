use super::{cell_offset, clean_lines, is_rule, slice_cells};
use crate::errors::SwpError;
use crate::model::{Manager, Scope, UpdatePackage};

fn is_noise(l: &str) -> bool {
    let t = l.trim();
    if t.is_empty() {
        return true;
    }
    if !t.chars().any(|c| c.is_alphabetic()) {
        return true; // spinner or bar glyphs, digits, percent
    }
    // download progress such as "1024 KB / 2.00 MB"
    t.contains('/') && (t.contains("KB") || t.contains("MB") || t.contains("GB"))
}

fn is_header(l: &str) -> bool {
    let t = l.trim_start();
    t.starts_with("Name") && t.contains("Id") && t.contains("Version") && t.contains("Available")
}

fn is_footer(l: &str) -> bool {
    let t = l.to_lowercase();
    t.contains("upgrade") && t.contains("available")
}

/// Parse `winget upgrade`. There is no JSON output, so columns are cut by the
/// header's cell offsets (display width, so wide chars keep alignment).
pub fn parse_winget(out: &str) -> Result<Vec<UpdatePackage>, SwpError> {
    let lines = clean_lines(out);
    let mut v = Vec::new();
    let mut tables = 0usize;
    let mut i = 0;
    while i < lines.len() {
        if !is_header(&lines[i]) {
            i += 1;
            continue;
        }
        let h = &lines[i];
        let at = |name: &str| h.find(name).map(|b| cell_offset(h, b));
        let (Some(c_id), Some(c_ver), Some(c_avail)) = (at("Id"), at("Version"), at("Available"))
        else {
            break;
        };
        let c_src = at("Source");
        i += 1;
        if i < lines.len() && is_rule(&lines[i]) {
            i += 1;
        }
        while i < lines.len() {
            let l = &lines[i];
            if l.trim().is_empty() || is_footer(l) {
                break;
            }
            let name = slice_cells(l, 0, Some(c_id));
            let id = slice_cells(l, c_id, Some(c_ver));
            let cur = slice_cells(l, c_ver, Some(c_avail));
            let avail = slice_cells(l, c_avail, c_src);
            if !id.is_empty() && !avail.is_empty() {
                v.push(UpdatePackage {
                    id,
                    name,
                    current_version: cur,
                    available_version: avail,
                    manager: Manager::Winget,
                    scope: Scope::Unknown,
                    elevation_required: false,
                    pinned_by_manager: tables > 0,
                    ignored: false,
                    user_pin: None,
                });
            }
            i += 1;
        }
        tables += 1;
    }
    if tables == 0 {
        let real: Vec<&String> = lines.iter().filter(|l| !is_noise(l)).collect();
        let nothing = real.iter().all(|l| {
            l.contains("No installed package found") || l.contains("No applicable upgrade")
        });
        if !nothing {
            return Err(SwpError::ParseFailed {
                manager: Manager::Winget,
                detail: "upgrade table header not found".into(),
            });
        }
    }
    Ok(v)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winget_parses_table_and_skips_noise() {
        let out = include_str!("../../../../tests/fixtures/winget_upgrade.synthetic.txt");
        let v = parse_winget(out).unwrap();
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].id, "Google.Chrome");
        assert_eq!(v[0].name, "Google Chrome");
        assert_eq!(v[0].current_version, "120.0.6099.109");
        assert_eq!(v[0].available_version, "121.0.6167.85");
        assert_eq!(v[0].manager, Manager::Winget);
        assert_eq!(v[0].scope, Scope::Unknown);
        assert_eq!(v[1].name, "Mozilla Firefox (x64 en-US)");
        assert_eq!(v[2].current_version, "Unknown");
    }

    #[test]
    fn winget_cjk_name_keeps_columns() {
        let out = include_str!("../../../../tests/fixtures/winget_upgrade_cjk.synthetic.txt");
        let v = parse_winget(out).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].name, "微信 WeChat");
        assert_eq!(v[0].id, "Tencent.WeChat");
        assert_eq!(v[0].current_version, "3.9.1");
        assert_eq!(v[0].available_version, "3.9.2");
    }

    #[test]
    fn winget_pinned_footer_sets_flag_and_keeps_less_than_version() {
        let out = include_str!("../../../../tests/fixtures/winget_upgrade_pinned.synthetic.txt");
        let v = parse_winget(out).unwrap();
        assert_eq!(v.len(), 2);
        assert!(!v[0].pinned_by_manager);
        assert!(v[1].pinned_by_manager);
        assert_eq!(v[1].id, "Foo.Tool");
        assert_eq!(v[1].current_version, "< 2.0");
    }

    #[test]
    fn winget_real_capture_v1_30_preview() {
        // Captured from winget v1.30.140-preview, Windows 10.0.26300, en-US.
        let out = include_str!("../../../../tests/fixtures/winget_upgrade.real.txt");
        let v = parse_winget(out).unwrap();
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].name, "Python Launcher");
        assert_eq!(v[0].id, "Python.Launcher");
        assert_eq!(v[0].current_version, "< 3.15.2");
        assert_eq!(v[0].available_version, "3.15.2");
        assert_eq!(v[1].id, "qBittorrent.qBittorrent");
        assert_eq!(v[1].current_version, "5.2.4.10");
        assert_eq!(v[1].available_version, "5.3.0");
        assert_eq!(v[2].id, "CodeSector.TeraCopy");
        assert_eq!(v[2].available_version, "4.0.5");
    }

    #[test]
    fn garbled_winget_without_header_is_parse_failed() {
        let e = parse_winget("Google Chrome   Google.Chrome   120   121\n").unwrap_err();
        assert_eq!(e.code(), "SWP-103");
    }
}
