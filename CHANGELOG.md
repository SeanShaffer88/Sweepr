# Changelog

## [Unreleased]

### Added (next slice)
- Update history: each update is recorded locally (`history.jsonl`, capped), shown with Ctrl+H or `--history [--json]`, with copy-and-run undo hints per manager. Hints are text only.
- Optional daily check through Task Scheduler (`--scheduled-check`): checks only, remembers the result, and asks before opening the window when the outdated set is new or changed.
- Sweepr self-update (on by default, switch off in Settings): WinHTTP check of this repository's latest release at most daily, SHA-256 verification against `SHA256SUMS.txt`, install only after you say yes. Manual command: Check for Sweepr updates. First network code in Sweepr itself; no new dependencies.
- Clickable action on the empty list (Check again, Open settings or Show the log).
- Localized WinGet parsing: header found by position, columns from the table layout, localized "nothing to upgrade" messages read as empty.

### Changed
- Settings and Commands dialogs now follow Windows high contrast.
- Settings dialog is larger (schedule and update options).
- Filter box text and placeholder are centered vertically (fix for the clipped placeholder; Unverified on screen).

### Changed (TesselGUI revision 2 conformance)
- Palette is now the TesselGUI GitHub preset (the r2 default); crimson is no longer used in the UI. Light and dark variants; Windows high contrast falls back to system colors.
- Layout follows Compact density: 32 px controls, 8 px gaps, 28 px rows and panel headers, 12 px dense / 13 px label type, a 2 px active-region rule, 40 px controls in dialogs.
- List draws its own rows: edge-colour row and column gridlines (fixes light gridlines on dark), 3 px selection bar with `selected` fill, own checkbox. Header stays fixed.
- The one primary action (Update selected) is the green `action` color with hover, pressed and focus states.
- Status bar leads with an icon and a word (Info, Done, Warning, Error) as well as color; failures say what, why and what to do, with the code last.
- Ten-segment determinate progress with "N of M" text for checks and updates.
- Command palette: recent first, fuzzy match, 8 rows, shortcut shown, unavailable commands stay listed and say why.
- Tooltips appear after 500 ms.

### Added
- Details panel (Ctrl+D) for the highlighted package; its open state is remembered.
- Hint bar with up to six clickable hints for the focused region.
- Shortcuts Ctrl+F (find), Ctrl+A (select all), Ctrl+S (save log), Ctrl+D (details), F1 (help), F6 (next region).
- Reset layout command.
- Empty states on a 24 px grid motif.
- Minimum window width is now 1040 px (logical); first-run width 1120.

## [0.1.0] - 2026-11-08

### Added
- Core library: models, error codes SWP-101..401, id validator, WinGet/Choco/Scoop parsers, settings with atomic write, ignore and pin rules.
- Process execution with timeouts, cancel tokens and job-object tree kill.
- Discovery orchestrator running managers in parallel.
- One-UAC elevation: self-relaunch with a hardened named pipe and a validated job file; journal for interrupted jobs.
- Win32 GUI (Common Controls v6): virtual list, sort, filter, source toggles, row menu (ignore, pin, update only this, copy id), log drawer, settings, command palette, tooltips, accelerators, dark/light theme, DPI handling, geometry persistence.
- CLI: headless list, JSON, dry-run.
- Docs, fixture-capture script, CI workflow, packaging scripts.

### Added (headless update)
- CLI `--update-all`: updates every outdated, non-ignored package from the command line, reusing the window's queue split and one-UAC elevation flow. `--dry-run` prints the commands and runs nothing; `--json` reports per-package results.

### Added (release-readiness)
- Embedded application icon (crimson broom on charcoal), compiled from `sweepr.rc` by `build.rs` with the SDK resource compiler — no new dependencies.
- CLI `--clean` (delete stored data, keep the program) and `--uninstall` (delete all stored data and its folder).
- Settings key renamed `version` → `schema_version` with a tested upward migration; older files still load.
- Field-pass checklist (docs/field-pass.md) covering the missing real Choco/Scoop captures, the Windows smoke test, the CLI and the installer.

### Changed
- Packaging now produces exactly `Sweepr-v<version>-x64-Setup.exe` (Inno Setup, per-user) + `Sweepr-v<version>-x64-Portable.zip` (now includes `portable.flag`) + `SHA256SUMS.txt`. The WiX MSI was removed.
- Project classified tier t1b under the Adaptive Engineering Contract V4.0 (`.project-tier`, Project Manifest in AGENTS.md).

### Known gaps
See [AUDIT.md](AUDIT.md) and [docs/limitations.md](docs/limitations.md). Released ahead of the real-machine field pass at the owner's direction; the GUI and elevation path remain Unverified until [docs/field-pass.md](docs/field-pass.md) has been run.

### Tests
- Real WinGet and Choco (empty) captures added as fixtures with parser tests.
