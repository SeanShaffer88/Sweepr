/// Package ids reach command lines, so only a conservative charset passes.
pub fn valid_package_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_ids() {
        for id in [
            "Google.Chrome",
            "7zip",
            "ripgrep",
            "Microsoft.VisualStudio.2022.Community",
            "git-lfs",
            "c++",
        ] {
            assert!(valid_package_id(id), "{id}");
        }
    }

    #[test]
    fn rejects_hostile_ids() {
        let long = "a".repeat(129);
        for id in [
            "foo & calc",
            "a;b",
            "..\\x",
            "a b",
            "\"x\"",
            "",
            long.as_str(),
            "$(x)",
            "a\nb",
        ] {
            assert!(!valid_package_id(id), "{id:?}");
        }
    }
}
