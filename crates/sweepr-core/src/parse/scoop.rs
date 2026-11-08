use super::{cell_offset, clean_lines, is_rule, slice_cells};
use crate::errors::SwpError;
use crate::model::{Manager, Scope, UpdatePackage};

/// Parse `scoop status` (Name / Installed Version / Latest Version table).
pub fn parse_scoop(out: &str) -> Result<Vec<UpdatePackage>, SwpError> {
    let lines = clean_lines(out);
    let hi = lines.iter().position(|l| {
        let t = l.trim_start();
        t.starts_with("Name") && t.contains("Installed Version") && t.contains("Latest Version")
    });
    let Some(hi) = hi else {
        let all = lines.join(" ");
        if all.trim().is_empty() || all.contains("up to date") || all.contains("Everything is ok") {
            return Ok(vec![]);
        }
        return Err(SwpError::ParseFailed {
            manager: Manager::Scoop,
            detail: "status table header not found".into(),
        });
    };
    let h = &lines[hi];
    let c_inst = cell_offset(h, h.find("Installed Version").unwrap_or(0));
    let c_latest = cell_offset(h, h.find("Latest Version").unwrap_or(0));
    let c_next = h.find("Missing Dependencies").map(|b| cell_offset(h, b));
    let mut v = Vec::new();
    for l in lines.iter().skip(hi + 1) {
        if l.trim().is_empty() {
            break;
        }
        if is_rule(l) || l.trim_start().starts_with("--") {
            continue;
        }
        let name = slice_cells(l, 0, Some(c_inst));
        let cur = slice_cells(l, c_inst, Some(c_latest));
        let latest = slice_cells(l, c_latest, c_next);
        let latest = latest.split_whitespace().next().unwrap_or("").to_string();
        if name.is_empty() || latest.is_empty() {
            continue;
        }
        v.push(UpdatePackage {
            id: name.clone(),
            name,
            current_version: cur,
            available_version: latest,
            manager: Manager::Scoop,
            scope: Scope::User,
            elevation_required: false,
            pinned_by_manager: false,
            ignored: false,
            user_pin: None,
        });
    }
    Ok(v)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoop_parses_status_table() {
        let out = include_str!("../../../../tests/fixtures/scoop_status.synthetic.txt");
        let v = parse_scoop(out).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].id, "ripgrep");
        assert_eq!(v[0].current_version, "14.0.3");
        assert_eq!(v[0].available_version, "14.1.0");
        assert_eq!(v[0].manager, Manager::Scoop);
        assert_eq!(v[0].scope, Scope::User);
        assert!(!v[0].elevation_required);
    }

    #[test]
    fn scoop_rows_without_latest_version_are_skipped() {
        let out = "Name  Installed Version Latest Version Missing Dependencies Info\n----  ----------------- -------------- -------------------- ----\nbroken                                                       Install failed\n";
        assert_eq!(parse_scoop(out), Ok(vec![]));
    }
}
