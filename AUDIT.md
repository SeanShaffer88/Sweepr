# Sweepr Audit

Evidence labels: **Verified** (run and observed) · **Partially verified** · **Unverified**.

Development ran in a Linux sandbox. Windows code was cross-checked (`cargo check`, `cargo clippy --target x86_64-pc-windows-msvc`) and never executed.

| Area | Label | Notes |
|---|---|---|
| Model, error codes, id validator, rules, settings, job validation | **Verified** (Linux unit tests, 87 tests) | |
| Parsers (WinGet, Choco, Scoop) | **Unverified** against real output | Only synthetic fixtures (`*.synthetic.txt`). Run `tools/capture-fixtures.ps1`, add `*.real.txt`, parameterize tests. WinGet header is localized. |
| List model, sort, filter, status text, command palette filter | **Verified** (unit tests) | |
| Theme contrast | **Verified** (computed in tests) | |
| Process execution, timeouts, tree kill | **Partially verified** | Unix path tested; Job Object path compiled only. |
| Elevation, named pipe, SDDL, journal | **Unverified** | Wire format and job validation tested; Windows pipe and UAC never run. |
| Win32 GUI (layout, DPI, tooltips, owner-draw, accelerators, dark mode) | **Unverified** | Compiles and lints on msvc target. |
| Keyboard-only walkthrough | **Unverified** | |
| CLI | **Partially verified** | Host tests with fake adapters; real managers not run. |
| CI, MSI, portable zip | **Unverified** | Not yet run. |

## Risks

1. Parser drift: manager output changes break lists. Mitigation: parse-failed error SWP-103 shows the raw output in the log.
2. Elevation path untested on Windows. Mitigation: validate on a VM before any release.
3. WinGet scope Unknown may fail without elevation.
4. Telemetry: none by design; no network code in Sweepr itself.

## Gate before release

1. Run fixture capture and replace synthetic fixtures.
2. Smoke test on Windows 10 and 11: check, update user package, update system package (one UAC), cancel, theme switch, DPI change.
3. CI green on windows-latest.
