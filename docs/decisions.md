# Decisions

| Decision | Reason |
|---|---|
| Rust + pure Win32, no GUI framework | Small static exe, no runtime, few deps. |
| Core crate has no UI | Logic is unit-tested on any host. |
| Threads + channels, no async | Work is a few child processes. |
| Fixed command templates, argument vectors | No shell injection surface. |
| Id charset `A-Za-z0-9._+-` | Blocks option-like and shell-special ids. |
| WinGet table parsed by header offsets (CJK-aware) | `winget upgrade` has no JSON output. |
| One UAC: relaunch self with `runas` + named pipe | One prompt for all system packages. |
| Pipe DACL owner + Administrators + SYSTEM, reject remote, first instance | Stops other users and remote hosts joining the pipe. |
| Job file holds only `{manager, id, scope}` | Helper never runs caller-supplied command lines. |
| WinGet scope Unknown goes to the user queue | Avoids needless UAC; may fail and be retried manually. |
| Headless `--update-all` reuses the window's job flow | Same queue split, `execute_job` and one-UAC elevated helper as the GUI, so the CLI and the window can never disagree about what runs. Shipped Unverified on real machines, like the GUI flow it mirrors. |
| No animation | Keep v0.1.0 small; TesselGUI allows no ambient motion. |
| Embedded icon via `rc.exe` from `build.rs` | An icon was added in the release-readiness slice; compiling `sweepr.rc` with the SDK resource compiler keeps the dependency list at three crates (no `winresource` build dependency). |
| Inno Setup installer; no MSI | Release shape is Setup.exe + Portable.zip + SHA256SUMS.txt only. The earlier WiX MSI was removed. |
| Settings key `schema_version` | Contract V4.0 §2.3 naming; the legacy `version` key still loads and migrates on save. |
| Tier t1b | Stock Win32 GUI utility: no rich renderer, service, driver, shell extension or autostart. |
| Neutral palette hex chosen by contrast tests | TesselGUI gives primary `#D02946` and accent `#76172A` only. |
| Log drawer is one text box with stream prefixes | Tabs deferred. |
