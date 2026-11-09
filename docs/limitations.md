# Limitations (v0.1.0)

- WinGet parser is verified against a real capture; Chocolatey's real capture covers the empty case only, and Scoop has no real capture yet. Other real output may differ.
- WinGet table headers are localized; non-English Windows is Unverified.
- Headless `--update-all` not implemented.
- No transfer speed or byte counts; status shows per-package state.
- ListView gridlines are not dark-themed.
- WinGet packages with unknown scope run in the user queue.
- Windows GUI and elevation were never run during development (Linux sandbox). They are cross-compiled only.
- The Setup.exe installer script (Inno Setup) is built in CI but has never been run on a real machine; see docs/field-pass.md.
