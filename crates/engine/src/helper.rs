//! User-restricted, versioned local IPC. No helper launch or elevation here.
use crate::{
    kernel::{Cancel, Handle, Pipe},
    ntfs::Cursor,
    walk::Listing,
};
use std::{
    io::{self, Read, Write},
    sync::Arc,
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::{IO::*, Pipes::*, Services::*, Threading::*},
};
pub const VERSION: u32 = 2;
pub const MAX_FRAME: usize = 2 * 1024 * 1024;
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum Request {
    Ping {
        version: u32,
    },
    Sync {
        version: u32,
        roots: Vec<Vec<u8>>,
        cursors: Vec<Cursor>,
        scan: bool,
    },
}
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum Response {
    Pong {
        version: u32,
    },
    Chunk {
        root: Vec<u8>,
        listing: Listing,
        last: bool,
    },
    Fallback {
        root: Vec<u8>,
        reason: String,
    },
    Reset {
        root: Vec<u8>,
        reason: String,
    },
    Changes {
        paths: Vec<Vec<u8>>,
        cursor: Cursor,
        commit: bool,
    },
    Overflow {
        roots: Vec<Vec<u8>>,
        cursor: Cursor,
        reason: String,
    },
    Ready {
        cursors: Vec<Cursor>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        scan_profiles: Vec<crate::ntfs::ScanProfile>,
    },
    Error {
        message: String,
    },
    Skipped {
        paths: Vec<(Vec<u8>, u32)>,
    },
}
pub fn write<T: serde::Serialize>(stream: &mut impl Write, value: &T) -> io::Result<()> {
    let b = serde_json::to_vec(value).map_err(io::Error::other)?;
    if b.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "helper frame too large",
        ));
    }
    stream.write_all(&(b.len() as u32).to_le_bytes())?;
    stream.write_all(&b)
}
pub fn read<T: serde::de::DeserializeOwned>(stream: &mut impl Read) -> io::Result<T> {
    serde_json::from_slice(&frame(stream)?).map_err(io::Error::other)
}
fn frame(stream: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut length = [0; 4];
    stream.read_exact(&mut length)?;
    let n = u32::from_le_bytes(length) as usize;
    if n == 0 || n > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid helper frame size",
        ));
    }
    let mut b = vec![0; n];
    stream.read_exact(&mut b)?;
    Ok(b)
}
/// Bulk listings use a compact wire representation, independent of Rust
/// padding/endianness. Other control messages retain readable JSON framing.
pub fn write_response(stream: &mut impl Write, value: &Response) -> io::Result<()> {
    let Response::Chunk {
        root,
        listing,
        last,
    } = value
    else {
        return write(stream, value);
    };
    let mut b = Vec::with_capacity(21 + root.len() + listing.names.len() + listing.ents.len() * 23);
    b.extend(b"SKC2");
    for n in [
        root.len(),
        listing.id as usize,
        listing.names.len(),
        listing.ents.len(),
    ] {
        b.extend(u32::try_from(n).map_err(io::Error::other)?.to_le_bytes());
    }
    b.push(u8::from(*last));
    b.extend(root);
    b.extend(&listing.names);
    for e in &listing.ents {
        b.extend(e.name_off.to_le_bytes());
        b.extend(e.name_len.to_le_bytes());
        b.push(e.kind);
        b.extend(e.size.to_le_bytes());
        b.extend(e.mtime.to_le_bytes());
        b.extend(e.child.to_le_bytes());
    }
    if b.len() > MAX_FRAME {
        return Err(io::Error::other("helper listing frame too large"));
    }
    stream.write_all(&(b.len() as u32).to_le_bytes())?;
    stream.write_all(&b)
}
// Batch only complete frames already in the pipe buffer. A journal/control
// response ends the batch, so idle events and Ready never wait for more data.
pub(crate) fn read_response_batch(stream: &mut std::io::BufReader<impl Read>) -> Vec<Response> {
    let first = match read_response(stream) {
        Ok(response) => response,
        Err(error) => {
            return vec![Response::Error {
                message: error.to_string(),
            }];
        }
    };
    let mut bytes = match &first {
        Response::Chunk { root, listing, .. } => {
            25 + root.len() + listing.names.len() + listing.ents.len() * 23
        }
        _ => return vec![first],
    };
    let mut batch = vec![first];
    while batch.len() < 64 {
        let buffered = stream.buffer();
        let Some(header) = buffered.get(..4) else {
            break;
        };
        let size = u32::from_le_bytes(header.try_into().unwrap()) as usize;
        if size == 0
            || size > MAX_FRAME
            || size + 4 > buffered.len()
            || bytes + size + 4 > 256 * 1024
        {
            break;
        }
        let response = read_response(stream).unwrap_or_else(|e| Response::Error {
            message: e.to_string(),
        });
        let control = !matches!(response, Response::Chunk { .. });
        batch.push(response);
        bytes += size + 4;
        if control {
            break;
        }
    }
    batch
}
pub fn read_response(stream: &mut impl Read) -> io::Result<Response> {
    let b = frame(stream)?;
    if !b.starts_with(b"SKC2") {
        return serde_json::from_slice(&b).map_err(io::Error::other);
    }
    let bad = || io::Error::new(io::ErrorKind::InvalidData, "invalid helper listing frame");
    if b.len() < 21 {
        return Err(bad());
    }
    let u32_at = |p| u32::from_le_bytes(b[p..p + 4].try_into().unwrap());
    let root_len = u32_at(4) as usize;
    let id = u32_at(8);
    let names_len = u32_at(12) as usize;
    let count = u32_at(16) as usize;
    let names = 21usize.checked_add(root_len).ok_or_else(bad)?;
    let entries = names.checked_add(names_len).ok_or_else(bad)?;
    let end = entries
        .checked_add(count.checked_mul(23).ok_or_else(bad)?)
        .ok_or_else(bad)?;
    if end != b.len() || b[20] > 1 {
        return Err(bad());
    }
    let mut ents = Vec::with_capacity(count);
    for e in b[entries..].chunks_exact(23) {
        let name_off = u32::from_le_bytes(e[..4].try_into().unwrap());
        let name_len = u16::from_le_bytes(e[4..6].try_into().unwrap());
        if name_off as usize + name_len as usize > names_len {
            return Err(bad());
        }
        ents.push(crate::walk::RawEnt {
            name_off,
            name_len,
            kind: e[6],
            size: u64::from_le_bytes(e[7..15].try_into().unwrap()),
            mtime: u32::from_le_bytes(e[15..19].try_into().unwrap()),
            child: u32::from_le_bytes(e[19..23].try_into().unwrap()),
        });
    }
    Ok(Response::Chunk {
        root: b[21..names].to_vec(),
        listing: Listing {
            id,
            names: b[names..entries].to_vec(),
            ents,
        },
        last: b[20] == 1,
    })
}
pub fn service_name(sid: &str) -> String {
    format!("SeekHelper-{sid}")
}
pub fn pipe_name(sid: &str) -> String {
    format!("\\\\.\\pipe\\SeekHelper-{sid}")
}
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}
pub fn user_sid() -> io::Result<String> {
    let mut raw = std::ptr::null_mut();
    if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut raw) } == 0 {
        if unsafe { GetLastError() } != ERROR_NO_TOKEN {
            return Err(io::Error::last_os_error());
        }
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    let token = unsafe { Handle::owned(raw)? };
    let mut n = 0;
    unsafe {
        GetTokenInformation(token.raw(), TokenUser, std::ptr::null_mut(), 0, &mut n);
    }
    // Aligned storage: TOKEN_USER contains a pointer.
    let mut storage = vec![0usize; (n as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe { GetTokenInformation(token.raw(), TokenUser, storage.as_mut_ptr() as _, n, &mut n) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    let user = unsafe { &*(storage.as_ptr() as *const TOKEN_USER) };
    let mut text = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut len = 0;
    unsafe {
        while *text.add(len) != 0 {
            len += 1;
        }
    }
    let result = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) });
    unsafe {
        LocalFree(text as _);
    }
    Ok(result)
}
pub fn validate_sid(sid: &str) -> io::Result<()> {
    if !sid.starts_with("S-1-")
        || sid.len() > 184
        || !sid
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'S' || b == b'-')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid user SID",
        ));
    }
    let mut raw = std::ptr::null_mut();
    if unsafe { ConvertStringSidToSidW(wide(sid).as_ptr(), &mut raw) } == 0 {
        return Err(io::Error::last_os_error());
    }
    unsafe {
        LocalFree(raw);
    }
    Ok(())
}
pub struct Impersonation;
impl Impersonation {
    pub fn token(token: &Handle) -> io::Result<Self> {
        if unsafe { ImpersonateLoggedOnUser(token.raw()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self)
    }
    pub fn client(pipe: &Pipe, expected: &str) -> io::Result<Self> {
        if unsafe { ImpersonateNamedPipeClient(pipe.handle.raw()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let guard = Self;
        if user_sid()? != expected {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "helper client SID mismatch",
            ));
        }
        Ok(guard)
    }
}
pub fn duplicate_client_token() -> io::Result<Handle> {
    let mut raw = std::ptr::null_mut();
    if unsafe {
        OpenThreadToken(
            GetCurrentThread(),
            TOKEN_QUERY | TOKEN_DUPLICATE,
            1,
            &mut raw,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let token = unsafe { Handle::owned(raw)? };
    let mut copy = std::ptr::null_mut();
    if unsafe {
        DuplicateTokenEx(
            token.raw(),
            TOKEN_QUERY | TOKEN_IMPERSONATE,
            std::ptr::null(),
            SecurityImpersonation,
            TokenImpersonation,
            &mut copy,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    unsafe { Handle::owned(copy) }
}
impl Drop for Impersonation {
    fn drop(&mut self) {
        unsafe {
            RevertToSelf();
        }
    }
}
pub fn listen(sid: &str, name: &str, cancel: Cancel) -> io::Result<Pipe> {
    validate_sid(sid)?;
    let mut descriptor = std::ptr::null_mut();
    let sddl = wide(&format!("D:P(A;;GA;;;SY)(A;;GRGW;;;{sid})"));
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let raw = unsafe {
        CreateNamedPipeW(
            wide(name).as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            64 * 1024,
            64 * 1024,
            0,
            &security,
        )
    };
    unsafe {
        LocalFree(descriptor);
    }
    let handle = unsafe { Handle::owned(raw)? };
    let event = unsafe { Handle::owned(CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()))? };
    let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
    ov.hEvent = event.raw();
    if unsafe { ConnectNamedPipe(handle.raw(), &mut ov) } == 0 {
        let error = unsafe { GetLastError() };
        if error == ERROR_IO_PENDING {
            crate::kernel::complete(&handle, &mut ov, &cancel, INFINITE)?;
        } else if error != ERROR_PIPE_CONNECTED {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
    }
    Ok(Pipe {
        handle: Arc::new(handle),
        cancel,
    })
}
pub fn connect(name: &str, cancel: Cancel) -> io::Result<Pipe> {
    let handle = unsafe {
        Handle::owned(CreateFileW(
            wide(name).as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IMPERSONATION,
            std::ptr::null_mut(),
        ))?
    };
    Ok(Pipe {
        handle: Arc::new(handle),
        cancel,
    })
}
pub fn service_pid(sid: &str) -> io::Result<Option<u32>> {
    let manager = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT) };
    if manager.is_null() {
        return Err(io::Error::last_os_error());
    }
    let service = unsafe {
        OpenServiceW(
            manager,
            wide(&service_name(sid)).as_ptr(),
            SERVICE_QUERY_STATUS,
        )
    };
    let error = io::Error::last_os_error();
    unsafe {
        CloseServiceHandle(manager);
    }
    if service.is_null() {
        return if error.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST as i32) {
            Ok(None)
        } else {
            Err(error)
        };
    }
    let mut status: SERVICE_STATUS_PROCESS = unsafe { std::mem::zeroed() };
    let mut n = 0;
    let ok = unsafe {
        QueryServiceStatusEx(
            service,
            SC_STATUS_PROCESS_INFO,
            &mut status as *mut _ as _,
            std::mem::size_of_val(&status) as u32,
            &mut n,
        )
    };
    unsafe {
        CloseServiceHandle(service);
    }
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok((status.dwCurrentState == SERVICE_RUNNING).then_some(status.dwProcessId))
    }
}
pub fn service_present(sid: &str) -> io::Result<bool> {
    let manager = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT) };
    if manager.is_null() {
        return Err(io::Error::last_os_error());
    }
    let service = unsafe {
        OpenServiceW(
            manager,
            wide(&service_name(sid)).as_ptr(),
            SERVICE_QUERY_STATUS,
        )
    };
    let error = io::Error::last_os_error();
    unsafe {
        CloseServiceHandle(manager);
    }
    if service.is_null() {
        if error.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST as i32) {
            Ok(false)
        } else {
            Err(error)
        }
    } else {
        unsafe {
            CloseServiceHandle(service);
        }
        Ok(true)
    }
}
/// Authenticate the server against SCM's protected installed service. An
/// unprivileged process squatting on the pipe name cannot supply an index.
pub fn installed_connection(cancel: Cancel) -> io::Result<Option<Pipe>> {
    let sid = user_sid()?;
    let Some(pid) = service_pid(&sid)? else {
        return Ok(None);
    };
    let pipe = connect(&pipe_name(&sid), cancel)?;
    let mut actual = 0;
    if unsafe { GetNamedPipeServerProcessId(pipe.handle.raw(), &mut actual) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if actual != pid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "pipe server is not the installed Seek helper",
        ));
    }
    Ok(Some(pipe))
}
pub fn pipe_server_pid(pipe: &Pipe) -> io::Result<u32> {
    let mut pid = 0;
    if unsafe { GetNamedPipeServerProcessId(pipe.handle.raw(), &mut pid) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(pid)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_ready_message_remains_compatible_with_optional_scan_profiles() {
        let mut wire = vec![];
        write(&mut wire, &serde_json::json!({"Ready":{"cursors":[]}})).unwrap();
        assert!(
            matches!(read_response(&mut wire.as_slice()).unwrap(), Response::Ready { scan_profiles, .. } if scan_profiles.is_empty())
        );
        let response = Response::Ready {
            cursors: vec![],
            scan_profiles: vec![crate::ntfs::ScanProfile {
                root: b"/C:".to_vec(),
                records: 100,
                directories: 10,
                metadata_ms: 1.25,
                namespace_ms: 2.5,
            }],
        };
        let mut wire = vec![];
        write_response(&mut wire, &response).unwrap();
        let decoded = read_response(&mut wire.as_slice()).unwrap();
        assert_eq!(
            serde_json::to_value(decoded).unwrap(),
            serde_json::to_value(response).unwrap()
        );
    }
    fn chunk(id: u32, names: Vec<u8>) -> Response {
        Response::Chunk {
            root: b"/C:".to_vec(),
            listing: Listing {
                id,
                names,
                ents: vec![],
            },
            last: true,
        }
    }
    #[test]
    fn response_batches_never_read_a_partial_next_frame_or_wait_after_control() {
        struct OneRead(Vec<u8>);
        impl Read for OneRead {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                assert!(!self.0.is_empty(), "batch tried to wait for more input");
                assert!(buffer.len() >= self.0.len());
                let len = self.0.len();
                buffer[..len].copy_from_slice(&self.0);
                self.0.clear();
                Ok(len)
            }
        }
        let mut wire = vec![];
        write_response(&mut wire, &chunk(0, vec![])).unwrap();
        // The second frame's length is buffered, but its body has not arrived.
        wire.extend(100u32.to_le_bytes());
        wire.extend(b"SK");
        let mut reader = std::io::BufReader::new(OneRead(wire));
        assert_eq!(read_response_batch(&mut reader).len(), 1);

        let mut wire = vec![];
        write_response(&mut wire, &chunk(0, vec![])).unwrap();
        write_response(
            &mut wire,
            &Response::Ready {
                cursors: vec![],
                scan_profiles: vec![],
            },
        )
        .unwrap();
        write_response(&mut wire, &chunk(1, vec![])).unwrap();
        let mut reader = std::io::BufReader::new(OneRead(wire));
        let batch = read_response_batch(&mut reader);
        assert_eq!(batch.len(), 2);
        assert!(matches!(batch[1], Response::Ready { .. }));
        assert!(
            matches!(read_response_batch(&mut reader)[0], Response::Chunk { ref listing, .. } if listing.id == 1)
        );
    }
    #[test]
    fn response_batches_bound_frame_count_and_bytes_without_losing_order() {
        let mut wire = vec![];
        for id in 0..66 {
            write_response(&mut wire, &chunk(id, vec![])).unwrap();
        }
        write_response(
            &mut wire,
            &Response::Ready {
                cursors: vec![],
                scan_profiles: vec![],
            },
        )
        .unwrap();
        let mut reader = std::io::BufReader::new(wire.as_slice());
        let batch = read_response_batch(&mut reader);
        assert_eq!(batch.len(), 64);
        for (id, response) in batch.iter().enumerate() {
            assert!(matches!(response, Response::Chunk { listing, .. } if listing.id == id as u32));
        }
        let batch = read_response_batch(&mut reader);
        assert_eq!(batch.len(), 3);
        assert!(matches!(batch[0], Response::Chunk { ref listing, .. } if listing.id == 64));
        assert!(matches!(batch[2], Response::Ready { .. }));
        let mut wire = vec![];
        for id in 0..2 {
            write_response(&mut wire, &chunk(id, vec![b'A'; 140 * 1024])).unwrap();
        }
        let mut reader = std::io::BufReader::with_capacity(1024 * 1024, wire.as_slice());
        assert_eq!(read_response_batch(&mut reader).len(), 1);
        assert!(
            matches!(read_response_batch(&mut reader)[0], Response::Chunk { ref listing, .. } if listing.id == 1)
        );
    }
    #[test]
    fn bounded_protocol_rejects_malformed_truncated_and_oversized_messages() {
        let mut b = vec![];
        write(&mut b, &Request::Ping { version: VERSION }).unwrap();
        assert!(matches!(
            read::<Request>(&mut b.as_slice()).unwrap(),
            Request::Ping { version: VERSION }
        ));
        for n in 0..b.len() {
            assert!(read::<Request>(&mut &b[..n]).is_err());
        }
        assert!(read::<Request>(&mut ((MAX_FRAME as u32) + 1).to_le_bytes().as_slice()).is_err());
        assert!(validate_sid("S-1-5-21;fake").is_err());
    }
    #[test]
    fn compact_listing_roundtrip_preserves_metadata_and_rejects_malformed_frames() {
        let listing = Listing {
            id: 17,
            names: vec![65, 0xed, 0xa0, 0x80],
            ents: vec![crate::walk::RawEnt {
                name_off: 0,
                name_len: 4,
                kind: 33,
                size: u64::MAX,
                mtime: 123,
                child: crate::walk::NONE,
            }],
        };
        let value = Response::Chunk {
            root: b"/C:/Downloads".to_vec(),
            listing,
            last: true,
        };
        let mut bytes = Vec::new();
        write_response(&mut bytes, &value).unwrap();
        let decoded = read_response(&mut bytes.as_slice()).unwrap();
        assert_eq!(
            serde_json::to_value(decoded).unwrap(),
            serde_json::to_value(value).unwrap()
        );
        for n in 0..bytes.len() {
            assert!(read_response(&mut &bytes[..n]).is_err());
        }
        let mut bad = bytes.clone();
        bad[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_response(&mut bad.as_slice()).is_err());
        bad = bytes.clone();
        bad[24] = 2;
        assert!(read_response(&mut bad.as_slice()).is_err());
    }
    #[test]
    fn real_local_pipe_authentication_exchange_and_blocking_read_cancellation() {
        let sid = user_sid().unwrap();
        let name = format!("{}-test-{}", pipe_name(&sid), std::process::id());
        let cancel = Cancel::new().unwrap();
        let server_cancel = cancel.clone();
        let server_name = name.clone();
        let server = std::thread::spawn(move || {
            let mut pipe = listen(&sid, &server_name, server_cancel).unwrap();
            let request: Request = read(&mut pipe).unwrap();
            assert!(Impersonation::client(&pipe, "S-1-5-18").is_err());
            assert_eq!(user_sid().unwrap(), sid);
            let _user = Impersonation::client(&pipe, &sid).unwrap();
            let token = duplicate_client_token().unwrap();
            std::thread::scope(|scope| {
                for _ in 0..8 {
                    let token = &token;
                    let sid = &sid;
                    scope.spawn(move || {
                        let _user = Impersonation::token(token).unwrap();
                        assert_eq!(&user_sid().unwrap(), sid);
                    });
                }
            });
            assert!(matches!(request, Request::Ping { version: VERSION }));
            write(&mut pipe, &Response::Pong { version: VERSION }).unwrap();
            read::<Request>(&mut pipe).unwrap_err().kind()
        });
        // Only the test waits for listener registration; production never polls.
        let mut client = loop {
            match connect(&name, cancel.clone()) {
                Ok(pipe) => break pipe,
                Err(e) if e.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32) => {
                    std::thread::yield_now()
                }
                Err(e) => panic!("{e}"),
            }
        };
        let deadline = cancel.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let watchdog = std::thread::spawn(move || {
            if done_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .is_err()
            {
                deadline.cancel();
            }
        });
        write(&mut client, &Request::Ping { version: VERSION }).unwrap();
        assert!(matches!(
            read::<Response>(&mut client).unwrap(),
            Response::Pong { version: VERSION }
        ));
        cancel.cancel();
        assert_eq!(server.join().unwrap(), io::ErrorKind::ConnectionAborted);
        let _ = done_tx.send(());
        watchdog.join().unwrap();
    }
    #[test]
    #[ignore = "explicit real-pipe performance check; no raw-volume access"]
    fn benchmark_buffered_directory_transfer() {
        const COUNT: u32 = 278_212;
        for buffered in [false, true] {
            let sid = user_sid().unwrap();
            let name = format!(
                "{}-transfer-{}-{buffered}",
                pipe_name(&sid),
                std::process::id()
            );
            let cancel = Cancel::new().unwrap();
            let server_cancel = cancel.clone();
            let server_name = name.clone();
            let deadline = cancel.clone();
            let (done_tx, done_rx) = std::sync::mpsc::channel();
            let watchdog = std::thread::spawn(move || {
                if done_rx
                    .recv_timeout(std::time::Duration::from_secs(60))
                    .is_err()
                {
                    deadline.cancel();
                }
            });
            let server = std::thread::spawn(move || {
                let mut pipe = listen(&sid, &server_name, server_cancel).unwrap();
                let _: Request = read(&mut pipe).unwrap();
                let _user = Impersonation::client(&pipe, &sid).unwrap();
                let mut writer: Box<dyn Write> = if buffered {
                    Box::new(std::io::BufWriter::with_capacity(1024 * 1024, pipe))
                } else {
                    Box::new(pipe)
                };
                for id in 0..COUNT {
                    let listing = Listing {
                        id,
                        names: b"package.jsoninvoice.pdf".to_vec(),
                        ents: vec![
                            crate::walk::RawEnt {
                                name_off: 0,
                                name_len: 12,
                                kind: 0,
                                size: 123,
                                mtime: 456,
                                child: crate::walk::NONE,
                            },
                            crate::walk::RawEnt {
                                name_off: 12,
                                name_len: 11,
                                kind: 0,
                                size: 789,
                                mtime: 456,
                                child: crate::walk::NONE,
                            },
                        ],
                    };
                    write_response(
                        &mut writer,
                        &Response::Chunk {
                            root: b"/C:".to_vec(),
                            listing,
                            last: true,
                        },
                    )
                    .unwrap();
                }
                write_response(
                    &mut writer,
                    &Response::Ready {
                        cursors: vec![],
                        scan_profiles: vec![],
                    },
                )
                .unwrap();
                writer.flush().unwrap();
            });
            let mut client = loop {
                match connect(&name, cancel.clone()) {
                    Ok(pipe) => break pipe,
                    Err(e) if e.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32) => {
                        std::thread::yield_now()
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            write(&mut client, &Request::Ping { version: VERSION }).unwrap();
            let mut reader: Box<dyn Read> = if buffered {
                Box::new(std::io::BufReader::with_capacity(1024 * 1024, client))
            } else {
                Box::new(client)
            };
            let timer = std::time::Instant::now();
            for id in 0..COUNT {
                let Response::Chunk {
                    root,
                    listing,
                    last,
                } = read_response(&mut reader).unwrap()
                else {
                    panic!("missing directory");
                };
                assert_eq!(root, b"/C:");
                assert_eq!(listing.id, id);
                assert!(last);
                assert_eq!(listing.names, b"package.jsoninvoice.pdf");
                assert_eq!(listing.ents.len(), 2);
                assert_eq!(listing.ents[1].size, 789);
            }
            assert!(matches!(
                read_response(&mut reader).unwrap(),
                Response::Ready { .. }
            ));
            println!(
                "{{\"buffered\":{buffered},\"directories\":{COUNT},\"files\":{},\"wall_ms\":{}}}",
                COUNT * 2,
                timer.elapsed().as_secs_f64() * 1000.
            );
            server.join().unwrap();
            let _ = done_tx.send(());
            watchdog.join().unwrap();
        }
    }
}
