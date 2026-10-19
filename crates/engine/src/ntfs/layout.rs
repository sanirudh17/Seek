//! Bounded decoder for FSCTL_QUERY_FILE_LAYOUT metadata batches (SDK 26100).
//! Names include every hard link; stream metadata includes resident/empty data.
use super::record::{Name, Record, bad, seconds, u32_at, u64_at};
use std::io;

fn relative(bytes: &[u8], base: usize, offset: u32, minimum: usize) -> io::Result<usize> {
    let p = base
        .checked_add(offset as usize)
        .ok_or_else(|| bad("layout offset overflow"))?;
    if (offset as usize) < minimum || !p.is_multiple_of(8) || p >= bytes.len() {
        return Err(bad("layout offset bounds"));
    }
    Ok(p)
}
pub(super) fn decode(
    bytes: &[u8],
    mut visit: impl FnMut(Record) -> io::Result<()>,
) -> io::Result<usize> {
    let count = u32_at(bytes, 0)? as usize;
    if count == 0 || count > bytes.len() / 40 || u32_at(bytes, 8)? & 1 == 0 {
        return Err(bad("empty/unsupported NTFS layout batch"));
    }
    let mut p = relative(bytes, 0, u32_at(bytes, 4)?, 16)?;
    for i in 0..count {
        let next = u32_at(bytes, p + 4)?;
        let end = if next == 0 {
            bytes.len()
        } else {
            relative(bytes, p, next, 40)?
        };
        if (i + 1 == count) != (next == 0) || end < p + 40 {
            return Err(bad("NTFS layout file chain"));
        }
        let b = &bytes[..end];
        if u32_at(b, p)? != 1 {
            return Err(bad("NTFS layout file version"));
        }
        let attributes = u32_at(b, p + 12)?;
        let mut r = Record {
            id: u64_at(b, p + 16)?,
            attributes: Some(attributes),
            directory: attributes & 16 != 0,
            size: Some(0),
            names: Vec::with_capacity(1),
            ..Default::default()
        };
        if r.id & 0x0000_ffff_ffff_ffff < 16 && r.id & 0x0000_ffff_ffff_ffff != 5 {
            p = end;
            continue;
        }
        let info = u32_at(b, p + 32)?;
        let info_len = u32_at(b, p + 36)?;
        if info == 0 || (info_len != 0 && info_len < 24) {
            return Err(bad("NTFS layout missing basic information"));
        }
        let info = relative(b, p, info, 40)?;
        r.mtime = Some(seconds(u64_at(b, info + 16)?));
        // The root's internal self name is not an ordinary file name and can
        // contain a separator. Keep its identity/metadata, without publishing
        // or validating it as a child name.
        if r.id & 0x0000_ffff_ffff_ffff == 5 {
            if !r.directory {
                return Err(bad("NTFS root layout is not a directory"));
            }
            visit(r)?;
            p = end;
            continue;
        }
        let mut offset = u32_at(b, p + 24)?;
        let mut base = p;
        let mut minimum = 40;
        while offset != 0 {
            let n = relative(b, base, offset, minimum)?;
            let flags = u32_at(b, n + 4)?;
            let len = u32_at(b, n + 16)? as usize;
            if len == 0 || !len.is_multiple_of(2) || len > 510 {
                return Err(bad("NTFS layout name length"));
            }
            // NTFS filenames have at most 255 UTF-16 units. Avoid allocating
            // an intermediate vector for each of millions of file names.
            let mut units = [0u16; 255];
            let wide = &mut units[..len / 2];
            for (unit, bytes) in wide.iter_mut().zip(
                b.get(n + 24..n + 24 + len)
                    .ok_or_else(|| bad("NTFS layout name bounds"))?
                    .chunks_exact(2),
            ) {
                *unit = u16::from_le_bytes([bytes[0], bytes[1]]);
            }
            if wide.iter().any(|c| matches!(*c, 0 | 47 | 92)) {
                return Err(bad("NTFS layout name separator"));
            }
            r.names.push(Name {
                parent: u64_at(b, n + 8)?,
                text: crate::paths::encode(wide),
                namespace: if flags & 2 != 0 && flags & 1 == 0 {
                    2
                } else {
                    1
                },
            });
            offset = u32_at(b, n)?;
            base = n;
            minimum = (24 + len).next_multiple_of(8);
        }
        offset = u32_at(b, p + 28)?;
        base = p;
        minimum = 40;
        while offset != 0 {
            let s = relative(b, base, offset, minimum)?;
            // Version 2 repurposes the version-1 reserved DWORD at +32 for
            // StreamInformationOffset. The name/type/EOF fields used here
            // retain their offsets; drivers still return version 1 streams.
            let version = u32_at(b, s)?;
            if !matches!(version, 1 | 2) {
                return Err(bad(&format!(
                    "NTFS layout stream version {version} at offset {s}, file {}",
                    r.id
                )));
            }
            let len = u32_at(b, s + 44)? as usize;
            if !len.is_multiple_of(2) {
                return Err(bad("NTFS layout stream name length"));
            }
            let identifier = b
                .get(s + 48..s + 48 + len)
                .ok_or_else(|| bad("NTFS layout stream bounds"))?;
            // The stream identifier uses :name:$TYPE notation. Only unnamed
            // $DATA determines file length; alternate streams are not entries.
            let unnamed = identifier.is_empty() || identifier == b":\0:\0$\0D\0A\0T\0A\0";
            if u32_at(b, s + 36)? == 0x80 && unnamed {
                let size = u64_at(b, s + 24)?;
                if size > i64::MAX as u64 {
                    return Err(bad("negative NTFS file size"));
                }
                r.size = Some(size);
            }
            offset = u32_at(b, s + 4)?;
            base = s;
            minimum = (48 + len).next_multiple_of(8);
        }
        r.finish()?;
        visit(r)?;
        p = end;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn put(b: &mut [u8], p: usize, v: u32) {
        b[p..p + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 232];
        put(&mut b, 0, 1);
        put(&mut b, 4, 16);
        put(&mut b, 8, 1);
        put(&mut b, 16, 1);
        put(&mut b, 28, 6);
        b[32..40].copy_from_slice(&42u64.to_le_bytes());
        put(&mut b, 40, 40);
        put(&mut b, 44, 104);
        put(&mut b, 48, 160);
        put(&mut b, 52, 56);
        // Two distinct names, including an unpaired surrogate, in two parents.
        put(&mut b, 56, 32);
        put(&mut b, 60, 1);
        b[64..72].copy_from_slice(&22u64.to_le_bytes());
        put(&mut b, 72, 2);
        b[80..82].copy_from_slice(&65u16.to_le_bytes());
        put(&mut b, 92, 1);
        b[96..104].copy_from_slice(&33u64.to_le_bytes());
        put(&mut b, 104, 2);
        b[112..114].copy_from_slice(&0xd800u16.to_le_bytes());
        put(&mut b, 120, 2);
        b[144..152].copy_from_slice(&12345u64.to_le_bytes());
        put(&mut b, 156, 0x80);
        put(&mut b, 164, 14);
        for (i, c) in "::$DATA".encode_utf16().enumerate() {
            b[168 + i * 2..170 + i * 2].copy_from_slice(&c.to_le_bytes());
        }
        b[192..200]
            .copy_from_slice(&((11_644_473_600u64 + 1_700_000_000) * 10_000_000).to_le_bytes());
        b
    }
    #[test]
    fn bulk_metadata_keeps_hard_links_resident_sizes_and_surrogates() {
        let b = fixture();
        assert_eq!(
            decode(&b, |r| {
                assert_eq!(r.names.len(), 2);
                assert_eq!(r.size, Some(12345));
                assert_eq!(r.mtime, Some(1_700_000_000));
                assert_eq!(crate::paths::decode(&r.names[1].text), [0xd800]);
                Ok(())
            })
            .unwrap(),
            1
        );
    }
    #[test]
    fn version_one_streams_retain_names_type_and_eof() {
        let mut b = fixture();
        put(&mut b, 120, 1);
        put(&mut b, 152, 0); // Version 1's reserved DWORD.
        decode(&b, |r| {
            assert_eq!(r.size, Some(12345));
            assert_eq!(r.names.len(), 2);
            Ok(())
        })
        .unwrap();
    }
    #[test]
    fn empty_default_and_alternate_stream_identifiers_keep_only_unnamed_length() {
        for (identifier, size) in [("", 12345), ("::$DATA", 12345), (":x:$DATA", 0)] {
            let mut b = fixture();
            let units: Vec<_> = identifier.encode_utf16().collect();
            put(&mut b, 164, (units.len() * 2) as u32);
            for (i, unit) in units.iter().enumerate() {
                b[168 + i * 2..170 + i * 2].copy_from_slice(&unit.to_le_bytes());
            }
            decode(&b, |r| {
                assert_eq!(r.size, Some(size));
                Ok(())
            })
            .unwrap();
        }
    }
    #[test]
    fn maximum_utf16_name_preserves_emoji_and_unpaired_surrogates() {
        let mut b = vec![0; 656];
        put(&mut b, 0, 1);
        put(&mut b, 4, 16);
        put(&mut b, 8, 1);
        put(&mut b, 16, 1);
        put(&mut b, 28, 16);
        b[32..40].copy_from_slice(&42u64.to_le_bytes());
        put(&mut b, 40, 40);
        put(&mut b, 48, 576);
        put(&mut b, 52, 56);
        b[64..72].copy_from_slice(&5u64.to_le_bytes());
        put(&mut b, 72, 510);
        let mut units = vec![65u16; 251];
        units.extend([0xd800, 0xd83d, 0xde80, 0xdc00]);
        for (i, unit) in units.iter().enumerate() {
            b[80 + i * 2..82 + i * 2].copy_from_slice(&unit.to_le_bytes());
        }
        decode(&b, |r| {
            assert_eq!(crate::paths::decode(&r.names[0].text), units);
            Ok(())
        })
        .unwrap();
    }
    #[test]
    fn root_identity_is_retained_without_exposing_its_internal_self_name() {
        let mut b = fixture();
        b[32..40].copy_from_slice(&((1u64 << 48) | 5).to_le_bytes());
        put(&mut b, 28, 16);
        b[80..82].copy_from_slice(&92u16.to_le_bytes());
        decode(&b, |r| {
            assert_eq!(r.id, (1u64 << 48) | 5);
            assert!(r.directory);
            assert!(r.names.is_empty());
            assert_eq!(r.mtime, Some(1_700_000_000));
            Ok(())
        })
        .unwrap();
    }
    #[test]
    fn malformed_chains_offsets_versions_and_truncated_values_are_rejected() {
        for n in 0..200 {
            assert!(decode(&fixture()[..n], |_| Ok(())).is_err(), "{n}");
        }
        for (p, v) in [
            (4, 0),
            (16, 7),
            (56, 8),
            (72, 511),
            (120, 8),
            (164, u32::MAX),
            (48, 0),
        ] {
            let mut b = fixture();
            put(&mut b, p, v);
            assert!(decode(&b, |_| Ok(())).is_err(), "{p}");
        }
    }
}
