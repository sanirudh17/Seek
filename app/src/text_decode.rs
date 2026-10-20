//! Shared strict decoding for previews and plain-text content search.
//! Binary data is never made to look like text by inserting replacement glyphs.
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetACP() -> u32;
    fn MultiByteToWideChar(
        codepage: u32,
        flags: u32,
        input: *const u8,
        len: i32,
        output: *mut u16,
        capacity: i32,
    ) -> i32;
}

fn readable(text: String) -> Result<String, String> {
    if text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t' | '\u{c}'))
    {
        Err("Binary or mixed-format data; text preview is unavailable.".into())
    } else {
        Ok(text)
    }
}

fn utf16(bytes: &[u8], little: bool, partial: bool) -> Result<String, String> {
    if !partial && !bytes.len().is_multiple_of(2) {
        return Err("Truncated UTF-16 text".into());
    }
    let mut words: Vec<_> = bytes
        .chunks_exact(2)
        .map(|p| {
            if little {
                u16::from_le_bytes([p[0], p[1]])
            } else {
                u16::from_be_bytes([p[0], p[1]])
            }
        })
        .collect();
    if partial && words.last().is_some_and(|w| (0xd800..=0xdbff).contains(w)) {
        words.pop();
    }
    readable(String::from_utf16(&words).map_err(|_| "Invalid UTF-16 text")?)
}

pub fn decode(bytes: &[u8], partial: bool) -> Result<String, String> {
    for (mark, little) in [([0xff, 0xfe, 0, 0], true), ([0, 0, 0xfe, 0xff], false)] {
        if let Some(bytes) = bytes.strip_prefix(&mark) {
            if !partial && !bytes.len().is_multiple_of(4) {
                return Err("Truncated UTF-32 text".into());
            }
            let text: Option<String> = bytes
                .chunks_exact(4)
                .map(|p| {
                    let word = [p[0], p[1], p[2], p[3]];
                    char::from_u32(if little {
                        u32::from_le_bytes(word)
                    } else {
                        u32::from_be_bytes(word)
                    })
                })
                .collect();
            return readable(text.ok_or("Invalid UTF-32 text")?);
        }
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        return utf16(bytes, true, partial);
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        return utf16(bytes, false, partial);
    }
    let pairs = bytes.len() / 2;
    if pairs >= 4 {
        let even = bytes.iter().step_by(2).filter(|b| **b == 0).count();
        let odd = bytes.iter().skip(1).step_by(2).filter(|b| **b == 0).count();
        if odd * 3 >= pairs && even * 10 <= pairs {
            return utf16(bytes, true, partial);
        }
        if even * 3 >= pairs && odd * 10 <= pairs {
            return utf16(bytes, false, partial);
        }
    }
    let bom = bytes.starts_with(&[0xef, 0xbb, 0xbf]);
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(text) => return readable(text.into()),
        Err(e) if partial && e.error_len().is_none() => {
            return readable(
                std::str::from_utf8(&bytes[..e.valid_up_to()])
                    .map_err(|_| "Invalid UTF-8 text")?
                    .into(),
            );
        }
        Err(_) if bom => return Err("Invalid UTF-8 text".into()),
        Err(_) => {}
    }
    if bytes
        .iter()
        .any(|b| *b < 32 && !matches!(*b, b'\n' | b'\r' | b'\t' | 12))
    {
        return Err("Binary or mixed-format data; text preview is unavailable.".into());
    }
    let codepage = unsafe { GetACP() };
    // Older Windows logs can still be ANSI on systems configured for UTF-8.
    let codepage = if codepage == 65001 { 1252 } else { codepage };
    let len = i32::try_from(bytes.len()).map_err(|_| "Text is too large")?;
    if len == 0 {
        return Ok(String::new());
    }
    let size =
        unsafe { MultiByteToWideChar(codepage, 8, bytes.as_ptr(), len, std::ptr::null_mut(), 0) };
    if size <= 0 {
        return Err("Unsupported or invalid text encoding".into());
    }
    let mut output = vec![0u16; size as usize];
    if unsafe { MultiByteToWideChar(codepage, 8, bytes.as_ptr(), len, output.as_mut_ptr(), size) }
        != size
    {
        return Err("Unsupported or invalid text encoding".into());
    }
    readable(String::from_utf16(&output).map_err(|_| "Invalid Windows text")?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_and_bomless_windows_logs_preserve_symbols() {
        let text = "Header\nRésumé — € 中文 🦀\tEnd";
        assert_eq!(decode(text.as_bytes(), false).unwrap(), text);
        for little in [true, false] {
            let bytes: Vec<_> = text
                .encode_utf16()
                .flat_map(|w| {
                    if little {
                        w.to_le_bytes()
                    } else {
                        w.to_be_bytes()
                    }
                })
                .collect();
            assert_eq!(decode(&bytes, false).unwrap(), text);
        }
        assert_eq!(
            decode(b"Windows \x93quote\x94 \x97 \x80", false).unwrap(),
            "Windows “quote” — €"
        );
    }
    #[test]
    fn binary_and_invalid_explicit_unicode_do_not_produce_gibberish() {
        assert!(decode(b"GPU log\0\x81\x02\xffdebug", false).is_err());
        assert!(decode(&[0xef, 0xbb, 0xbf, 0xff], false).is_err());
        assert!(decode(&[0xff, 0xfe, 0, 0xd8], false).is_err());
        assert_eq!(decode(&[b'a', 0xf0, 0x9f], true).unwrap(), "a");
    }
}
