# Sweepr Audit

Evidence labels: **Verified** (run and observed) · **Partially verified** · **Unverified**.

Development ran in a Linux sandbox. Windows code was cross-checked (`cargo check`, `cargo clippy --target x86_64-pc-windows-msvc`) and never executed.

| Area | Label | Notes |
|---|---|---|
| Model, error codes, id validator, rules, settings, job validation | **Verified** (Linux unit tests, 87 tests) | |
| Parsers | **Partially verified** | WinGet: real capture (v1.30.140-preview, Win 10.0.26300, en-US, 3 rows incl. `< ` version) parses correctly, and a live run of v0.1.0 on the owner's machine (2026-11-08) listed the same 3 outdated packages. Choco 2.7.5: real capture and live run both cover the empty case only; non-empty real output Unverified. Scoop: not installed on the test machine, so real output Unverified (the live run correctly reported "not installed"). Non-English WinGet Unverified. WinGet hides unknown-version packages unless `--include-unknown`; Sweepr does not pass it. |
| List model, sort, filter, status text, command palette filter | **Verified** (unit tests) | |
| Theme contrast | **Verified** (computed in tests) | GitHub-preset tokens; ratios mirror TesselGUI's measured contract. |
| Process execution, timeouts, tree kill | **Partially verified** | Unix path tested; Job Object path compiled only. |
| Elevation, named pipe, SDDL, journal | **Unverified** | Wire format and job validation tested; Windows pipe and UAC never run. |
| Win32 GUI (layout, DPI, tooltips, owner-draw, accelerators, dark mode) | **Partially verified** | v0.1.0 portable launched on the owner's real Windows machine (2026-11-08): main window and Settings dialog render correctly in the dark theme, discovery results and status bar are correct, and the portable data folder is honored. Still Unverified: DPI changes, updates run from the window, keyboard-only walkthrough. Known cosmetic note from that capture: the filter box placeholder text is clipped. The TesselGUI r2 rewrite (panels, own-drawn list, hint bar, details panel) has not been run on Windows at all: **Unverified**. |
| Keyboard-only walkthrough | **Unverified** | |
| CLI | **Partially verified** | Host tests with fake adapters; real managers not run. |
| CI build on windows-latest (fmt, clippy, tests, release build, portable zip, SHA256SUMS) | **Verified** (PR #1 run) | Build and tests only; the app was not launched. |
| Packaging (Setup.exe + Portable.zip + SHA256SUMS.txt) | **Unverified** | Inno Setup script and naming reworked in the release-readiness slice; the installer has never been run. No MSI is produced. |
| Embedded icon | **Verified** | The shipped v0.1.0 exe carries the icon resources (read back from the release asset), and the broom icon shows in the title bar of the owner's real-machine capture (2026-11-08). |
| `--clean` / `--uninstall`, `schema_version` migration | **Verified** (Linux unit tests) | Data removal and the legacy `version` → `schema_version` migration are covered by tests; not yet run on Windows. |
| Headless `--update-all` | **Partially verified** | The user-queue path is covered by a host test that really runs a (fake) upgrade; the system queue reuses the GUI's elevation flow, which is Unverified until the field pass. |

## TesselGUI revision 2 conformance

Source: TesselGUI v1.2.0 (`tokens.json`, `materials.md`). Sweepr implements the GitHub preset only. Evidence: logic is **Verified** by unit tests; everything drawn or interactive is **Unverified** until run on Windows (cross-compiled and clippy-clean only).

| Rule | Status | Evidence |
|---|---|---|
| Palette tokens (surfaces, text, edge, action, selection, focus, status) | Conforms | `theme.rs`, contrast tests |
| Compact density: 32 px controls, 8 px gaps, 28 px rows and headers, 12/13 px type | Conforms | Constants in `theme.rs`, used by `layout`; Unverified on screen |
| One primary action per region, with hover, pressed, focus | Conforms | `draw_primary`; Unverified on screen |
| Panels with headers, 2 px active-region rule | Conforms | `paint`; Unverified on screen |
| List: edge gridlines, 3 px selection bar, fixed header | Conforms | `draw_row`; Unverified on screen |
| Details panel, remembered open state | Conforms | `present.rs` tests, settings test for old files |
| Empty states on 24 px grid with shortcuts | Partial | Text and shortcut only; no clickable primary button |
| Severity = icon + word + color; error anatomy what/why/fix, code last | Conforms | `present.rs` tests |
| Ten-segment progress with units | Conforms | `segments` tests |
| Hint bar, at most six clickable hints | Conforms | `hints` tests; click handling Unverified |
| Reserved keys (Ctrl+K, Ctrl+,, F1, Esc, F6) and required Ctrl+F/S/A | Conforms | Accelerator table; Unverified at runtime |
| Undo/Redo | Partial | Filter text box only; no document |
| Palette: fuzzy, recent first, 8 rows, shortcuts, disabled commands explain why | Conforms | `actions.rs` tests; dialog Unverified |
| Tooltips after 500 ms | Conforms | `TTM_SETDELAYTIME`; Unverified |
| Reset layout | Conforms | Command; Unverified |
| No ambient motion | Conforms | None used |
| High contrast | Partial | Main window uses system colors; dialogs do not |
| Other presets (Crimson etc.) | Not implemented | Deferred by decision |

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
