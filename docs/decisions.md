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
| Headless lists only | `--update-all` deferred until a real-machine test. |
| No animation, no embedded icon | Keep v0.1.0 small; TesselGUI allows no ambient motion. |
| Neutral palette hex chosen by contrast tests | TesselGUI gives primary `#D02946` and accent `#76172A` only. |
| Log drawer is one text box with stream prefixes | Tabs deferred. |
