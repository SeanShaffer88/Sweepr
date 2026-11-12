# Limitations (v0.1.0)

- WinGet parser is verified against a real capture; Chocolatey's real capture covers the empty case only, and Scoop has no real capture yet. Other real output may differ.
- WinGet table headers are localized; the parser reads the layout, but only synthetic non-English fixtures exist. A header with several words per column that cannot be told apart is rejected (SWP-103) rather than guessed.
- A localized "nothing to upgrade" message is recognized by shape (a few short lines, no columns), not by wording.
- Self-update trusts the release it downloads from: the checksum file comes from the same release, and releases are not code-signed. Portable installs unpack with PowerShell and cannot self-update from a folder whose name has an apostrophe.
- Self-update and the daily check were never run on Windows.
- Undo hints are suggestions: downgrades are not supported by every package, and Sweepr never runs them.
- No transfer speed or byte counts; status shows per-package state.
- Undo/Redo shortcuts apply only to the filter text box (native edit behavior); Sweepr has no document to undo.
- WinGet packages with unknown scope run in the user queue.
- Windows GUI and elevation were never run during development (Linux sandbox). They are cross-compiled only.
- The Setup.exe installer script (Inno Setup) is built in CI but has never been run on a real machine; see docs/field-pass.md.
