use std::path::PathBuf;

fn main() {
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os == "windows" && env == "msvc" {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{dir}\\sweepr.manifest");
        embed_icon(&dir);
    }
    println!("cargo:rerun-if-changed=sweepr.manifest");
    println!("cargo:rerun-if-changed=sweepr.rc");
    println!("cargo:rerun-if-changed=sweepr.ico");
    println!("cargo:rerun-if-changed=build.rs");
}

/// Compile sweepr.rc (application icon) with the Windows SDK resource
/// compiler and link the resulting .res. No build dependencies: rc.exe is
/// located under the Windows Kits install. When rc.exe cannot be found the
/// build continues without an embedded icon and says so in the build log.
fn embed_icon(manifest_dir: &str) {
    let Some(rc) = find_rc() else {
        println!("cargo:warning=rc.exe not found; building without the embedded application icon");
        return;
    };
    let out_dir = std::env::var("OUT_DIR").unwrap_or_default();
    let res = PathBuf::from(&out_dir).join("sweepr.res");
    let ok = std::process::Command::new(&rc)
        .args(["/nologo", "/fo"])
        .arg(&res)
        .arg(format!("{manifest_dir}\\sweepr.rc"))
        .current_dir(manifest_dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        println!("cargo:rustc-link-arg-bins={}", res.display());
    } else {
        println!("cargo:warning=rc.exe failed; building without the embedded application icon");
    }
}

fn find_rc() -> Option<PathBuf> {
    for root in [
        std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into()),
        std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into()),
    ] {
        let bin = PathBuf::from(root)
            .join("Windows Kits")
            .join("10")
            .join("bin");
        let Ok(versions) = std::fs::read_dir(&bin) else {
            continue;
        };
        // Newest SDK version folder wins.
        let mut dirs: Vec<PathBuf> = versions.filter_map(|e| e.ok().map(|e| e.path())).collect();
        dirs.sort();
        for v in dirs.into_iter().rev() {
            let rc = v.join("x64").join("rc.exe");
            if rc.exists() {
                return Some(rc);
            }
        }
    }
    None
}
