//! Bounds-checked NTFS 3.x metadata reader. No content streams are read.
use std::io;
pub(crate) fn bad(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
pub(crate) fn u16_at(b: &[u8], p: usize) -> io::Result<u16> {
    Ok(u16::from_le_bytes(
        b.get(p..p + 2)
            .ok_or_else(|| bad("truncated u16"))?
            .try_into()
            .unwrap(),
    ))
}
pub(crate) fn u32_at(b: &[u8], p: usize) -> io::Result<u32> {
    Ok(u32::from_le_bytes(
        b.get(p..p + 4)
            .ok_or_else(|| bad("truncated u32"))?
            .try_into()
            .unwrap(),
    ))
}
pub(crate) fn u64_at(b: &[u8], p: usize) -> io::Result<u64> {
    Ok(u64::from_le_bytes(
        b.get(p..p + 8)
            .ok_or_else(|| bad("truncated u64"))?
            .try_into()
            .unwrap(),
    ))
}

#[derive(Debug, Clone)]
pub struct Name {
    pub parent: u64,
    pub text: Vec<u8>,
    pub namespace: u8,
}
#[derive(Debug, Default)]
pub struct Record {
    pub id: u64,
    pub base: u64,
    pub links: u16,
    pub directory: bool,
    pub attributes: Option<u32>,
    pub mtime: Option<u32>,
    pub size: Option<u64>,
    pub reparse_tag: Option<u32>,
    pub names: Vec<Name>,
    pub extensions: Vec<u64>,
    pub attribute_lists: Vec<AttributeList>,
}
#[derive(Debug)]
pub struct Run {
    pub vcn: u64,
    pub lcn: Option<u64>,
    pub clusters: u64,
}
#[derive(Debug)]
pub struct AttributeList {
    pub size: u64,
    pub runs: Vec<Run>,
}
pub fn attribute_references(v: &[u8]) -> io::Result<Vec<u64>> {
    let mut out = vec![];
    let mut p = 0;
    while p < v.len() {
        let n = u16_at(v, p + 4)? as usize;
        if n < 26 || p + n > v.len() {
            return Err(bad("attribute list bounds"));
        }
        out.push(u64_at(v, p + 16)?);
        p += n;
    }
    Ok(out)
}
pub fn run_list(bytes: &[u8]) -> io::Result<Vec<Run>> {
    let (mut p, mut vcn, mut lcn) = (0, 0u64, 0i64);
    let mut out = vec![];
    loop {
        let header = *bytes
            .get(p)
            .ok_or_else(|| bad("unterminated mapping pairs"))?;
        p += 1;
        if header == 0 {
            break;
        }
        let n = (header & 15) as usize;
        let o = (header >> 4) as usize;
        if n == 0 || n > 8 || o > 8 {
            return Err(bad("mapping pair width"));
        }
        let mut size = [0; 8];
        size[..n].copy_from_slice(
            bytes
                .get(p..p + n)
                .ok_or_else(|| bad("mapping pair length"))?,
        );
        p += n;
        let clusters = u64::from_le_bytes(size);
        if clusters == 0 {
            return Err(bad("empty mapping pair"));
        }
        let physical = if o == 0 {
            None
        } else {
            let delta = bytes
                .get(p..p + o)
                .ok_or_else(|| bad("mapping pair offset"))?;
            p += o;
            let mut offset = if delta[o - 1] & 128 != 0 {
                [255; 8]
            } else {
                [0; 8]
            };
            offset[..o].copy_from_slice(delta);
            lcn = lcn
                .checked_add(i64::from_le_bytes(offset))
                .ok_or_else(|| bad("mapping pair overflow"))?;
            Some(u64::try_from(lcn).map_err(|_| bad("negative physical cluster"))?)
        };
        out.push(Run {
            vcn,
            lcn: physical,
            clusters,
        });
        vcn = vcn
            .checked_add(clusters)
            .ok_or_else(|| bad("virtual cluster overflow"))?;
    }
    Ok(out)
}
pub fn seconds(filetime: u64) -> u32 {
    (filetime / 10_000_000)
        .saturating_sub(11_644_473_600)
        .min(u32::MAX as u64) as u32
}

pub fn parse(id: u64, bytes: &[u8]) -> io::Result<Record> {
    parse_record(id, bytes, false)
}
// Only the successful NTFS FSCTL result may use this path. Kernel-cached
// records can contain updated sector trailers with an older on-disk USA.
// Keep raw-record validation separate; never relax it for raw disk input.
pub(crate) fn parse_kernel(id: u64, bytes: &[u8]) -> io::Result<Record> {
    parse_record(id, bytes, true)
}
fn parse_record(id: u64, bytes: &[u8], kernel: bool) -> io::Result<Record> {
    if bytes.get(..4) != Some(b"FILE") {
        return Err(bad("NTFS record signature"));
    }
    // FSCTL_GET_NTFS_FILE_RECORD returns a logical, kernel-validated record.
    // Its USA describes a disk transfer, not the current cached contents. Even
    // when a current trailer coincidentally equals the USA marker, applying an
    // old replacement would corrupt an attribute crossing that boundary.
    let fixed = if kernel {
        std::borrow::Cow::Borrowed(bytes)
    } else {
        fixups(bytes)?
    };
    let bytes = fixed.as_ref();
    let sequence = u16_at(bytes, 16)? as u64;
    if id >> 48 != 0 && id >> 48 != sequence {
        return Err(io::Error::new(
            if kernel {
                io::ErrorKind::NotFound
            } else {
                io::ErrorKind::InvalidData
            },
            "reused NTFS file reference",
        ));
    }
    let flags = u16_at(bytes, 22)?;
    if flags & 1 == 0 {
        return Err(io::Error::new(
            if kernel {
                io::ErrorKind::NotFound
            } else {
                io::ErrorKind::InvalidData
            },
            "unused NTFS file record",
        ));
    }
    let used = u32_at(bytes, 24)? as usize;
    let b = bytes.get(..used).ok_or_else(|| bad("NTFS record length"))?;
    let mut out = Record {
        id: (id & 0x0000_ffff_ffff_ffff) | sequence << 48,
        base: u64_at(b, 32)?,
        links: u16_at(b, 18)?,
        directory: flags & 2 != 0,
        ..Default::default()
    };
    let mut p = u16_at(b, 20)? as usize;
    if p < 42 || !p.is_multiple_of(8) {
        return Err(bad("NTFS attribute offset"));
    }
    loop {
        let code = u32_at(b, p)?;
        if code == u32::MAX {
            break;
        }
        let len = u32_at(b, p + 4)? as usize;
        if len < 24 || !len.is_multiple_of(8) {
            return Err(bad("NTFS attribute length"));
        }
        let attr = b
            .get(
                p..p.checked_add(len)
                    .ok_or_else(|| bad("attribute overflow"))?,
            )
            .ok_or_else(|| bad("NTFS attribute bounds"))?;
        let nonresident = attr[8];
        if nonresident > 1 {
            return Err(bad("NTFS attribute form"));
        }
        let value = if nonresident == 0 {
            let n = u32_at(attr, 16)? as usize;
            let start = u16_at(attr, 20)? as usize;
            if start < 24 {
                return Err(bad("resident value offset"));
            }
            Some(
                attr.get(
                    start
                        ..start
                            .checked_add(n)
                            .ok_or_else(|| bad("resident overflow"))?,
                )
                .ok_or_else(|| bad("resident value bounds"))?,
            )
        } else {
            None
        };
        match code {
            0x10 => {
                let v = value.ok_or_else(|| bad("nonresident standard information"))?;
                out.mtime = Some(seconds(u64_at(v, 8)?));
                out.attributes = Some(u32_at(v, 32)?);
            }
            0x20 => {
                if let Some(v) = value {
                    out.extensions.extend(attribute_references(v)?);
                } else {
                    if u64_at(attr, 16)? != 0 || u16_at(attr, 12)? & 255 != 0 {
                        return Err(bad(
                            "unsupported split/compressed attribute list; walker fallback",
                        ));
                    }
                    let start = u16_at(attr, 32)? as usize;
                    if start < 64 {
                        return Err(bad("mapping pair offset"));
                    }
                    out.attribute_lists.push(AttributeList {
                        size: u64_at(attr, 48)?,
                        runs: run_list(
                            attr.get(start..)
                                .ok_or_else(|| bad("mapping pair bounds"))?,
                        )?,
                    });
                }
            }
            0x30 => {
                let v = value.ok_or_else(|| bad("nonresident filename"))?;
                let n = *v.get(64).ok_or_else(|| bad("filename length"))? as usize;
                let namespace = *v.get(65).ok_or_else(|| bad("filename namespace"))?;
                if n == 0 || namespace > 3 {
                    return Err(bad("invalid NTFS filename"));
                }
                let wide: Vec<u16> = v
                    .get(66..66 + n * 2)
                    .ok_or_else(|| bad("filename bounds"))?
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                if wide.iter().any(|c| matches!(*c, 0 | 47 | 92)) {
                    return Err(bad("invalid filename separator"));
                }
                out.names.push(Name {
                    parent: u64_at(v, 0)?,
                    text: crate::paths::encode(&wide),
                    namespace,
                });
            }
            0x80 if attr[9] == 0 => {
                out.size = Some(if nonresident == 0 {
                    value.unwrap().len() as u64
                } else {
                    if u64_at(attr, 16)? == 0 {
                        u64_at(attr, 48)?
                    } else {
                        out.size.unwrap_or(0)
                    }
                });
            }
            0xc0 => {
                out.reparse_tag = value.map(|v| u32_at(v, 0)).transpose()?;
            }
            _ => {}
        }
        p += len;
    }
    Ok(out)
}
impl Record {
    pub fn merge(&mut self, other: Record) -> io::Result<()> {
        if other.base != self.id {
            return Err(bad("NTFS extension belongs to another file"));
        }
        self.names.extend(other.names);
        self.extensions.extend(other.extensions);
        self.attribute_lists.extend(other.attribute_lists);
        self.attributes = self.attributes.or(other.attributes);
        self.mtime = self.mtime.or(other.mtime);
        self.size = self.size.or(other.size);
        self.reparse_tag = self.reparse_tag.or(other.reparse_tag);
        Ok(())
    }
    pub fn finish(&mut self) -> io::Result<()> {
        // The header counts FILE_NAME attributes, including DOS aliases.
        // Check completeness before removing aliases from the user namespace.
        if !self.directory && self.names.len() < self.links as usize {
            return Err(bad(
                "incomplete NTFS hard-link names; walker fallback required",
            ));
        }
        if self.names.iter().any(|n| n.namespace == 2) {
            let long_parents: std::collections::HashSet<_> = self
                .names
                .iter()
                .filter(|n| n.namespace == 1 || n.namespace == 3)
                .map(|n| n.parent)
                .collect();
            self.names
                .retain(|n| n.namespace != 2 || !long_parents.contains(&n.parent));
        }
        self.names
            .sort_by(|a, b| a.parent.cmp(&b.parent).then(a.text.cmp(&b.text)));
        self.names
            .dedup_by(|a, b| a.parent == b.parent && a.text == b.text);
        if self.attributes.is_none() || self.mtime.is_none() {
            return Err(bad("incomplete NTFS standard information"));
        }
        Ok(())
    }
    pub fn kind(&self) -> u8 {
        let a = self.attributes.unwrap_or_default();
        let link = a & 0x400 != 0 && self.reparse_tag.is_none_or(|t| t & 0x2000_0000 != 0);
        (if link {
            crate::walk::KIND_LINK
        } else if self.directory {
            crate::walk::KIND_DIR
        } else {
            crate::walk::KIND_FILE
        }) | if a & 2 != 0 {
            crate::walk::FLAG_HIDDEN
        } else {
            0
        } | if a & 4 != 0 {
            crate::walk::FLAG_SYSTEM
        } else {
            0
        }
    }
}
fn fixups(bytes: &[u8]) -> io::Result<std::borrow::Cow<'_, [u8]>> {
    let start = u16_at(bytes, 4)? as usize;
    let count = u16_at(bytes, 6)? as usize;
    if count <= 1 {
        return Ok(std::borrow::Cow::Borrowed(bytes));
    }
    if start < 8 || start + count * 2 > bytes.len() || !bytes.len().is_multiple_of(count - 1) {
        return Err(bad("NTFS update sequence bounds"));
    }
    let sector = bytes.len() / (count - 1);
    if !(512..=4096).contains(&sector) || !sector.is_power_of_two() {
        return Err(bad("NTFS update sequence sector size"));
    }
    let marker = u16_at(bytes, start)?;
    let raw = (1..count).all(|i| u16_at(bytes, i * sector - 2).is_ok_and(|v| v == marker));
    let fixed =
        (1..count).all(|i| u16_at(bytes, i * sector - 2).ok() == u16_at(bytes, start + i * 2).ok());
    if !raw {
        if fixed {
            return Ok(std::borrow::Cow::Borrowed(bytes));
        }
        return Err(bad("torn NTFS file record"));
    }
    let mut out = bytes.to_vec();
    for i in 1..count {
        out[i * sector - 2..i * sector].copy_from_slice(&bytes[start + i * 2..start + i * 2 + 2]);
    }
    Ok(std::borrow::Cow::Owned(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn attr(code: u32, value: &[u8]) -> Vec<u8> {
        let len = (24 + value.len()).next_multiple_of(8);
        let mut a = vec![0; len];
        a[..4].copy_from_slice(&code.to_le_bytes());
        a[4..8].copy_from_slice(&(len as u32).to_le_bytes());
        a[16..20].copy_from_slice(&(value.len() as u32).to_le_bytes());
        a[20..22].copy_from_slice(&24u16.to_le_bytes());
        a[24..24 + value.len()].copy_from_slice(value);
        a
    }
    fn file(names: &[(u64, &[u16], u8)]) -> Vec<u8> {
        let mut b = vec![0; 48];
        b[..4].copy_from_slice(b"FILE");
        b[16..18].copy_from_slice(&7u16.to_le_bytes());
        b[18..20]
            .copy_from_slice(&(names.iter().filter(|n| n.2 != 2).count() as u16).to_le_bytes());
        b[20..22].copy_from_slice(&48u16.to_le_bytes());
        b[22..24].copy_from_slice(&1u16.to_le_bytes());
        let mut info = vec![0; 48];
        info[8..16]
            .copy_from_slice(&((11_644_473_600u64 + 1_700_000_000) * 10_000_000).to_le_bytes());
        info[32..36].copy_from_slice(&6u32.to_le_bytes());
        b.extend(attr(0x10, &info));
        for (parent, name, namespace) in names {
            let mut v = vec![0; 66];
            v[..8].copy_from_slice(&parent.to_le_bytes());
            v[64] = name.len() as u8;
            v[65] = *namespace;
            for c in *name {
                v.extend(c.to_le_bytes());
            }
            b.extend(attr(0x30, &v));
        }
        b.extend(attr(0x80, b"actual resident size"));
        b.extend(u32::MAX.to_le_bytes());
        let n = b.len() as u32;
        b[24..28].copy_from_slice(&n.to_le_bytes());
        b
    }
    #[test]
    fn metadata_all_hard_links_unicode_and_dos_alias() {
        let unicode: Vec<_> = "Résumé-🚀.txt".encode_utf16().collect();
        let raw = file(&[
            (22, &unicode, 1),
            (33, &[65, 0xd800, 66], 0),
            (22, &[82, 69, 83, 85, 77, 69, 126, 49], 2),
        ]);
        let mut r = parse(7 << 48 | 42, &raw).unwrap();
        r.finish().unwrap();
        assert_eq!(r.names.len(), 2);
        assert_eq!(crate::paths::decode(&r.names[1].text), [65, 0xd800, 66]);
        assert_eq!(r.size, Some(20));
        assert_eq!(r.mtime, Some(1_700_000_000));
        assert_eq!(
            r.kind(),
            crate::walk::FLAG_HIDDEN | crate::walk::FLAG_SYSTEM
        );
    }
    #[test]
    fn rejects_truncation_corruption_reused_ids_and_incomplete_links() {
        let raw = file(&[(22, &[65], 1)]);
        for n in 0..raw.len() {
            assert!(parse(7 << 48 | 42, &raw[..n]).is_err(), "prefix {n}");
        }
        assert!(parse(8 << 48 | 42, &raw).is_err());
        assert_eq!(
            parse_kernel(8 << 48 | 42, &raw).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            parse(8 << 48 | 42, &raw).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let mut bad = raw.clone();
        bad[52..56].copy_from_slice(&0u32.to_le_bytes());
        assert!(parse(7 << 48 | 42, &bad).is_err());
        let mut r = parse(7 << 48 | 42, &raw).unwrap();
        r.links = 2;
        assert!(r.finish().is_err());
    }
    #[test]
    fn cloud_metadata_is_regular_and_name_surrogates_are_links() {
        let mut r = Record {
            directory: true,
            attributes: Some(0x410),
            reparse_tag: Some(0x9000_001a),
            ..Default::default()
        };
        assert_eq!(r.kind(), 1);
        r.reparse_tag = Some(0xa000_000c);
        assert_eq!(r.kind(), 2);
    }
    #[test]
    fn accepts_raw_and_kernel_fixed_records_and_rejects_torn_sectors() {
        let mut raw = file(&[(22, &[65], 1)]);
        raw.resize(1024, 0);
        raw[4..6].copy_from_slice(&40u16.to_le_bytes());
        raw[6..8].copy_from_slice(&3u16.to_le_bytes());
        raw[40..42].copy_from_slice(&0xfafeu16.to_le_bytes());
        assert!(parse(7 << 48 | 42, &raw).is_ok());
        for p in [510, 1022] {
            raw[p..p + 2].copy_from_slice(&0xfafeu16.to_le_bytes());
        }
        assert!(parse(7 << 48 | 42, &raw).is_ok());
        raw[510..512].copy_from_slice(&0x3333u16.to_le_bytes());
        assert!(parse(7 << 48 | 42, &raw).is_err());
        // A checked FSCTL record is an in-memory record, not a raw-sector
        // transport. Do not overwrite newer data with a stale disk fixup.
        assert!(parse_kernel(7 << 48 | 42, &raw).is_ok());
    }
    #[test]
    fn kernel_record_keeps_filename_bytes_crossing_sector_boundary() {
        // The FILE_NAME value starts at byte 144; a long name crosses 510.
        let name: Vec<u16> = (0..240)
            .map(|i| if i == 150 { 0xfafe } else { 65 })
            .collect();
        let mut bytes = file(&[(22, &name, 1)]);
        bytes.resize(1024, 0);
        bytes[4..6].copy_from_slice(&40u16.to_le_bytes());
        bytes[6..8].copy_from_slice(&3u16.to_le_bytes());
        bytes[40..42].copy_from_slice(&0xfafeu16.to_le_bytes());
        // Stale on-disk replacements must never overwrite the logical name.
        bytes[42..46].copy_from_slice(&[66, 0, 67, 0]);
        bytes[1022..1024].copy_from_slice(&0xfafeu16.to_le_bytes());
        let actual = parse_kernel(7 << 48 | 42, &bytes).unwrap();
        assert_eq!(crate::paths::decode(&actual.names[0].text), name);
        for n in 0..bytes.len() {
            // Only the allocated padding may be absent from a logical record.
            if n < u32_at(&bytes, 24).unwrap() as usize {
                assert!(parse_kernel(7 << 48 | 42, &bytes[..n]).is_err());
            }
        }
    }
    #[test]
    fn mapping_pairs_support_fragmentation_negative_offsets_sparse_runs_and_bounds() {
        let runs = run_list(&[0x11, 3, 10, 0x11, 2, 0xfe, 0x01, 1, 0]).unwrap();
        assert_eq!(runs.len(), 3);
        assert_eq!(
            (runs[0].vcn, runs[0].lcn, runs[0].clusters),
            (0, Some(10), 3)
        );
        assert_eq!((runs[1].vcn, runs[1].lcn), (3, Some(8)));
        assert_eq!(runs[2].lcn, None);
        for bad in [
            &[0x11, 3][..],
            &[0x11, 3, 0xff, 0][..],
            &[0x11, 0, 10, 0][..],
            &[0x90, 3, 0][..],
        ] {
            assert!(run_list(bad).is_err());
        }
    }
}
