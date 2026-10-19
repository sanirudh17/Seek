//! Windows paths use reversible WTF-8 internally. A virtual '/' contains drive roots.
use std::{
    ffi::OsString,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};

pub fn encode(wide: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(wide.len());
    let mut i = 0;
    while i < wide.len() {
        let mut c = wide[i] as u32;
        if (0xd800..=0xdbff).contains(&c)
            && i + 1 < wide.len()
            && (0xdc00..=0xdfff).contains(&wide[i + 1])
        {
            c = 0x10000 + ((c - 0xd800) << 10) + (wide[i + 1] as u32 - 0xdc00);
            i += 1;
        }
        match c {
            0..=0x7f => out.push(c as u8),
            0x80..=0x7ff => {
                out.push((0xc0 | (c >> 6)) as u8);
                out.push((0x80 | (c & 63)) as u8);
            }
            0x800..=0xffff => {
                out.push((0xe0 | (c >> 12)) as u8);
                out.push((0x80 | ((c >> 6) & 63)) as u8);
                out.push((0x80 | (c & 63)) as u8);
            }
            _ => {
                out.push((0xf0 | (c >> 18)) as u8);
                out.push((0x80 | ((c >> 12) & 63)) as u8);
                out.push((0x80 | ((c >> 6) & 63)) as u8);
                out.push((0x80 | (c & 63)) as u8);
            }
        }
        i += 1;
    }
    out
}

pub fn decode(bytes: &[u8]) -> Vec<u16> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let len = if b < 128 {
            1
        } else if b < 224 {
            2
        } else if b < 240 {
            3
        } else {
            4
        };
        if i + len > bytes.len() {
            out.push(0xfffd);
            break;
        }
        let mut c = (b & match len {
            1 => 127,
            2 => 31,
            3 => 15,
            _ => 7,
        }) as u32;
        for j in 1..len {
            c = (c << 6) | (bytes[i + j] & 63) as u32;
        }
        if c <= 0xffff {
            out.push(c as u16);
        } else {
            c -= 0x10000;
            out.push((0xd800 + (c >> 10)) as u16);
            out.push((0xdc00 + (c & 1023)) as u16);
        }
        i += len;
    }
    out
}

pub fn internal(path: &Path) -> Vec<u8> {
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.starts_with(&[92, 92, 63, 92]) {
        wide.drain(..4);
    }
    for c in &mut wide {
        if *c == 92 {
            *c = 47;
        }
    }
    let mut out = encode(&wide);
    if !out.starts_with(b"/") {
        out.insert(0, b'/');
    }
    while out.len() > 1 && out.ends_with(b"/") {
        out.pop();
    }
    out
}

pub fn native(path: &[u8]) -> PathBuf {
    let path = if path.starts_with(b"/") {
        &path[1..]
    } else {
        path
    };
    let mut wide = decode(path);
    for c in &mut wide {
        if *c == 47 {
            *c = 92;
        }
    }
    if wide.len() == 2 && wide[1] == 58 {
        wide.push(92);
    }
    PathBuf::from(OsString::from_wide(&wide))
}

pub fn extended(path: &[u8]) -> Vec<u16> {
    let mut wide: Vec<u16> = native(path).as_os_str().encode_wide().collect();
    let mut prefix = vec![92, 92, 63, 92];
    if wide.starts_with(&[92, 92]) {
        prefix.extend([85, 78, 67, 92]);
        wide.drain(..2);
    }
    prefix.extend(wide);
    prefix.push(0);
    prefix
}

pub fn cmp_ci(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    // CompareStringOrdinal uses uppercase ordinal values for ASCII. Most
    // indexed names are ASCII: avoid two allocations and a Win32 call for
    // every comparison while retaining the native Unicode/WTF-8 path.
    if a.is_ascii() && b.is_ascii() {
        for (&a, &b) in a.iter().zip(b) {
            let order = a.to_ascii_uppercase().cmp(&b.to_ascii_uppercase());
            if !order.is_eq() {
                return order;
            }
        }
        return a.len().cmp(&b.len());
    }
    use windows_sys::Win32::Globalization::{
        CSTR_GREATER_THAN, CSTR_LESS_THAN, CompareStringOrdinal,
    };
    let (a, b) = (decode(a), decode(b));
    let result =
        unsafe { CompareStringOrdinal(a.as_ptr(), a.len() as i32, b.as_ptr(), b.len() as i32, 1) };
    match result {
        CSTR_LESS_THAN => std::cmp::Ordering::Less,
        CSTR_GREATER_THAN => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reversible_utf16() {
        let wide = vec![65, 0xd800, 66, 0xdc00, 0xd83d, 0xde80, 0xe9];
        assert_eq!(decode(&encode(&wide)), wide);
    }
    #[test]
    fn drive_and_long_paths() {
        let p = PathBuf::from(format!("C:\\{}\\🚀.txt", "folder\\".repeat(100)));
        assert_eq!(native(&internal(&p)), p);
        assert!(extended(&internal(&p)).starts_with(&[92, 92, 63, 92]));
    }
    #[test]
    fn ordinal_case() {
        assert_eq!(
            cmp_ci("Éclair".as_bytes(), "éCLAIR".as_bytes()),
            std::cmp::Ordering::Equal
        );
    }
    #[test]
    fn ascii_fast_order_matches_windows_including_punctuation_and_lengths() {
        use windows_sys::Win32::Globalization::CompareStringOrdinal;
        let native = |a: &[u8], b: &[u8]| {
            let (a, b) = (decode(a), decode(b));
            let result = unsafe {
                CompareStringOrdinal(a.as_ptr(), a.len() as i32, b.as_ptr(), b.len() as i32, 1)
            };
            assert_ne!(result, 0);
            result.cmp(&2)
        };
        for a in 0u8..128 {
            for b in 0u8..128 {
                assert_eq!(cmp_ci(&[a], &[b]), native(&[a], &[b]), "{a} {b}");
            }
        }
        for a in [
            b"".as_slice(),
            b"package.json",
            b"Package.JSON",
            b"[file]",
            b"_file",
            b"file",
            b"file.txt",
            b"a/Z",
        ] {
            for b in [
                b"".as_slice(),
                b"package.json",
                b"PACKAGE",
                b"[file]",
                b"_file",
                b"file",
                b"FILE.TXT",
                b"a/z",
            ] {
                assert_eq!(cmp_ci(a, b), native(a, b));
            }
        }
    }
}
