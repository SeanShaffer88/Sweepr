# Limitations (v0.1.0)

- Parsers tested on synthetic fixtures only. Real output may differ.
- WinGet table headers are localized; non-English Windows is Unverified.
- Headless `--update-all` not implemented.
- No transfer speed or byte counts; status shows per-package state.
- No embedded application icon.
- ListView gridlines are not dark-themed.
- WinGet packages with unknown scope run in the user queue.
- Windows GUI and elevation were never run during development (Linux sandbox). They are cross-compiled only.
- Installer: portable zip and MSI scripts only. No Setup exe yet.
