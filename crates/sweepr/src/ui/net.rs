//! Minimal HTTPS GET over WinHTTP (no extra crates). Certificates are validated by the
//! system; redirects follow WinHTTP's default policy, which never goes from https to http.
//! Windows only; Unverified at runtime until run on a real machine.

use windows_sys::Win32::Networking::WinHttp::*;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct Handle(*mut core::ffi::c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

/// GETs `url` (https only) and returns the body, failing past `max` bytes.
pub fn get(url: &str, max: usize, accept: &str) -> Result<Vec<u8>, String> {
    if !url.starts_with("https://") {
        return Err("only https URLs are allowed".into());
    }
    unsafe {
        let wurl = wide(url);
        let mut host = [0u16; 256];
        let mut path = [0u16; 2048];
        let mut uc: URL_COMPONENTS = std::mem::zeroed();
        uc.dwStructSize = std::mem::size_of::<URL_COMPONENTS>() as u32;
        uc.lpszHostName = host.as_mut_ptr();
        uc.dwHostNameLength = host.len() as u32;
        uc.lpszUrlPath = path.as_mut_ptr();
        uc.dwUrlPathLength = path.len() as u32;
        // Query strings (GitHub CDN links carry one) are part of the path to send.
        let mut extra = [0u16; 1536];
        uc.lpszExtraInfo = extra.as_mut_ptr();
        uc.dwExtraInfoLength = extra.len() as u32;
        if WinHttpCrackUrl(wurl.as_ptr(), 0, 0, &mut uc) == 0 {
            return Err("could not read the URL".into());
        }
        let ua = wide(&format!("Sweepr/{}", env!("CARGO_PKG_VERSION")));
        let session = Handle(WinHttpOpen(
            ua.as_ptr(),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            std::ptr::null(),
            std::ptr::null(),
            0,
        ));
        if session.0.is_null() {
            return Err("could not start WinHTTP".into());
        }
        WinHttpSetTimeouts(session.0, 10_000, 10_000, 30_000, 60_000);
        let conn = Handle(WinHttpConnect(session.0, host.as_ptr(), uc.nPort, 0));
        if conn.0.is_null() {
            return Err("could not connect".into());
        }
        let mut full: Vec<u16> = path[..uc.dwUrlPathLength as usize].to_vec();
        full.extend_from_slice(&extra[..uc.dwExtraInfoLength as usize]);
        full.push(0);
        let req = Handle(WinHttpOpenRequest(
            conn.0,
            wide("GET").as_ptr(),
            full.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ));
        if req.0.is_null() {
            return Err("could not open the request".into());
        }
        let hdr = wide(&format!("Accept: {accept}"));
        WinHttpAddRequestHeaders(req.0, hdr.as_ptr(), u32::MAX, WINHTTP_ADDREQ_FLAG_ADD);
        if WinHttpSendRequest(req.0, std::ptr::null(), 0, std::ptr::null(), 0, 0, 0) == 0
            || WinHttpReceiveResponse(req.0, std::ptr::null_mut()) == 0
        {
            return Err("request failed (offline, blocked, or a certificate problem)".into());
        }
        let mut status: u32 = 0;
        let mut len = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            std::ptr::null(),
            &mut status as *mut u32 as *mut _,
            &mut len,
            std::ptr::null_mut(),
        );
        if status != 200 {
            return Err(format!("server answered {status}"));
        }
        let mut body = Vec::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let mut got: u32 = 0;
            if WinHttpReadData(
                req.0,
                buf.as_mut_ptr() as *mut _,
                buf.len() as u32,
                &mut got,
            ) == 0
            {
                return Err("download was interrupted".into());
            }
            if got == 0 {
                break;
            }
            body.extend_from_slice(&buf[..got as usize]);
            if body.len() > max {
                return Err("download is larger than allowed".into());
            }
        }
        Ok(body)
    }
}
