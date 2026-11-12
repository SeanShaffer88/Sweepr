//! Sweepr's own update check: parse the GitHub release, compare versions, pick the right
//! asset, and verify its SHA-256 against the release's `SHA256SUMS.txt`. Pure logic only;
//! the HTTP transfer lives in the Windows UI crate. Integrity note: the checksum file comes
//! from the same release as the download, so it guards against corruption and mismatched
//! files, not against a compromised release. Releases are not code-signed.

use serde::Deserialize;

pub const OWNER_REPO: &str = "SeanShaffer88/Sweepr";
pub const LATEST_URL: &str = "https://api.github.com/repos/SeanShaffer88/Sweepr/releases/latest";
/// Largest installer or zip we will download.
pub const MAX_ASSET_BYTES: usize = 200 * 1024 * 1024;
pub const MAX_API_BYTES: usize = 1024 * 1024;
pub const MAX_SUMS_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
}

/// Parses the `releases/latest` JSON. Drafts and prereleases are rejected.
pub fn parse_release(json: &[u8]) -> Result<Release, String> {
    let r: ApiRelease =
        serde_json::from_slice(json).map_err(|e| format!("bad release JSON: {e}"))?;
    if r.draft || r.prerelease {
        return Err("latest release is a draft or prerelease".into());
    }
    let version = r.tag_name.trim_start_matches('v').to_string();
    parse_version(&version).ok_or_else(|| format!("unreadable version tag {:?}", r.tag_name))?;
    Ok(Release {
        version,
        assets: r
            .assets
            .into_iter()
            .map(|a| Asset {
                name: a.name,
                url: a.browser_download_url,
            })
            .collect(),
    })
}

/// `major.minor.patch` only (no pre-release suffix).
pub fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.trim().split('.');
    let p = |s: Option<&str>| s?.parse::<u64>().ok();
    let r = (p(it.next())?, p(it.next())?, p(it.next())?);
    it.next().is_none().then_some(r)
}

pub fn is_newer(current: &str, latest: &str) -> bool {
    match (parse_version(current), parse_version(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

/// Only this repository's own GitHub release downloads, over https.
pub fn url_allowed(url: &str) -> bool {
    let prefix = format!("https://github.com/{OWNER_REPO}/releases/download/");
    url.starts_with(&prefix)
        && !url.contains(['\\', ' ', '\n', '\r', '"'])
        && !url[prefix.len()..].contains("..")
}

/// What to download for a release: the installer or the portable zip, plus the checksum list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub version: String,
    pub asset: Asset,
    pub sums: Asset,
}

pub fn plan(rel: &Release, portable: bool) -> Result<Plan, String> {
    let want = if portable {
        format!("Sweepr-v{}-x64-Portable.zip", rel.version)
    } else {
        format!("Sweepr-v{}-x64-Setup.exe", rel.version)
    };
    let find = |name: &str| {
        rel.assets
            .iter()
            .find(|a| a.name == name)
            .cloned()
            .ok_or_else(|| format!("release has no {name}"))
    };
    let (asset, sums) = (find(&want)?, find("SHA256SUMS.txt")?);
    for a in [&asset, &sums] {
        if !url_allowed(&a.url) {
            return Err(format!("refusing download URL for {}", a.name));
        }
    }
    Ok(Plan {
        version: rel.version.clone(),
        asset,
        sums,
    })
}

/// Finds the hash for `name` in `sha256sum` style text (`<hex>  <name>` or `<hex> *<name>`).
pub fn expected_hash(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (h, rest) = l.trim().split_once(char::is_whitespace)?;
        let n = rest.trim().trim_start_matches('*');
        (n == name && h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| h.to_ascii_lowercase())
    })
}

/// Checks `data` against the release's checksum list.
pub fn verify(data: &[u8], sums: &str, name: &str) -> Result<(), String> {
    let want = expected_hash(sums, name).ok_or_else(|| format!("no checksum for {name}"))?;
    let got = hex(&sha256(data));
    if got == want {
        Ok(())
    } else {
        Err(format!("checksum mismatch for {name}"))
    }
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// SHA-256 (FIPS 180-4).
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, b) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    let mut out = [0u8; 32];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(&sha256(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        let million_a = vec![b'a'; 1_000_000];
        assert_eq!(
            hex(&sha256(&million_a)),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("0.1.0", "0.2.0"));
        assert!(is_newer("0.9.9", "0.10.0"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.3.0", "0.2.9"));
        assert!(!is_newer("0.1.0", "garbage"));
        assert!(!is_newer("0.1.0", "1.0.0-beta"));
    }

    fn json(tag: &str, extra: &str) -> String {
        format!(
            r#"{{"tag_name":"{tag}"{extra},"assets":[
            {{"name":"Sweepr-v0.2.0-x64-Setup.exe","browser_download_url":"https://github.com/SeanShaffer88/Sweepr/releases/download/v0.2.0/Sweepr-v0.2.0-x64-Setup.exe"}},
            {{"name":"Sweepr-v0.2.0-x64-Portable.zip","browser_download_url":"https://github.com/SeanShaffer88/Sweepr/releases/download/v0.2.0/Sweepr-v0.2.0-x64-Portable.zip"}},
            {{"name":"SHA256SUMS.txt","browser_download_url":"https://github.com/SeanShaffer88/Sweepr/releases/download/v0.2.0/SHA256SUMS.txt"}}]}}"#
        )
    }

    #[test]
    fn release_parse_and_plan() {
        let r = parse_release(json("v0.2.0", "").as_bytes()).unwrap();
        assert_eq!(r.version, "0.2.0");
        let p = plan(&r, false).unwrap();
        assert_eq!(p.asset.name, "Sweepr-v0.2.0-x64-Setup.exe");
        let p = plan(&r, true).unwrap();
        assert_eq!(p.asset.name, "Sweepr-v0.2.0-x64-Portable.zip");
        assert_eq!(p.sums.name, "SHA256SUMS.txt");
    }

    #[test]
    fn drafts_prereleases_and_bad_tags_rejected() {
        assert!(parse_release(json("v0.2.0", r#","draft":true"#).as_bytes()).is_err());
        assert!(parse_release(json("v0.2.0", r#","prerelease":true"#).as_bytes()).is_err());
        assert!(parse_release(json("nightly", "").as_bytes()).is_err());
        assert!(parse_release(b"{").is_err());
    }

    #[test]
    fn plan_refuses_foreign_or_missing_assets() {
        let mut r = parse_release(json("v0.2.0", "").as_bytes()).unwrap();
        r.assets[0].url = "https://evil.example/Sweepr-v0.2.0-x64-Setup.exe".into();
        assert!(plan(&r, false).unwrap_err().contains("refusing"));
        r.assets.retain(|a| a.name != "SHA256SUMS.txt");
        assert!(plan(&r, true).unwrap_err().contains("SHA256SUMS"));
    }

    #[test]
    fn url_allowlist_is_strict() {
        let ok = "https://github.com/SeanShaffer88/Sweepr/releases/download/v1.0.0/a.exe";
        assert!(url_allowed(ok));
        assert!(!url_allowed(&ok.replace("https", "http")));
        assert!(!url_allowed(
            "https://github.com/Other/Sweepr/releases/download/v1/a.exe"
        ));
        assert!(!url_allowed(
            "https://github.com.evil.example/SeanShaffer88/Sweepr/releases/download/v1/a"
        ));
        assert!(!url_allowed(&format!("{ok}/../../x")));
        assert!(!url_allowed(&format!("{ok}\" & calc")));
    }

    #[test]
    fn checksums_parse_and_verify() {
        let data = b"hello";
        let h = hex(&sha256(data));
        let sums = format!("{h}  Sweepr-a.exe\n{} *Sweepr-b.zip\n", "0".repeat(64));
        assert_eq!(expected_hash(&sums, "Sweepr-a.exe"), Some(h.clone()));
        assert!(verify(data, &sums, "Sweepr-a.exe").is_ok());
        assert!(verify(b"hellp", &sums, "Sweepr-a.exe")
            .unwrap_err()
            .contains("mismatch"));
        assert!(verify(data, &sums, "missing")
            .unwrap_err()
            .contains("no checksum"));
        assert!(expected_hash("zz  Sweepr-a.exe", "Sweepr-a.exe").is_none());
    }
}
