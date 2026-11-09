# Sweepr Audit

Evidence labels: **Verified** (run and observed) · **Partially verified** · **Unverified**.

Development ran in a Linux sandbox. Windows code was cross-checked (`cargo check`, `cargo clippy --target x86_64-pc-windows-msvc`) and never executed.

| Area | Label | Notes |
|---|---|---|
| Model, error codes, id validator, rules, settings, job validation | **Verified** (Linux unit tests, 87 tests) | |
| Parsers | **Partially verified** | WinGet: real capture (v1.30.140-preview, Win 10.0.26300, en-US, 3 rows incl. `< ` version) parses correctly. Choco 2.7.5: real capture is the empty case only; non-empty real output Unverified. Scoop: not installed on the test machine, so real output Unverified. Non-English WinGet Unverified. WinGet hides unknown-version packages unless `--include-unknown`; Sweepr does not pass it. |
| List model, sort, filter, status text, command palette filter | **Verified** (unit tests) | |
| Theme contrast | **Verified** (computed in tests) | |
| Process execution, timeouts, tree kill | **Partially verified** | Unix path tested; Job Object path compiled only. |
| Elevation, named pipe, SDDL, journal | **Unverified** | Wire format and job validation tested; Windows pipe and UAC never run. |
| Win32 GUI (layout, DPI, tooltips, owner-draw, accelerators, dark mode) | **Unverified** | Compiles and lints on msvc target. |
| Keyboard-only walkthrough | **Unverified** | |
| CLI | **Partially verified** | Host tests with fake adapters; real managers not run. |
| CI build on windows-latest (fmt, clippy, tests, release build, portable zip, SHA256SUMS) | **Verified** (PR #1 run) | Build and tests only; the app was not launched. |
| Packaging (Setup.exe + Portable.zip + SHA256SUMS.txt) | **Unverified** | Inno Setup script and naming reworked in the release-readiness slice; the installer has never been run. No MSI is produced. |
| Embedded icon | **Unverified** | `build.rs` compiles `sweepr.rc` with rc.exe on windows-msvc builds; the exe has not been inspected on Windows yet (field pass step 12). |
| `--clean` / `--uninstall`, `schema_version` migration | **Verified** (Linux unit tests) | Data removal and the legacy `version` → `schema_version` migration are covered by tests; not yet run on Windows. |
| Headless `--update-all` | **Partially verified** | The user-queue path is covered by a host test that really runs a (fake) upgrade; the system queue reuses the GUI's elevation flow, which is Unverified until the field pass. |

## Risks

1. Parser drift: manager output changes break lists. Mitigation: parse-failed error SWP-103 shows the raw output in the log.
2. Elevation path untested on Windows. Mitigation: validate on a VM before any release.
3. WinGet scope Unknown may fail without elevation.
4. Telemetry: none by design; no network code in Sweepr itself.

## Gate before release

1. Capture real Choco output with at least one outdated package, and Scoop output (install Scoop or use a machine that has it). WinGet capture done.
2. Smoke test on Windows 10 and 11: check, update user package, update system package (one UAC), cancel, theme switch, DPI change.
3. CI green on windows-latest (done for PR #1).
4. Steps 1–2 are written up as a single sitting in [docs/field-pass.md](docs/field-pass.md), which also covers the installer, the CLI maintenance flags and the icon.
