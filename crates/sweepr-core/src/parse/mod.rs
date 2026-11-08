//! Pure text parsers for manager output. All fail closed with `SWP-103`.

pub mod choco;
pub mod scoop;
pub mod winget;

pub use choco::parse_choco;
pub use scoop::parse_scoop;
pub use winget::parse_winget;

/// Display width of one char in terminal cells (0, 1 or 2).
pub fn cell_width(c: char) -> usize {
    let u = c as u32;
    if (0x0300..=0x036F).contains(&u) || u == 0x200B {
        return 0;
    }
    let wide = (0x1100..=0x115F).contains(&u)
        || (0x2E80..=0xA4CF).contains(&u)
        || (0xAC00..=0xD7A3).contains(&u)
        || (0xF900..=0xFAFF).contains(&u)
        || (0xFE30..=0xFE6F).contains(&u)
        || (0xFF00..=0xFF60).contains(&u)
        || (0xFFE0..=0xFFE6).contains(&u)
        || (0x1F300..=0x1F64F).contains(&u)
        || (0x1F900..=0x1F9FF).contains(&u)
        || (0x20000..=0x3FFFD).contains(&u);
    if wide {
        2
    } else {
        1
    }
}

/// Split output into lines; keep only the text after the last carriage return
/// (progress spinners redraw a line with `\r`).
pub(crate) fn clean_lines(out: &str) -> Vec<String> {
    out.split('\n')
        .map(|l| {
            l.trim_end_matches('\r')
                .rsplit('\r')
                .next()
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

/// Cell offset of byte index `byte` within `line`.
pub(crate) fn cell_offset(line: &str, byte: usize) -> usize {
    line[..byte].chars().map(cell_width).sum()
}

/// Text of `line` whose chars start in cell range `[start, end)`.
pub(crate) fn slice_cells(line: &str, start: usize, end: Option<usize>) -> String {
    let mut cell = 0;
    let mut out = String::new();
    for c in line.chars() {
        if cell >= start && end.map_or(true, |e| cell < e) {
            out.push(c);
        }
        cell += cell_width(c);
    }
    out.trim().to_string()
}

pub(crate) fn is_rule(line: &str) -> bool {
    let t = line.trim();
    !t.is_empty() && t.chars().all(|c| c == '-' || c == ' ')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_width_counts_wide_chars() {
        assert_eq!(cell_width('a'), 1);
        assert_eq!(cell_width('微'), 2);
        assert_eq!(cell_width('\u{0301}'), 0);
    }

    #[test]
    fn uptodate_outputs_are_empty_lists() {
        let w = include_str!("../../../../tests/fixtures/winget_uptodate.synthetic.txt");
        assert_eq!(parse_winget(w), Ok(vec![]));
        assert_eq!(parse_choco(""), Ok(vec![]));
        let s = include_str!("../../../../tests/fixtures/scoop_uptodate.synthetic.txt");
        assert_eq!(parse_scoop(s), Ok(vec![]));
    }
}
