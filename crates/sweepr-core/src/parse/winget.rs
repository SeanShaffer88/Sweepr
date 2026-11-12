use super::{cell_width, clean_lines, is_rule, slice_cells};
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

fn is_footer(l: &str) -> bool {
    let t = l.to_lowercase();
    t.contains("upgrade") && t.contains("available")
}

/// Cell offsets where each header token starts (tokens are separated by spaces).
fn token_starts(h: &str) -> Vec<usize> {
    let mut v = Vec::new();
    let (mut cell, mut prev_space) = (0usize, true);
    for c in h.chars() {
        let sp = c.is_whitespace();
        if !sp && prev_space {
            v.push(cell);
        }
        prev_space = sp;
        cell += cell_width(c);
    }
    v
}

fn char_at_cell(line: &str, at: usize) -> Option<char> {
    let mut cell = 0;
    for c in line.chars() {
        if at < cell + cell_width(c).max(1) && at >= cell {
            return Some(c);
        }
        cell += cell_width(c);
    }
    None
}

/// Column starts for a header followed by a rule line. WinGet localizes the header words
/// but always emits Name, Id, Version, Available and optionally Source, in that order.
/// A single-word-per-column header gives 4 or 5 tokens. A header with multi-word names is
/// resolved from the data rows (a column starts where some row begins a cell); when
/// that does not give exactly 4 or 5 columns the table is rejected (fail closed, SWP-103).
fn column_starts(h: &str, rows: &[&String]) -> Option<Vec<usize>> {
    let toks = token_starts(h);
    if (4..=5).contains(&toks.len()) {
        return Some(toks);
    }
    // A real column start is where some data row begins a cell: non-blank there, blank
    // just before. Spaces inside a multi-word header name fail this.
    let starts: Vec<usize> = toks
        .into_iter()
        .filter(|&c| {
            rows.iter().any(|r| {
                char_at_cell(r, c).is_some_and(|ch| !ch.is_whitespace())
                    && (c == 0 || char_at_cell(r, c - 1).is_none_or(char::is_whitespace))
            })
        })
        .collect();
    (4..=5).contains(&starts.len()).then_some(starts)
}

/// A data row must be aligned to the header: a blank cell right before every column start
/// after the first, and text reaching the Available column. Footers fail this.
fn row_fits(l: &str, starts: &[usize]) -> bool {
    let reach: usize = l.chars().map(cell_width).sum();
    reach > starts[3]
        && starts[1..4]
            .iter()
            .all(|c| char_at_cell(l, c - 1).is_none_or(char::is_whitespace))
}

/// Parse `winget upgrade`. There is no JSON output, so columns are cut by the header's cell
/// offsets (display width, so wide chars keep alignment). The header may be localized.
pub fn parse_winget(out: &str) -> Result<Vec<UpdatePackage>, SwpError> {
    let lines = clean_lines(out);
    let mut v = Vec::new();
    let mut tables = 0usize;
    let mut i = 0;
    while i + 1 < lines.len() {
        // A header is the line right before a rule line.
        if is_rule(&lines[i]) || lines[i].trim().is_empty() || !is_rule(&lines[i + 1]) {
            i += 1;
            continue;
        }
        let h = lines[i].clone();
        let body: Vec<&String> = lines[i + 2..]
            .iter()
            .take_while(|l| !l.trim().is_empty())
            .collect();
        let Some(starts) = column_starts(&h, &body) else {
            i += 1;
            continue;
        };
        let (c_id, c_ver, c_avail) = (starts[1], starts[2], starts[3]);
        let c_src = starts.get(4).copied();
        i += 2;
        while i < lines.len() {
            let l = &lines[i];
            if l.trim().is_empty() || is_footer(l) || !row_fits(l, &starts) {
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
        // Nothing to upgrade prints only short sentences, in the user's language. Anything
        // that still looks like columns (a run of 2+ spaces between words) is a table we
        // could not read, so fail closed.
        let columnar = real.iter().any(|l| l.trim().contains("  "));
        let nothing = real.len() <= 3 && !columnar;
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
    fn winget_german_header_parses() {
        let out = include_str!("../../../../tests/fixtures/winget_upgrade_de.synthetic.txt");
        let v = parse_winget(out).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].id, "Mozilla.Firefox");
        assert_eq!(v[1].id, "Notepad++.Notepad++");
        assert_eq!(v[1].available_version, "8.6.1");
    }

    #[test]
    fn winget_chinese_header_parses() {
        let out = include_str!("../../../../tests/fixtures/winget_upgrade_zh.synthetic.txt");
        let v = parse_winget(out).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].name, "微信 WeChat");
        assert_eq!(v[0].id, "Tencent.WeChat");
        assert_eq!(v[1].available_version, "24.07");
    }

    #[test]
    fn winget_multiword_header_resolved_by_gutters() {
        let out =
            include_str!("../../../../tests/fixtures/winget_upgrade_fr_multiword.synthetic.txt");
        let v = parse_winget(out).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].id, "Git.Git");
        assert_eq!(v[0].current_version, "2.43.0");
        assert_eq!(v[1].id, "cURL.cURL");
        assert_eq!(v[1].available_version, "8.5.0");
    }

    #[test]
    fn winget_localized_nothing_to_upgrade_is_empty() {
        let out = include_str!("../../../../tests/fixtures/winget_uptodate_de.synthetic.txt");
        assert_eq!(parse_winget(out), Ok(vec![]));
    }

    #[test]
    fn garbled_winget_without_header_is_parse_failed() {
        let e = parse_winget("Google Chrome   Google.Chrome   120   121\n").unwrap_err();
        assert_eq!(e.code(), "SWP-103");
    }
}
