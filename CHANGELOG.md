# Changelog

## [Unreleased] - 0.1.0

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
See [AUDIT.md](AUDIT.md) and [docs/limitations.md](docs/limitations.md). No release until real fixtures are captured and a Windows smoke test is done.

### Tests
- Real WinGet and Choco (empty) captures added as fixtures with parser tests.
