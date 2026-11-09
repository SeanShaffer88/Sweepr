use crate::errors::SwpError;
use crate::model::{Manager, Scope, UpdatePackage};

/// Parse `choco outdated --limit-output`: `name|current|available|pinned`.
pub fn parse_choco(out: &str) -> Result<Vec<UpdatePackage>, SwpError> {
    let mut v = Vec::new();
    for line in out.lines() {
        let line = line.trim();
        if !line.contains('|') {
            continue;
        }
        let f: Vec<&str> = line.split('|').collect();
        if f.len() != 4 || f[0].is_empty() {
            return Err(SwpError::ParseFailed {
                manager: Manager::Choco,
                detail: format!("unexpected line: {line}"),
            });
        }
        v.push(UpdatePackage {
            id: f[0].to_string(),
            name: f[0].to_string(),
            current_version: f[1].to_string(),
            available_version: f[2].to_string(),
            manager: Manager::Choco,
            scope: Scope::Machine,
            elevation_required: true,
            pinned_by_manager: f[3].eq_ignore_ascii_case("true"),
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
    fn choco_parses_pipe_lines() {
        let out = include_str!("../../../../tests/fixtures/choco_outdated.synthetic.txt");
        let v = parse_choco(out).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].id, "git");
        assert_eq!(v[0].current_version, "2.43.0");
        assert_eq!(v[0].available_version, "2.44.0");
        assert_eq!(v[0].manager, Manager::Choco);
        assert_eq!(v[0].scope, Scope::Machine);
        assert!(v[0].elevation_required);
        assert!(!v[0].pinned_by_manager);
        assert!(v[1].pinned_by_manager);
    }

    #[test]
    fn choco_real_capture_nothing_outdated_is_empty_ok() {
        // Captured from choco 2.7.5 with --limit-output: no output when nothing is outdated.
        let out = include_str!("../../../../tests/fixtures/choco_outdated.real.txt");
        assert!(parse_choco(out).unwrap().is_empty());
    }

    #[test]
    fn choco_bad_line_is_parse_failed() {
        let e = parse_choco("git|2.43.0\n").unwrap_err();
        assert_eq!(e.code(), "SWP-103");
    }
}
