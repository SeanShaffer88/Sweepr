# Sweepr

## TL;DR

One window listing every outdated WinGet, Chocolatey and Scoop package; pick some or all and update them with at most one UAC prompt. Native Rust/Win32, no telemetry, no accounts, no background service. Free, MIT.

One window. Every outdated app. Update them all with at most one UAC prompt.

Sweepr is a small native Windows updater over **WinGet**, **Chocolatey** and **Scoop**. It lists what is outdated, lets you pick, and updates. It works offline apart from what the managers themselves do. No telemetry. No accounts. No background service.

> Status: **v0.1.0 in development, not released.** The parser for WinGet is verified against a real capture; the Windows GUI and the elevation path have been cross-compiled only and never run. See [AUDIT.md](AUDIT.md) and the [field pass](docs/field-pass.md).

## Features

- Lists outdated packages from all three managers in one sortable, filterable list.
- Select some or all, then update. User-scope and system-scope packages are split into two jobs. System jobs run in one elevated helper: one UAC prompt for the whole batch.
- Ignore a package, or pin it (skip one specific version). Both persist in settings.
- Log drawer with copy and save. Saved logs have your profile path redacted.
- Command palette (Ctrl+K) and keyboard shortcuts.
- Dark and light theme, following Windows or set by hand. Per-monitor DPI. Window size and position remembered.
- CLI mode for scripting: `--headless`, `--json`, `--dry-run`.

## Shortcuts

| Key | Action |
|---|---|
| F5 | Check for updates |
| Ctrl+U | Update selected |
| Ctrl+Shift+U | Update all |
| Ctrl+L | Toggle log drawer |
| Ctrl+, | Settings |
| Ctrl+I | Show / hide ignored |
| Ctrl+K | Command palette |
| Space / Enter | Check or uncheck the row |
| Esc | Cancel running check or update |

## CLI

```
sweepr.exe --headless            list outdated packages as text
sweepr.exe --headless --json     list as JSON
sweepr.exe --headless --dry-run  show what would run, run nothing
sweepr.exe --update-all          update every outdated package (one UAC prompt for system packages)
sweepr.exe --update-all --json   update all, report results as JSON
sweepr.exe --clean               delete Sweepr's stored data (settings, logs, jobs)
sweepr.exe --uninstall           delete all stored data and its data folder
```

Other flags: `--silent --verbose --debug --trace --help --version`. Exit codes: 0 ok, 1 error, 2 bad arguments. `--update-all --dry-run` prints the commands and runs nothing.

## Download

Releases (once published) carry exactly three files: `Sweepr-v<version>-x64-Setup.exe` (per-user installer, no admin rights needed), `Sweepr-v<version>-x64-Portable.zip` (unzip and run; writes only to the `data` folder beside the exe), and `SHA256SUMS.txt`. Binaries are unsigned.

## Requirements

- Windows 10 or 11, x64.
- At least one of WinGet, Chocolatey, Scoop installed and on PATH. Missing managers are shown as "not installed", not as errors.

## Build

```
cargo build --release
cargo test --locked
```

Static CRT. Dependencies: `windows-sys`, `serde`, `serde_json`. Nothing else.

## Safety

- Manager commands are built from fixed templates and run as argument vectors, never through a shell.
- Package ids must match `A-Za-z0-9._+-`, 1 to 128 characters. Anything else is dropped.
- The elevated helper receives only `{manager, id, scope}`, validates it again, and builds the commands itself.
- The helper pipe is owner/Administrators/SYSTEM only, rejects remote clients, and accepts the first instance only.

## Error codes

SWP-101 manager missing, 102 manager timed out, 103 parse failed, 201 UAC declined, 202 elevated pipe lost, 203 job rejected (or launch failed), 301 settings corrupt, 401 installer failed.

## Screenshots to capture (not yet taken)

1. Main window, light theme, with results from all three managers.
2. Main window, dark theme.
3. Log drawer open during an update.
4. Settings dialog.
5. Command palette.
6. Empty state ("everything is up to date").

## License

MIT. See [LICENSE](LICENSE). Credits: [CREDITS.md](CREDITS.md).
