# Sweepr — Agent Notes

## Project Manifest

- **Tier:** t1b (Adaptive Engineering Contract V4.0)
- **Rationale:** Windows GUI utility built on stock Win32 / Common Controls v6 — no rich rendering engine, no service, driver, shell extension, or tray autostart.
- **Sanctioned Alternate Stack:** Native Default (Rust, `windows-sys`, static CRT)
- **Targets:** windows-x64
- **Renderer:** Win32 / Common Controls v6, TesselGUI palette
- **Release assets:** `Sweepr-v{VERSION}-x64-Setup.exe` + `Sweepr-v{VERSION}-x64-Portable.zip` + `SHA256SUMS.txt` — no MSI, no other variants
- **Overrides:** none
- **Dropped sections:** none

## Working rules

- Dependencies stay at `windows-sys`, `serde`, `serde_json` — nothing else, including build dependencies (the icon is compiled with the SDK's `rc.exe` from `build.rs`).
- Manager commands are argument vectors from fixed templates, never a shell. Package ids must pass `valid_package_id`.
- Settings carry `schema_version` with tested upward migrations (the pre-rename `version` key still loads). Corrupt files are quarantined, never silently overwritten.
- Full gate before any publish: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, plus a linked Windows-target build.
- Windows runtime behaviour is Unverified until it runs on a real machine; see `AUDIT.md` and `docs/field-pass.md`.
- Releases need the owner's explicit confirmation first. Never ship an MSI.
