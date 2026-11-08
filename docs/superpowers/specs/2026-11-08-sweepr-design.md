# Sweepr — Design Spec

Date: 2026-11-08 · Status: draft for review · Source: `multi_manager_software_updater_blueprint.md` + dev charter (AGENTS.md)

## 1. Purpose and success criteria

**Purpose.** One small, fast, offline-capable Windows tool that finds outdated software across WinGet, Chocolatey and Scoop, shows it in one list, and updates the selected items with at most one UAC prompt.

**Users.** Sean first; any Windows 10/11 user who has one or more of the three managers.

**Success criteria (v0.1.0).**
- Cold start to first window render < 50 ms. Idle CPU ~0%, idle working set < 15 MB. No polling timers.
- Discovery runs the three managers in parallel; the UI never blocks; results stream in.
- A mixed selection (user + machine scope) triggers exactly one UAC prompt.
- Every failure shows a stable code plus the raw exit code and output.
- Zero telemetry, zero network calls made by Sweepr itself (the managers do their own).
- Window size/position/splitters persist and validate against `EnumDisplayMonitors`.

**Non-goals.** Installing or uninstalling packages, installing the managers themselves, search/browse of repositories, scheduler, tray, auto-update, other managers (Store, pip, npm), non-Windows.

**Assumptions (Sean delegated: "you decide everything").**
- Rust, pure Win32 + Common Controls v6, static CRT, TesselGUI skin.
- Min OS: Windows 10 1809 (WinGet floor). Windows 11 supported. Both marked Unverified until run on real machines.
- x64 first; x86 portable optional later.
- License MIT; public repo `SeanShaffer88/Sweepr`; name Sweepr.

## 2. Architecture

Cargo workspace, two crates, no async runtime (threads + channels).

- `sweepr-core` (lib, no UI, fully testable): model, adapters, parsers, orchestrator, rules store, job/journal, elevation protocol, error codes, logging.
- `sweepr` (bin): Win32 GUI. The same exe is the CLI: `--headless`, `--json`, `--dry-run`, `--silent`, `--verbose/--debug/--trace`, `--help`, `--elevated-run <job>`.

Dependencies kept minimal: `windows-sys`, `serde`, `serde_json`. No `toml`, no `regex` (hand-written validators).

### 2.1 Model

`UpdatePackage { id, name, current_version, available_version, manager: Winget|Choco|Scoop, scope: Machine|User|Unknown, elevation_required: bool, pinned_by_manager: bool, ignored: bool, user_pin: Option<String> }`.

Dedup key is `(manager, id)`. The same app present in two managers is shown twice, tagged by source; Sweepr does not guess cross-manager identity.

### 2.2 Adapters

Trait `PackageManager { detect() -> Detected, list_outdated(ctx) -> Result<Vec<UpdatePackage>>, upgrade_cmd(&UpdatePackage) -> Command }`.

| Manager | Discovery | Upgrade | Notes |
|---|---|---|---|
| WinGet | `winget upgrade --accept-source-agreements --disable-interactivity` | `winget upgrade --id {id} --exact --silent --accept-package-agreements --accept-source-agreements --disable-interactivity` | No JSON output exists for `upgrade`; parse the table by column offsets taken from the header row. Handle truncated names, "pinned" and "unknown version" footers. Header is localized: non-English Windows is Unverified. |
| Chocolatey | `choco outdated --limit-output` | `choco upgrade {id} -y` | Pipe format `name|current|available|pinned`. Always machine scope, always elevated. Blueprint's `--noop` is a dry run, so it is used only for `--dry-run`. |
| Scoop | `scoop status` | `scoop update {id}` | Table parse. User scope, no elevation. Global installs (`scoop update -g`) need admin and go into the elevated batch. |

Commands are spawned without a shell (argument vector, no string concatenation). Manager executables are resolved to absolute paths from fixed known locations (WinGet via the App Installer alias path, Chocolatey via `C:\ProgramData\chocolatey`, Scoop via `%USERPROFILE%\scoop\shims`) with a `PATH` fallback shown in the log. Output is decoded as UTF-8 with lossy fallback. Parsers are pure functions over text.

### 2.3 Orchestrator

- **Discovery.** One worker thread per enabled manager, low priority. Each sends `Event::Found(Vec<UpdatePackage>)` / `Event::ManagerStatus` over a channel. UI drains on a posted window message (no timer).
- **Timeout.** Per-manager, configurable, default 60 s (blueprint's 30 s is too short for a cold WinGet). On timeout: kill the process tree via JobObject, `SWP-102`, keep partial results from others. Cancel takes effect < 50 ms.
- **Rules.** Ignore list, user version pins and source toggles applied after parsing. Ignored items are hidden unless "Show ignored" is on and are excluded from "Update all".
- **Queues.** Split selection into user-space (run now, background threads) and system-space (WinGet machine scope, all Choco, Scoop global).

### 2.4 One-prompt elevation

1. If the current process is already elevated, run the system queue in-process.
2. Otherwise write a **job file** and relaunch the same exe via `ShellExecuteExW` verb `runas` with `--elevated-run <job>`. One UAC prompt for the whole batch.
3. The elevated helper streams progress and log lines back over a **named pipe** with a random GUID name and a DACL limited to the launching user's SID. It exits when the job completes or the pipe closes.
4. **Job file** lives in `jobs\` under the Sweepr data dir, user-only ACL, deleted on completion (`FILE_FLAG_DELETE_ON_CLOSE` where possible).
5. **Helper validation (security).** The helper accepts only `{manager, id, scope}` tuples. IDs must match a conservative charset (`A-Z a-z 0-9 . _ + -`), length-capped. It builds the command itself from the fixed templates above; the job file never carries command lines or paths. Any other field is rejected with `SWP-203`.
6. Decline of UAC (Win32 1223) → `SWP-201`; nothing from the system queue runs; the user queue is unaffected.
7. **Journal.** Each job writes a journal (`queued → started → done/failed`). On next launch, unfinished jobs show as "interrupted" and are never re-run without a click.

### 2.5 Persistence

`settings.json` (human-readable, versioned, atomic write: temp → `FlushFileBuffers` → `ReplaceFileW`; validated on load; unparseable file quarantined as `settings.json.bad-<ts>` and defaults loaded). Fields: `version`, `ignore[]`, `pins{id: version}`, `sources{winget,choco,scoop: bool}`, `timeout_secs`, `theme`, `window{x,y,w,h,maximized,splitter}`, `log_drawer_open`.

Data location: portable build writes only to `data\` next to the exe; installed build writes to `%LOCALAPPDATA%\Sweepr`. Selected by presence of a `portable.flag` file beside the exe. Logs: local, timestamped, levels ERROR/WARN/INFO/DEBUG, rotated, never sent anywhere.

### 2.6 Error codes

Stable codes: `SWP-101 manager-missing`, `SWP-102 timeout`, `SWP-103 parse-failed`, `SWP-201 uac-declined`, `SWP-202 elevated-pipe-lost`, `SWP-203 job-rejected`, `SWP-301 settings-corrupt`, `SWP-401 installer-failed` (carries raw exit code). Plain-language text is mapped only for exit codes verifiable from official docs (e.g. 1618 installer busy); otherwise "exit code N" + raw output. No panic dialogs; worker panics are caught at thread boundaries.

## 3. UI (TesselGUI skin)

Wireframe confirmed in chat on 2026-11-08 (approved). Summary:
- Toolbar: Check (F5), Update selected (the one crimson primary), Update all, Ctrl+K command palette. Filter box, per-manager toggles, "Show ignored".
- Main region ≈65%: virtual `LVS_OWNERDATA` list, 28 px rows, row and column gridlines, columns Select/Name/Id/Current/Available/Source/Scope. No main-window scrollbar; the list scrolls inside its own viewport.
- Row context menu: Ignore, Pin version, Update only this, Copy id.
- Bottom log drawer (Ctrl+L): stdout/stderr tabs, per-manager status chips (ok / running / timeout / not installed), Copy, Save, redacted export bundle.
- Status bar: selection count, UAC prompts needed (0 or 1), real bytes/speed per job, Cancel (Esc).
- Settings modal (no scrollbar): source toggles, timeout, theme (follows Windows light/dark live), data location, open log folder.
- Empty states: no manager installed (names the missing tool, never installs), nothing outdated, all failed.
- Accessibility: tab order, visible focus, accessible names, text accompanies every color (source/scope are text badges), high contrast honored, tooltips on every control and column header, Esc cancel / Enter submit.
- Gaps: TesselGUI `docs/typography.md` and `docs/motion.md` were not readable; type and motion rules to be applied from those files at implementation time. Static UI, no ambient motion.

## 4. Testing and verification

- Pure-function parser tests over **real captured fixtures**. Sean must run the shipped `tools/capture-fixtures.ps1` once (dumps `winget upgrade`, `choco outdated -r`, `scoop status`). Until then, parsers are labelled **Unverified**.
- Orchestrator tests with fake adapters (timeouts, partial results, cancel). Settings round-trip, migration and quarantine tests. Job-file validation tests (hostile IDs rejected). Journal replay tests.
- Gate before any publish: `cargo fmt --check`, `cargo clippy -- -D warnings`, full `cargo test --locked` (all targets; GUI tests live in the bin).
- This dev sandbox is Linux: only `cargo check --target x86_64-pc-windows-msvc`-style cross-checks are possible here. Real build and test run on GitHub Actions `windows-latest`. CI workflow edits need Sean in the browser.
- Evidence labels (Verified / Partially verified / Unverified) and a verification matrix (Win10/11, admin vs user, 100–200% scaling, light/dark, keyboard-only) live in `AUDIT.md`.

## 5. Docs and release

Repo ships `README.md` (with a screenshot capture list, no placeholders, no competitor comparisons), `CHANGELOG.md`, `DESCRIPTION.md`, `AUDIT.md`, `CREDITS.md`, `LICENSE` (MIT), decision log and known-limitations list.

Release assets per charter: `Sweepr-v{VERSION}-x64-Setup.exe`, `-x64-Setup.msi`, `-x64-Portable.zip`, `SHA256SUMS.txt`. Static CRT, no redistributables, no web runtimes.

## 6. Known risks

- WinGet table parsing is brittle across versions and locales (mitigated by header-offset parsing and fixtures).
- Same-user processes could invoke the helper protocol; impact is limited to updating validated package IDs behind a UAC prompt. Recorded in `AUDIT.md`.
- Chocolatey and Scoop output formats can change; parsers fail closed with `SWP-103` and show raw output.
