//! Sweepr's own update: fetch, verify, then install on the user's say-so. Windows only;
//! the logic it leans on (release parsing, checksum, URL allowlist) is unit-tested in
//! `sweepr_core::update`. Runtime behavior is Unverified until run on a real machine.

use super::net;
use std::path::{Path, PathBuf};
use sweepr_core::update::{
    is_newer, parse_release, plan, verify, LATEST_URL, MAX_API_BYTES, MAX_ASSET_BYTES,
    MAX_SUMS_BYTES,
};

/// A verified download waiting to be installed.
#[derive(Debug, Clone)]
pub struct Ready {
    pub version: String,
    pub file: PathBuf,
    pub portable: bool,
}

/// Looks for a newer release and downloads and verifies it. `Ok(None)` means up to date.
pub fn check_and_fetch(data_dir: &Path, portable: bool) -> Result<Option<Ready>, String> {
    let current = env!("CARGO_PKG_VERSION");
    let api = net::get(LATEST_URL, MAX_API_BYTES, "application/vnd.github+json")?;
    let rel = parse_release(&api)?;
    if !is_newer(current, &rel.version) {
        return Ok(None);
    }
    let p = plan(&rel, portable)?;
    let sums = net::get(&p.sums.url, MAX_SUMS_BYTES, "application/octet-stream")?;
    let sums = String::from_utf8(sums).map_err(|_| "checksum list is not text".to_string())?;
    let data = net::get(&p.asset.url, MAX_ASSET_BYTES, "application/octet-stream")?;
    verify(&data, &sums, &p.asset.name)?;
    let dir = data_dir.join("updates");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    // The name comes from our own plan, not from the server body; keep it flat anyway.
    let file = dir.join(&p.asset.name);
    std::fs::write(&file, &data).map_err(|e| format!("could not save the download: {e}"))?;
    Ok(Some(Ready {
        version: p.version,
        file,
        portable,
    }))
}

/// Starts the install. The caller closes the window right after `Ok`.
/// Installed copies run the verified Setup.exe silently (it closes and restarts Sweepr).
/// Portable copies unpack the zip and swap the exe; the old one is kept as `.old` until the
/// next start.
pub fn install(ready: &Ready, exe: &Path) -> Result<(), String> {
    if !ready.portable {
        std::process::Command::new(&ready.file)
            .args([
                "/SILENT",
                "/SUPPRESSMSGBOXES",
                "/NORESTART",
                "/CLOSEAPPLICATIONS",
                "/RESTARTAPPLICATIONS",
            ])
            .spawn()
            .map_err(|e| format!("could not start the installer: {e}"))?;
        return Ok(());
    }
    let out = ready
        .file
        .with_file_name(format!("unpacked-{}", ready.version));
    let (zip, dest) = (ready.file.display().to_string(), out.display().to_string());
    if zip.contains('\'') || dest.contains('\'') {
        return Err(
            "this folder name has an apostrophe; update by hand from the release page".into(),
        );
    }
    let _ = std::fs::remove_dir_all(&out);
    let cmd = format!("Expand-Archive -LiteralPath '{zip}' -DestinationPath '{dest}' -Force");
    let st = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &cmd])
        .status()
        .map_err(|e| format!("could not unpack: {e}"))?;
    if !st.success() {
        return Err("unpacking the zip failed".into());
    }
    let new_exe = out.join("sweepr.exe");
    if !new_exe.is_file() {
        return Err("the zip has no sweepr.exe".into());
    }
    let old = exe.with_extension("exe.old");
    let _ = std::fs::remove_file(&old);
    // A running exe can be renamed, not overwritten.
    std::fs::rename(exe, &old).map_err(|e| format!("could not move the old exe: {e}"))?;
    if let Err(e) = std::fs::copy(&new_exe, exe) {
        let _ = std::fs::rename(&old, exe);
        return Err(format!("could not place the new exe: {e}"));
    }
    std::process::Command::new(exe)
        .spawn()
        .map_err(|e| format!("updated, but could not restart: {e}"))?;
    Ok(())
}

/// Removes leftovers from a finished update. Safe to call on every start.
pub fn cleanup(data_dir: &Path, exe: &Path) {
    let _ = std::fs::remove_file(exe.with_extension("exe.old"));
    let _ = std::fs::remove_dir_all(data_dir.join("updates"));
}
