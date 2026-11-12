# Field pass — Sweepr v0.1.0 (run on a real Windows machine)

Everything below is **Unverified** until it has been run here. Work top to
bottom; note pass / fail / not-run per numbered step.

## 1. Capture the missing real fixtures (~10 minutes)

1. Install Scoop if it is missing (https://scoop.sh), then install one small
   app with it, e.g. `scoop install git`.
2. Make one Chocolatey package outdated: install an older version of
   something small, e.g. `choco install 7zip --version 24.9 -y`, then wait
   for a newer 7-Zip to exist (or pick any package with a pending update).
3. From the repo folder, run:
   `powershell -ExecutionPolicy Bypass -File .\tools\capture-fixtures.ps1`
4. Confirm `captured\` now holds non-empty `choco_outdated` output (at least
   one data row) and a real `scoop status` table, then hand the `captured`
   folder back so the fixtures can be committed as `*.real.txt`.

## 2. Smoke test — portable build

Use `Sweepr-v0.1.0-x64-Portable.zip` unpacked anywhere.

5. Launch `sweepr.exe`. Window opens, first check runs, outdated packages
   from every installed manager appear in one list.
6. Update one user-scope package (Scoop). It completes; no UAC prompt.
7. Select a mixed batch (one Scoop + one Chocolatey package) and update.
   **Exactly one** UAC prompt appears; both complete.
8. Start a check and press Esc mid-run: it cancels promptly.
9. Switch Windows between light and dark: Sweepr follows (theme = System).
10. Move the PC to a different-DPI display (or change scaling 100% → 150%):
    layout stays correct, nothing overlaps.
11. Close and relaunch: window size/position are where you left them.
12. Check the title-bar and taskbar icon: the crimson broom icon shows.

12b. TesselGUI r2 look (new, never run): GitHub dark and light both read
    cleanly; list has edge-colour row and column gridlines, a 3 px bar on
    the highlighted row, and an own-drawn checkbox that toggles on click and
    on Space; Update selected is green and brightens on hover; panel
    headers show a 2 px blue rule on the focused region; F6 cycles regions;
    Ctrl+D hides and shows Details and the choice survives a relaunch;
    hint bar items are clickable; progress shows ten segments during a check;
    Ctrl+K shows recent commands first and says why a command is unavailable;
    tooltips wait about half a second; Reset layout restores the first-run
    window; turn on Windows high contrast and the main window follows it.

## 3. CLI

13. `sweepr.exe --headless` lists the same outdated set as the window.
14. `sweepr.exe --headless --json` prints valid JSON.
14b. `sweepr.exe --update-all --dry-run` prints commands and changes nothing;
    then `sweepr.exe --update-all` updates the lot with at most one UAC prompt.
15. `sweepr.exe --clean` removes the `data` folder contents next to the exe
    and reports what it removed; relaunch shows default settings.
16. `sweepr.exe --uninstall` removes the `data` folder itself.

## 4. Installer

17. Run `Sweepr-v0.1.0-x64-Setup.exe`: installs per-user with no admin
    prompt, Start menu entry appears, app launches from it.
18. Uninstall from Settings > Apps: program files are gone; settings in
    `%LOCALAPPDATA%\Sweepr` remain (by design — `--uninstall` removes them).

## 5. Screenshots owed (README list)

19. Capture the six shots listed in `README.md` > "Screenshots to capture".

Any failure: save the log drawer contents (Copy / Save in the app) with the
step number and send both back.
