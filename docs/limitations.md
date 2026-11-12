# Limitations (v0.1.0)

- WinGet parser is verified against a real capture; Chocolatey's real capture covers the empty case only, and Scoop has no real capture yet. Other real output may differ.
- WinGet table headers are localized; non-English Windows is Unverified.
- No transfer speed or byte counts; status shows per-package state.
- Dialogs (Settings, Commands) use the preset palette and do not follow Windows high contrast; the main window does.
- Undo/Redo shortcuts apply only to the filter text box (native edit behavior); Sweepr has no document to undo.
- Empty states show the shortcut in text; there is no clickable primary button on them.
- WinGet packages with unknown scope run in the user queue.
- Windows GUI and elevation were never run during development (Linux sandbox). They are cross-compiled only.
- The Setup.exe installer script (Inno Setup) is built in CI but has never been run on a real machine; see docs/field-pass.md.
