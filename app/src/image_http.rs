//! Bounded Windows HTTP image loading for GPUI's existing image cache.
use gpui::{
    BackgroundExecutor,
    http_client::{self, AsyncBody, HttpClient, Response},
};
use std::{
    ffi::c_void,
    ptr,
    time::{Duration, Instant},
};
use windows_sys::Win32::Networking::WinHttp::*;
pub struct Client(pub BackgroundExecutor);
impl HttpClient for Client {
    fn user_agent(&self) -> Option<&http_client::http::HeaderValue> {
        None
    }
    fn proxy(&self) -> Option<&url::Url> {
        None
    }
    fn send(
        &self,
        request: http_client::Request<AsyncBody>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<Response<AsyncBody>>> + Send>,
    > {
        let executor = self.0.clone();
        Box::pin(async move {
            anyhow::ensure!(
                request.method() == http_client::Method::GET,
                "Image requests must use GET"
            );
            let location = request.uri().to_string();
            let (status, bytes) = executor
                .spawn(async move { download(&location, 16 * 1024 * 1024) })
                .await
                .map_err(anyhow::Error::msg)?;
            Ok(Response::builder()
                .status(status)
                .body(AsyncBody::from(bytes))?)
        })
    }
}
struct Handle(*mut c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            WinHttpCloseHandle(self.0);
        }
    }
}
fn handle(raw: *mut c_void) -> Result<Handle, String> {
    if raw.is_null() {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(Handle(raw))
    }
}
fn check(value: i32) -> Result<(), String> {
    if value == 0 {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
pub fn download(location: &str, limit: usize) -> Result<(u16, Vec<u8>), String> {
    let mut url = url::Url::parse(location).map_err(|e| e.to_string())?;
    let session = handle(unsafe {
        WinHttpOpen(
            wide("Seek Markdown images").as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            ptr::null(),
            ptr::null(),
            0,
        )
    })?;
    check(unsafe { WinHttpSetTimeouts(session.0, 5000, 5000, 5000, 10000) })?;
    let never = WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
    check(unsafe {
        WinHttpSetOption(
            session.0,
            WINHTTP_OPTION_REDIRECT_POLICY,
            ptr::from_ref(&never).cast(),
            4,
        )
    })?;
    let began = Instant::now();
    for _ in 0..6 {
        if began.elapsed() > Duration::from_secs(30) {
            return Err("Image request timed out".into());
        }
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err("Unsupported image URL".into());
        }
        let host = url.host_str().ok_or("Missing image host")?;
        let port = url.port_or_known_default().ok_or("Missing image port")?;
        let connection =
            handle(unsafe { WinHttpConnect(session.0, wide(host).as_ptr(), port, 0) })?;
        let path = format!(
            "{}{}",
            url.path(),
            url.query().map_or(String::new(), |q| format!("?{q}"))
        );
        let request = handle(unsafe {
            WinHttpOpenRequest(
                connection.0,
                wide("GET").as_ptr(),
                wide(&path).as_ptr(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                if url.scheme() == "https" {
                    WINHTTP_FLAG_SECURE
                } else {
                    0
                },
            )
        })?;
        check(unsafe { WinHttpSendRequest(request.0, ptr::null(), 0, ptr::null(), 0, 0, 0) })?;
        check(unsafe { WinHttpReceiveResponse(request.0, ptr::null_mut()) })?;
        let mut status = 0u32;
        let mut length = 4;
        check(unsafe {
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                ptr::null(),
                ptr::from_mut(&mut status).cast(),
                &mut length,
                ptr::null_mut(),
            )
        })?;
        if [301, 302, 303, 307, 308].contains(&status) {
            let mut target = vec![0u16; 8192];
            let mut length = (target.len() * 2) as u32;
            check(unsafe {
                WinHttpQueryHeaders(
                    request.0,
                    WINHTTP_QUERY_LOCATION,
                    ptr::null(),
                    target.as_mut_ptr().cast(),
                    &mut length,
                    ptr::null_mut(),
                )
            })?;
            let target =
                String::from_utf16(&target[..length as usize / 2]).map_err(|e| e.to_string())?;
            url = url
                .join(target.trim_end_matches('\0'))
                .map_err(|e| e.to_string())?;
            continue;
        }
        if status != 200 {
            return Ok((status as u16, vec![]));
        }
        let mut bytes = vec![];
        let mut buffer = [0u8; 16384];
        loop {
            if began.elapsed() > Duration::from_secs(30) {
                return Err("Image request timed out".into());
            }
            let mut read = 0;
            check(unsafe {
                WinHttpReadData(
                    request.0,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut read,
                )
            })?;
            if read == 0 {
                break;
            }
            if bytes.len() + read as usize > limit {
                return Err("Image exceeds its download limit".into());
            }
            bytes.extend_from_slice(&buffer[..read as usize]);
        }
        return Ok((200, bytes));
    }
    Err("Too many image redirects".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    #[test]
    fn local_http_images_redirect_and_enforce_download_limits() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            for index in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 2048];
                let _ = stream.read(&mut request).unwrap();
                if index == 0 {
                    stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: /image\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                } else {
                    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nimage").unwrap();
                }
            }
        });
        assert_eq!(
            download(&format!("http://{addr}/redirect"), 10).unwrap(),
            (200, b"image".to_vec())
        );
        assert!(download(&format!("http://{addr}/large"), 4).is_err());
        worker.join().unwrap();
    }
}
