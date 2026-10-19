//! Short-lived PDF filter host. Receives only already scope-checked bytes on
//! stdin; never opens a document path. The parent enforces a hard time limit.
use std::io::{Read, Write};
use windows::{
    Win32::{Storage::IndexServer::*, System::Com::*, UI::Shell::SHCreateMemStream},
    core::{GUID, Interface, PWSTR},
};
fn registry_default(key: &str) -> Result<String, String> {
    use windows_sys::Win32::System::Registry::*;
    let key: Vec<u16> = key.encode_utf16().chain(Some(0)).collect();
    let mut bytes = 512u32;
    let mut value = [0u16; 256];
    let status = unsafe {
        RegGetValueW(
            HKEY_CLASSES_ROOT,
            key.as_ptr(),
            std::ptr::null(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            value.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if status != 0 {
        return Err(format!("PDF filter registry: {status}"));
    }
    Ok(String::from_utf16_lossy(
        &value[..value.iter().position(|x| *x == 0).unwrap_or(value.len())],
    ))
}
/// External filter updates also invalidate remembered PDF text.
pub(crate) fn fingerprint() -> Result<String, String> {
    use std::os::windows::fs::MetadataExt;
    let handler = registry_default(r".pdf\PersistentHandler")?;
    let class = registry_default(&format!(
        r"CLSID\{handler}\PersistentAddinsRegistered\{{89BCB740-6119-101A-BCB7-00DD010655AF}}"
    ))?;
    let dll = registry_default(&format!(r"CLSID\{class}\InprocServer32"))?;
    let dll = dll.trim_matches('"').replace(
        "%SystemRoot%",
        &std::env::var("SystemRoot").unwrap_or_default(),
    );
    let meta = std::fs::metadata(&dll).map_err(|e| e.to_string())?;
    Ok(format!(
        "{class}|{dll}|{}|{}",
        meta.len(),
        meta.last_write_time()
    ))
}
fn extract(bytes: &[u8]) -> Result<String, String> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|e| e.to_string())?;
        let handler = registry_default(r".pdf\PersistentHandler")?;
        let class = registry_default(&format!(
            r"CLSID\{handler}\PersistentAddinsRegistered\{{89BCB740-6119-101A-BCB7-00DD010655AF}}"
        ))?;
        let class = GUID::try_from(class.trim().trim_matches(['{', '}']))
            .map_err(|e| format!("PDF filter class: {e}"))?;
        let filter: IFilter = CoCreateInstance(&class, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| format!("Create filter: {e}"))?;
        let stream = SHCreateMemStream(Some(bytes)).ok_or("Cannot create PDF memory stream")?;
        filter
            .cast::<IPersistStream>()
            .map_err(|e| e.to_string())?
            .Load(&stream)
            .map_err(|e| format!("Load stream: {e}"))?;
        let mut flags = 0;
        windows::core::HRESULT((Interface::vtable(&filter).Init)(
            Interface::as_raw(&filter),
            0,
            0,
            std::ptr::null(),
            &mut flags,
        ))
        .ok()
        .map_err(|e| format!("Initialize PDF filter: {e}"))?;
        let mut text = Vec::<u16>::new();
        let mut chunk = STAT_CHUNK::default();
        loop {
            let status = windows::core::HRESULT(filter.GetChunk(&mut chunk));
            if status == FILTER_E_END_OF_CHUNKS {
                break;
            }
            status.ok().map_err(|e| e.to_string())?;
            if chunk.flags.0 & CHUNK_TEXT.0 == 0 {
                continue;
            }
            if !text.is_empty() {
                match chunk.breakType {
                    CHUNK_NO_BREAK => {}
                    CHUNK_EOW | CHUNK_EOS => text.push(b' ' as u16),
                    _ => text.push(b'\n' as u16),
                }
            }
            loop {
                let mut buffer = [0u16; 4096];
                let mut len = buffer.len() as u32;
                let status =
                    windows::core::HRESULT(filter.GetText(&mut len, PWSTR(buffer.as_mut_ptr())));
                if status == FILTER_E_NO_MORE_TEXT {
                    break;
                }
                status.ok().map_err(|e| e.to_string())?;
                if len as usize > buffer.len() {
                    return Err("Invalid PDF filter output length".into());
                }
                text.extend_from_slice(&buffer[..len as usize]);
                if text.len() * 2 > 8 * 1024 * 1024 {
                    return Err("PDF text exceeds 8 MiB".into());
                }
                if status == FILTER_S_LAST_TEXT || len == 0 {
                    break;
                }
            }
        }
        let text = String::from_utf16_lossy(&text);
        if text.len() > 8 * 1024 * 1024 {
            return Err("PDF text exceeds 8 MiB".into());
        }
        Ok(text)
    }
}
pub fn run() -> i32 {
    let Some(job) = crate::argument("--content-pdf-job") else {
        eprintln!("A parent job is required");
        return 2;
    };
    if let Err(error) = crate::child_job::attach_current(&job) {
        eprintln!("Cannot join parent job: {error}");
        return 2;
    }

    let mut bytes = Vec::new();
    if std::io::stdin()
        .take(crate::content::MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > crate::content::MAX_FILE
    {
        return 2;
    }
    match extract(&bytes) {
        Ok(text) => {
            if std::io::stdout().write_all(text.as_bytes()).is_ok() {
                0
            } else {
                3
            }
        }
        Err(error) => {
            eprintln!("{error}");
            2
        }
    }
}
