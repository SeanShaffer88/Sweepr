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

### Known gaps
See [AUDIT.md](AUDIT.md) and [docs/limitations.md](docs/limitations.md). No release until real fixtures are captured and a Windows smoke test is done.

### Tests
- Real WinGet and Choco (empty) captures added as fixtures with parser tests.
