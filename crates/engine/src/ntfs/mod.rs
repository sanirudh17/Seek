//! NTFS metadata enumeration and USN reads. Raw handles are confined to the
//! optional elevated helper; normal app/CLI processes use the walker fallback.
mod catalog;
mod layout;
pub mod record;
use crate::{
    kernel::{self, Cancel, Handle},
    walk::{self, Listing, NONE, RawEnt},
};
use record::{bad, u16_at, u32_at, u64_at};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    io,
};
use windows_sys::Win32::{Foundation::*, Storage::FileSystem::*, System::Ioctl::*};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Cursor {
    pub drive: u8,
    pub serial: u64,
    pub journal_id: u64,
    pub next_usn: i64,
}
impl Cursor {
    pub fn valid_for(&self, current: &Cursor, first_usn: i64) -> bool {
        self.drive == current.drive
            && self.serial == current.serial
            && self.journal_id == current.journal_id
            && self.next_usn >= first_usn
            && self.next_usn <= current.next_usn
    }
}
#[derive(Debug, Clone)]
pub struct JournalEntry {
    pub id: u64,
    pub parent: u64,
    pub usn: i64,
    pub reason: u32,
    pub attributes: u32,
    pub name: Vec<u8>,
}
pub fn entries(bytes: &[u8]) -> io::Result<Vec<JournalEntry>> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < bytes.len() {
        let b = &bytes[p..];
        let len = u32_at(b, 0)? as usize;
        if len < 60 || !len.is_multiple_of(8) || len > b.len() {
            return Err(bad("USN record length"));
        }
        let b = &b[..len];
        if u16_at(b, 4)? != 2 {
            return Err(bad("unsupported USN version; walker fallback required"));
        }
        let n = u16_at(b, 56)? as usize;
        let start = u16_at(b, 58)? as usize;
        if start < 60 || !n.is_multiple_of(2) || start + n > len {
            return Err(bad("USN name bounds"));
        }
        let wide: Vec<u16> = b[start..start + n]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        if wide.iter().any(|c| matches!(*c, 0 | 47 | 92)) {
            return Err(bad("USN filename separator"));
        }
        out.push(JournalEntry {
            id: u64_at(b, 8)?,
            parent: u64_at(b, 16)?,
            usn: u64_at(b, 24)? as i64,
            reason: u32_at(b, 40)?,
            attributes: u32_at(b, 52)?,
            name: crate::paths::encode(&wide),
        });
        p += len;
    }
    Ok(out)
}
pub struct NtfsSource {
    volume: Handle,
    layout_volumes: Vec<Handle>,
    record_count: u64,
    pub cancel: Cancel,
    pub drive: u8,
    pub serial: u64,
    record_size: usize,
    sector: usize,
    cluster: u64,
}
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ScanProfile {
    pub root: Vec<u8>,
    pub records: usize,
    pub directories: usize,
    pub metadata_ms: f64,
    pub namespace_ms: f64,
}
pub struct Scan {
    pub listings: Vec<Listing>,
    pub profile: ScanProfile,
}
impl NtfsSource {
    pub fn open(drive: u8, cancel: Cancel) -> io::Result<Self> {
        let drive = drive.to_ascii_uppercase();
        if !drive.is_ascii_uppercase() {
            return Err(bad("local drive letter required"));
        }
        let path: Vec<u16> = format!("\\\\.\\{}:", drive as char)
            .encode_utf16()
            .chain([0])
            .collect();
        let volume = unsafe {
            Handle::owned(CreateFileW(
                path.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                std::ptr::null_mut(),
            ))?
        };
        let mut b = vec![
            0;
            std::mem::size_of::<NTFS_VOLUME_DATA_BUFFER>()
                + std::mem::size_of::<NTFS_EXTENDED_VOLUME_DATA>()
        ];
        let n = kernel::control::<u8>(&volume, FSCTL_GET_NTFS_VOLUME_DATA, None, &mut b, &cancel)?;
        if n < 104 || u16_at(&b, 100)? != 3 {
            return Err(bad("only NTFS 3.x volumes are supported"));
        }
        let size = u32_at(&b, 48)? as usize;
        if !(512..=65536).contains(&size) {
            return Err(bad("NTFS file-record size"));
        }
        let sector = u32_at(&b, 40)? as usize;
        let cluster = u32_at(&b, 44)? as u64;
        if !(512..=65536).contains(&sector)
            || !sector.is_power_of_two()
            || cluster < sector as u64
            || !cluster.is_multiple_of(sector as u64)
        {
            return Err(bad("NTFS sector/cluster geometry"));
        }
        Ok(Self {
            layout_volumes: (0..3)
                .map(|_| unsafe {
                    Handle::owned(CreateFileW(
                        path.as_ptr(),
                        GENERIC_READ,
                        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                        std::ptr::null(),
                        OPEN_EXISTING,
                        FILE_FLAG_OVERLAPPED,
                        std::ptr::null_mut(),
                    ))
                })
                .collect::<io::Result<_>>()?,
            record_count: u64_at(&b, 56)? / size as u64,
            volume,
            cancel,
            drive,
            serial: u64_at(&b, 0)?,
            record_size: size,
            sector,
            cluster,
        })
    }
    pub fn cursor(&self) -> io::Result<(Cursor, i64)> {
        let mut b = vec![0; std::mem::size_of::<USN_JOURNAL_DATA_V0>()];
        let n = kernel::control::<u8>(
            &self.volume,
            FSCTL_QUERY_USN_JOURNAL,
            None,
            &mut b,
            &self.cancel,
        )?;
        if n < 56 {
            return Err(bad("short journal metadata"));
        }
        Ok((
            Cursor {
                drive: self.drive,
                serial: self.serial,
                journal_id: u64_at(&b, 0)?,
                next_usn: u64_at(&b, 16)? as i64,
            },
            (u64_at(&b, 8)? as i64).max(u64_at(&b, 24)? as i64),
        ))
    }
    fn record_one(&self, id: u64) -> io::Result<record::Record> {
        let input = NTFS_FILE_RECORD_INPUT_BUFFER {
            FileReferenceNumber: id as i64,
        };
        let mut b = vec![0; self.record_size + 16];
        let n = kernel::control(
            &self.volume,
            FSCTL_GET_NTFS_FILE_RECORD,
            Some(&input),
            &mut b,
            &self.cancel,
        )?;
        if n < 12 {
            return Err(bad("short NTFS record output"));
        }
        if u64_at(&b, 0)? & 0x0000_ffff_ffff_ffff != id & 0x0000_ffff_ffff_ffff {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "NTFS record disappeared during scan",
            ));
        }
        let len = u32_at(&b, 8)? as usize;
        if len != self.record_size || len > n - 12 {
            return Err(bad("NTFS record output length"));
        }
        record::parse_kernel(
            id,
            b[..n]
                .get(12..12 + len)
                .ok_or_else(|| bad("NTFS record output length"))?,
        )
    }
    fn record(&self, id: u64) -> io::Result<record::Record> {
        let mut r = self.record_one(id)?;
        let mut seen = HashSet::new();
        seen.insert(r.id);
        let mut p = 0;
        let mut list = 0;
        while p < r.extensions.len() || list < r.attribute_lists.len() {
            while list < r.attribute_lists.len() {
                r.extensions.extend(record::attribute_references(
                    &self.attribute_list(&r.attribute_lists[list])?,
                )?);
                list += 1;
            }
            if p == r.extensions.len() {
                break;
            }
            let ext = r.extensions[p];
            p += 1;
            if seen.insert(ext) {
                if seen.len() > 4096 {
                    return Err(bad("NTFS extension limit; walker fallback required"));
                }
                // A missing extension of a still-existing file is an incomplete
                // snapshot, not evidence that the whole file has disappeared.
                let extension = self.record_one(ext).map_err(|e| {
                    if e.kind() == io::ErrorKind::NotFound {
                        bad("NTFS extension changed during scan")
                    } else {
                        e
                    }
                })?;
                r.merge(extension)?;
            }
        }
        r.finish()?;
        Ok(r)
    }
    fn attribute_list(&self, list: &record::AttributeList) -> io::Result<Vec<u8>> {
        let size = usize::try_from(list.size).map_err(|_| bad("attribute list size"))?;
        if size > 16 * 1024 * 1024 {
            return Err(bad("large attribute list; walker fallback required"));
        }
        let mut out = vec![0; size];
        let mut covered = 0;
        for run in &list.runs {
            let start = usize::try_from(
                run.vcn
                    .checked_mul(self.cluster)
                    .ok_or_else(|| bad("attribute list offset"))?,
            )
            .map_err(|_| bad("attribute list offset"))?;
            if start >= size {
                break;
            }
            if start != covered {
                return Err(bad("attribute list mapping gap"));
            }
            let length = usize::try_from(
                run.clusters
                    .checked_mul(self.cluster)
                    .ok_or_else(|| bad("attribute list extent"))?
                    .min((size - start) as u64),
            )
            .map_err(|_| bad("attribute list extent"))?;
            if let Some(lcn) = run.lcn {
                let physical = lcn
                    .checked_mul(self.cluster)
                    .ok_or_else(|| bad("attribute list physical offset"))?;
                let rounded = length.next_multiple_of(self.sector);
                let mut buffer = vec![0; rounded + self.sector];
                let address = buffer.as_ptr() as usize;
                let shift = address.next_multiple_of(self.sector) - address;
                let aligned = &mut buffer[shift..shift + rounded];
                let n = kernel::read_at(&self.volume, physical, aligned, &self.cancel)?;
                if n < length {
                    return Err(bad("short attribute list read"));
                }
                out[start..start + length].copy_from_slice(&aligned[..length]);
            }
            covered = start + length;
        }
        if covered != size {
            return Err(bad("incomplete attribute list mapping"));
        }
        Ok(out)
    }
    fn layouts_range(
        &self,
        volume: &Handle,
        range: (u64, u64),
        mut visit: impl FnMut(Vec<record::Record>) -> io::Result<()>,
    ) -> io::Result<()> {
        let mut input: QUERY_FILE_LAYOUT_INPUT = unsafe { std::mem::zeroed() };
        input.Anonymous.NumberOfPairs = 1;
        input.FilterType = QUERY_FILE_LAYOUT_FILTER_TYPE_FILEID;
        input.Filter.FileReferenceRanges = [FILE_REFERENCE_RANGE {
            StartingFileReferenceNumber: range.0,
            EndingFileReferenceNumber: range.1,
        }];
        input.Flags = QUERY_FILE_LAYOUT_RESTART
            | QUERY_FILE_LAYOUT_INCLUDE_NAMES
            | QUERY_FILE_LAYOUT_INCLUDE_EXTRA_INFO
            | QUERY_FILE_LAYOUT_INCLUDE_STREAMS
            | QUERY_FILE_LAYOUT_INCLUDE_STREAMS_WITH_NO_CLUSTERS_ALLOCATED;
        // METHOD_NEITHER requires aligned input/output. Keep each batch bounded.
        let mut storage = vec![0u64; 2 * 1024 * 1024 / 8];
        loop {
            let b = unsafe {
                std::slice::from_raw_parts_mut(storage.as_mut_ptr() as *mut u8, storage.len() * 8)
            };
            let n = match kernel::control(
                volume,
                FSCTL_QUERY_FILE_LAYOUT,
                Some(&input),
                b,
                &self.cancel,
            ) {
                Ok(n) => n,
                Err(e) if e.raw_os_error() == Some(ERROR_HANDLE_EOF as i32) => return Ok(()),
                Err(e)
                    if e.raw_os_error() == Some(ERROR_INSUFFICIENT_BUFFER as i32)
                        && storage.len() * 8 < 16 * 1024 * 1024 =>
                {
                    storage.resize(storage.len() * 2, 0);
                    continue;
                }
                Err(e) => return Err(e),
            };
            let mut records = Vec::new();
            layout::decode(
                b.get(..n).ok_or_else(|| bad("NTFS layout output length"))?,
                |record| {
                    let ordinal = record.id & 0x0000_ffff_ffff_ffff;
                    if ordinal < range.0 || ordinal > range.1 {
                        return Err(bad("NTFS layout escaped its record range"));
                    }
                    records.push(record);
                    Ok(())
                },
            )?;
            visit(records)?;
            input.Flags &= !QUERY_FILE_LAYOUT_RESTART;
        }
    }
    fn layouts(&self, mut visit: impl FnMut(record::Record) -> io::Result<()>) -> io::Result<()> {
        let volumes: Vec<_> = std::iter::once(&self.volume)
            .chain(&self.layout_volumes)
            .collect();
        let ranges = record_ranges(self.record_count, volumes.len())?;
        std::thread::scope(|scope| {
            // Independent handles retain separate native enumeration cursors.
            // Bound buffered batches and join all workers before publication.
            let (output, input) = std::sync::mpsc::sync_channel(4);
            for (volume, range) in volumes.into_iter().zip(ranges) {
                let output = output.clone();
                scope.spawn(move || {
                    crate::qos::utility();
                    if let Err(error) = self.layouts_range(volume, range, |records| {
                        output.send(Ok(records)).map_err(io::Error::other)
                    }) {
                        let _ = output.send(Err(error));
                    }
                });
            }
            drop(output);
            for records in input {
                for record in records? {
                    visit(record)?;
                }
            }
            Ok(())
        })
    }
    pub fn scan(&self, root: &[u8]) -> io::Result<Scan> {
        let root_id = file_id(root, FILE_LIST_DIRECTORY)?;
        let root_record = self.record(root_id)?;
        if root_record.attributes.unwrap_or(0) & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(bad(
                "Provider-backed root requires a directory listing; USN replay remains available",
            ));
        }
        // One kernel metadata pass supplies both the directory graph and files.
        // Do not issue a separate FSCTL for each of hundreds of thousands of
        // directories. The temporary catalog stays inside the helper; only
        // descendants authorized with the client's token can be published.
        let mut catalog = catalog::Catalog::default();
        let metadata = std::time::Instant::now();
        let mut records = 0;
        self.layouts(|record| {
            records += 1;
            catalog.add(record)
        })?;
        catalog.ensure_volume_root(root_record)?;
        let metadata_ms = metadata.elapsed().as_secs_f64() * 1000.;
        let namespace = std::time::Instant::now();
        let token = crate::helper::duplicate_client_token()?;
        let scoped = std::thread::scope(|scope| {
            let (jobs, input) = std::sync::mpsc::sync_channel::<(usize, u64, Vec<u8>)>(256);
            let input = std::sync::Arc::new(std::sync::Mutex::new(input));
            let (results, completed) = std::sync::mpsc::channel();
            let (ready, initialized) = std::sync::mpsc::channel();
            // Native directory opens block in filesystem/filter drivers. Keep
            // the pool bounded, below normal priority, and destroy it at Ready.
            const WORKERS: usize = 16;
            for _ in 0..WORKERS {
                let input = input.clone();
                let results = results.clone();
                let ready = ready.clone();
                let token = &token;
                scope.spawn(move || {
                    crate::qos::utility();
                    let _user = match crate::helper::Impersonation::token(token) {
                        Ok(user) => user,
                        Err(error) => {
                            let _ = ready.send(Err(error));
                            return;
                        }
                    };
                    let _ = ready.send(Ok(()));
                    loop {
                        let job = input.lock().unwrap().recv();
                        let Ok((ordinal, id, path)) = job else {
                            break;
                        };
                        let access = if self.cancel.cancelled() {
                            Err(io::Error::new(
                                io::ErrorKind::Interrupted,
                                "directory access cancelled",
                            ))
                        } else {
                            directory_access(&self.volume, id, &path)
                        };
                        if results.send((ordinal, access)).is_err() {
                            break;
                        }
                    }
                });
            }
            drop(ready);
            drop(results);
            let result = (|| -> io::Result<_> {
                for _ in 0..WORKERS {
                    initialized.recv().map_err(io::Error::other)??;
                }
                catalog.scope(
                    root_id,
                    root,
                    || self.cancel.cancelled(),
                    |paths| {
                        for (ordinal, &(id, path)) in paths.iter().enumerate() {
                            if jobs.send((ordinal, id, path.to_vec())).is_err() {
                                // Incomplete work is a failed scan, never an
                                // apparent directory permission denial.
                                return vec![];
                            }
                        }
                        let mut ordered: Vec<_> =
                            std::iter::repeat_with(|| None).take(paths.len()).collect();
                        for _ in paths {
                            match completed.recv() {
                                Ok((ordinal, result)) => ordered[ordinal] = Some(result),
                                Err(_) => {
                                    return vec![];
                                }
                            }
                        }
                        ordered.into_iter().map(|r| r.unwrap()).collect()
                    },
                    crate::live::lstat,
                    |path| walk::scan(path, 4),
                )
            })();
            drop(jobs);
            result
        })?;
        let profile = ScanProfile {
            root: root.to_vec(),
            records,
            directories: scoped.listings.len(),
            metadata_ms,
            namespace_ms: namespace.elapsed().as_secs_f64() * 1000.,
        };
        walk::DENIED.lock().unwrap().extend(scoped.denied);
        Ok(Scan {
            listings: scoped.listings,
            profile,
        })
    }
    pub fn read_journal(
        &self,
        cursor: &Cursor,
        wait: bool,
    ) -> io::Result<(Cursor, Vec<JournalEntry>)> {
        let (current, first) = self.cursor()?;
        if !cursor.valid_for(&current, first) {
            return Err(bad("USN journal history lost or volume replaced"));
        }
        let input = READ_USN_JOURNAL_DATA_V0 {
            StartUsn: cursor.next_usn,
            ReasonMask: u32::MAX,
            ReturnOnlyOnClose: 0,
            Timeout: 0,
            BytesToWaitFor: if wait { 1 } else { 0 },
            UsnJournalID: cursor.journal_id,
        };
        let mut b = vec![0; 64 * 1024];
        let n = kernel::control(
            &self.volume,
            FSCTL_READ_USN_JOURNAL,
            Some(&input),
            &mut b,
            &self.cancel,
        )?;
        if n < 8 {
            return Err(bad("short USN journal read"));
        }
        let mut next = cursor.clone();
        next.next_usn = u64_at(&b, 0)? as i64;
        if next.next_usn < cursor.next_usn {
            return Err(bad("USN cursor moved backwards"));
        }
        Ok((next, entries(&b[8..n])?))
    }
    pub fn changed_directories(
        &self,
        events: &[JournalEntry],
        roots: &[Vec<u8>],
    ) -> io::Result<Vec<Vec<u8>>> {
        let mut dirs = BTreeSet::new();
        let mut resolved = HashMap::new();
        let history: HashMap<_, _> = events
            .iter()
            .filter(|event| event.attributes & FILE_ATTRIBUTE_DIRECTORY != 0)
            .map(|event| (event.id, event))
            .collect();
        let mut files = HashSet::new();
        let gone: HashSet<_> = events
            .iter()
            .filter(|e| e.reason & USN_REASON_FILE_DELETE != 0)
            .map(|e| e.id)
            .collect();
        let mut parent = |id| -> io::Result<Vec<u8>> {
            if let Some(path) = resolved.get(&id) {
                return Ok(Vec::clone(path));
            }
            let path = resolve_journal_parent(id, &history, &mut resolved, &mut |id| {
                self.resolve_directory(id).or_else(|error| {
                    // OpenFileById can also return ERROR_INVALID_PARAMETER for
                    // historical IDs. Validate the exact record sequence through
                    // the authorized volume handle before accepting any path.
                    // Cancellation must propagate immediately.
                    match error.kind() {
                        io::ErrorKind::ConnectionAborted
                        | io::ErrorKind::Interrupted
                        | io::ErrorKind::TimedOut => Err(error),
                        _ => self.record_path(id),
                    }
                })
            })?;
            Ok(path)
        };
        let scoped = |path: &[u8]| {
            roots.iter().any(|root| {
                path.len() >= root.len()
                    && crate::paths::cmp_ci(&path[..root.len()], root).is_eq()
                    && (path.len() == root.len() || path.get(root.len()) == Some(&b'/'))
            })
        };
        for e in events {
            if e.id & 0x0000_ffff_ffff_ffff < 16 {
                continue;
            }
            // Both old/new parent records are emitted for moves. Resolve parents
            // by file ID, never by a bounded cache that could silently forget one.
            let parent_path = parent(e.parent)?;
            if scoped(&parent_path) {
                dirs.insert(parent_path.clone());
            }
            // Include the directory itself for permission/reparse changes too:
            // previously denied children must be discovered once access returns.
            if e.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                let path = crate::live::join(&parent_path, &e.name);
                if scoped(&path) {
                    dirs.insert(path);
                }
            }
            if e.attributes & FILE_ATTRIBUTE_DIRECTORY == 0
                && !gone.contains(&e.id)
                && files.insert(e.id)
            {
                // Coalesce repeated records for a file within this batch.
                // Keep all remaining hard-link parents even when the journal's
                // primary name is outside the selected roots. The event itself
                // supplies the parent of a removed/renamed link.
                let record = match self.record(e.id) {
                    Ok(record) => record,
                    // The event parent was already retained. If the base record
                    // has since been deleted/reused it has no current hard links
                    // to refresh; each removed name has its own journal parent.
                    Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e),
                };
                for name in record.names {
                    let path = parent(name.parent)?;
                    if scoped(&path) {
                        dirs.insert(path);
                    }
                }
            }
        }
        Ok(dirs.into_iter().collect())
    }
    fn record_path(&self, mut id: u64) -> io::Result<Vec<u8>> {
        let mut names = Vec::new();
        let mut seen = HashSet::new();
        while id & 0x0000_ffff_ffff_ffff != 5 {
            if !seen.insert(id) || seen.len() > 1024 {
                return Err(bad("NTFS parent cycle/depth"));
            }
            let r = self.record(id)?;
            if !r.directory {
                return Err(bad("NTFS parent is not a directory"));
            }
            let name = r
                .names
                .into_iter()
                .next()
                .ok_or_else(|| bad("NTFS parent has no name"))?;
            names.push(name.text);
            id = name.parent;
        }
        let mut path = vec![b'/', self.drive, b':'];
        for name in names.into_iter().rev() {
            path = crate::live::join(&path, &name);
        }
        Ok(path)
    }
    fn resolve_directory(&self, id: u64) -> io::Result<Vec<u8>> {
        let descriptor = FILE_ID_DESCRIPTOR {
            dwSize: std::mem::size_of::<FILE_ID_DESCRIPTOR>() as u32,
            Type: FileIdType,
            Anonymous: FILE_ID_DESCRIPTOR_0 { FileId: id as i64 },
        };
        let handle = unsafe {
            Handle::owned(OpenFileById(
                self.volume.raw(),
                &descriptor,
                FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            ))?
        };
        let mut b = vec![0u16; 32768];
        let n =
            unsafe { GetFinalPathNameByHandleW(handle.raw(), b.as_mut_ptr(), b.len() as u32, 0) };
        if n == 0 {
            return Err(io::Error::last_os_error());
        }
        if n as usize >= b.len() {
            return Err(bad("resolved NTFS path too long"));
        }
        use std::os::windows::ffi::OsStringExt;
        Ok(crate::paths::internal(&std::path::PathBuf::from(
            std::ffi::OsString::from_wide(&b[..n as usize]),
        )))
    }
}
impl walk::IndexSource for NtfsSource {
    fn scan(&self, root: &[u8], _: usize) -> io::Result<Vec<Listing>> {
        self.scan(root).map(|scan| scan.listings)
    }
    fn list(&self, path: &[u8]) -> Option<Listing> {
        walk::list_one(path)
    }
}
fn record_ranges(count: u64, workers: usize) -> io::Result<Vec<(u64, u64)>> {
    const END: u64 = (1 << 48) - 1;
    if !(16..=END + 1).contains(&count) || !(1..=4).contains(&workers) {
        return Err(bad("NTFS record range geometry"));
    }
    let step = count.div_ceil(workers as u64);
    Ok((0..workers)
        .map(|n| {
            let start = n as u64 * step;
            // Cover the complete namespace, including records allocated after
            // volume geometry was read but before the scan's USN cursor was saved.
            let end = if n + 1 == workers {
                END
            } else {
                start + step - 1
            };
            (start, end)
        })
        .collect())
}
fn resolve_journal_parent(
    id: u64,
    history: &HashMap<u64, &JournalEntry>,
    resolved: &mut HashMap<u64, Vec<u8>>,
    current: &mut impl FnMut(u64) -> io::Result<Vec<u8>>,
) -> io::Result<Vec<u8>> {
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    let mut ancestor = id;
    let mut path = loop {
        if let Some(path) = resolved.get(&ancestor) {
            break path.clone();
        }
        if !seen.insert(ancestor) || seen.len() > 1024 {
            return Err(bad("historical NTFS parent cycle/depth"));
        }
        match current(ancestor) {
            Ok(path) => {
                resolved.insert(ancestor, path.clone());
                break path;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // A deleted/reused directory still has its old identity, name
                // and parent in the complete replay. Recover that old path so
                // the client removes stale descendants without a drive scan.
                let event = history.get(&ancestor).ok_or(error)?;
                chain.push((ancestor, event.name.as_slice()));
                ancestor = event.parent;
            }
            Err(error) => return Err(error),
        }
    };
    for (id, name) in chain.into_iter().rev() {
        path = crate::live::join(&path, name);
        resolved.insert(id, path.clone());
    }
    Ok(path)
}
fn open_directory_id(volume: &Handle, id: u64) -> io::Result<Handle> {
    let descriptor = FILE_ID_DESCRIPTOR {
        dwSize: std::mem::size_of::<FILE_ID_DESCRIPTOR>() as u32,
        Type: FileIdType,
        Anonymous: FILE_ID_DESCRIPTOR_0 { FileId: id as i64 },
    };
    unsafe {
        Handle::owned(OpenFileById(
            volume.raw(),
            &descriptor,
            FILE_LIST_DIRECTORY,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        ))
    }
}
fn directory_access(volume: &Handle, id: u64, path: &[u8]) -> io::Result<u64> {
    // OpenFileById identifies the object by its full file reference, including
    // its sequence, and checks the requested rights under the impersonated
    // client token. There is no need to query the same identity again.
    open_directory_id(volume, id)
        .map(|_| id)
        .or_else(|_| file_id(path, FILE_LIST_DIRECTORY))
}
pub fn file_id(path: &[u8], access: u32) -> io::Result<u64> {
    let handle = unsafe {
        Handle::owned(CreateFileW(
            crate::paths::extended(path).as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        ))?
    };
    handle_file_id(&handle)
}
fn handle_file_id(handle: &Handle) -> io::Result<u64> {
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(handle.raw(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((info.nFileIndexHigh as u64) << 32 | info.nFileIndexLow as u64)
}
fn graft_subtree(out: &mut Vec<Listing>, target: u32, mut listings: Vec<Listing>) {
    let offset = out.len() as u32;
    for listing in &mut listings {
        listing.id = if listing.id == 0 {
            target
        } else {
            offset + listing.id - 1
        };
        for e in &mut listing.ents {
            if e.child != NONE {
                e.child = offset + e.child - 1;
            }
        }
    }
    if let Some(pos) = listings.iter().position(|l| l.id == target) {
        out[target as usize] = listings.swap_remove(pos);
    }
    listings.sort_by_key(|l| l.id);
    out.extend(listings);
}
fn insert(
    listing: &mut Listing,
    name: &[u8],
    kind: u8,
    size: u64,
    mtime: u32,
    child: u32,
) -> io::Result<()> {
    let name_len = u16::try_from(name.len()).map_err(|_| bad("NTFS name too long"))?;
    let name_off =
        u32::try_from(listing.names.len()).map_err(|_| bad("NTFS directory too large"))?;
    listing.ents.push(RawEnt {
        name_off,
        name_len,
        kind,
        size,
        mtime,
        child,
    });
    listing.names.extend(name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn record_partitions_cover_every_ordinal_including_volume_growth() {
        for count in [16, 17, 1000, 3_026_176, 1 << 48] {
            for workers in 1..=4 {
                let ranges = record_ranges(count, workers).unwrap();
                assert_eq!(ranges[0].0, 0);
                assert_eq!(ranges.last().unwrap().1, (1 << 48) - 1);
                for pair in ranges.windows(2) {
                    assert_eq!(pair[0].1 + 1, pair[1].0);
                }
                assert!(ranges.iter().all(|(start, end)| start <= end));
            }
        }
        for (count, workers) in [(0, 4), (15, 4), (1 << 49, 4), (1000, 0), (1000, 5)] {
            assert!(record_ranges(count, workers).is_err());
        }
    }
    #[test]
    fn historical_parent_chain_recovers_deleted_paths_but_preserves_current_identity() {
        let old = JournalEntry {
            id: 20,
            parent: 5,
            usn: 9,
            reason: USN_REASON_FILE_DELETE,
            attributes: FILE_ATTRIBUTE_DIRECTORY,
            name: b"removed".to_vec(),
        };
        let nested = JournalEntry {
            id: 21,
            parent: 20,
            usn: 8,
            reason: USN_REASON_FILE_DELETE,
            attributes: FILE_ATTRIBUTE_DIRECTORY,
            name: b"nested".to_vec(),
        };
        let history = HashMap::from([(20, &old), (21, &nested)]);
        let mut resolved = HashMap::new();
        let path = resolve_journal_parent(21, &history, &mut resolved, &mut |id| {
            if id == 5 {
                Ok(b"/C:".to_vec())
            } else {
                Err(io::Error::new(io::ErrorKind::NotFound, "deleted or reused"))
            }
        })
        .unwrap();
        assert_eq!(path, b"/C:/removed/nested");
        assert_eq!(resolved[&20], b"/C:/removed");
        let path = resolve_journal_parent(21, &history, &mut HashMap::new(), &mut |_| {
            Ok(b"/C:/renamed/current".to_vec())
        })
        .unwrap();
        assert_eq!(path, b"/C:/renamed/current");
        assert_eq!(
            resolve_journal_parent(21, &history, &mut HashMap::new(), &mut |_| Err(bad(
                "corrupt record"
            )))
            .unwrap_err()
            .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            resolve_journal_parent(22, &history, &mut HashMap::new(), &mut |_| Err(
                io::Error::new(io::ErrorKind::NotFound, "unknown parent")
            ))
            .unwrap_err()
            .kind(),
            io::ErrorKind::NotFound
        );
        let cycle = JournalEntry {
            parent: 21,
            ..nested
        };
        let history = HashMap::from([(21, &cycle)]);
        assert!(
            resolve_journal_parent(21, &history, &mut HashMap::new(), &mut |_| Err(
                io::Error::new(io::ErrorKind::NotFound, "deleted")
            ))
            .is_err()
        );
    }
    #[test]
    fn native_id_open_checks_sequence_and_preserves_identity_across_rename() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("before");
        std::fs::create_dir(&folder).unwrap();
        let path = crate::paths::internal(&folder);
        let id = file_id(&path, FILE_LIST_DIRECTORY).unwrap();
        let volume_hint = unsafe {
            Handle::owned(CreateFileW(
                crate::paths::extended(&crate::paths::internal(temp.path())).as_ptr(),
                FILE_LIST_DIRECTORY,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            ))
            .unwrap()
        };
        assert_eq!(
            handle_file_id(&open_directory_id(&volume_hint, id).unwrap()).unwrap(),
            id
        );
        assert!(open_directory_id(&volume_hint, id ^ (1u64 << 48)).is_err());
        let renamed = temp.path().join("after");
        std::fs::rename(&folder, &renamed).unwrap();
        assert_eq!(
            handle_file_id(&open_directory_id(&volume_hint, id).unwrap()).unwrap(),
            id
        );
        assert_eq!(directory_access(&volume_hint, id, &path).unwrap(), id);
        drop(volume_hint);
        std::fs::remove_dir(&renamed).unwrap();
        assert!(open_directory_id_after_deletion(temp.path(), id).is_err());
    }
    fn open_directory_id_after_deletion(root: &std::path::Path, id: u64) -> io::Result<Handle> {
        let volume_hint = unsafe {
            Handle::owned(CreateFileW(
                crate::paths::extended(&crate::paths::internal(root)).as_ptr(),
                FILE_LIST_DIRECTORY,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            ))?
        };
        open_directory_id(&volume_hint, id)
    }
    #[test]
    fn provider_subtree_preserves_siblings_and_remaps_all_descendants() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("nested/deeper")).unwrap();
        std::fs::write(temp.path().join("nested/deeper/cloud-🚀.txt"), b"metadata").unwrap();
        let mut out: Vec<_> = (0..3)
            .map(|id| Listing {
                id,
                names: vec![],
                ents: vec![],
            })
            .collect();
        insert(&mut out[0], b"provider", walk::KIND_DIR, 0, 0, 1).unwrap();
        insert(&mut out[0], b"ordinary", walk::KIND_DIR, 0, 0, 2).unwrap();
        insert(&mut out[2], b"sibling.txt", walk::KIND_FILE, 1, 0, NONE).unwrap();
        graft_subtree(
            &mut out,
            1,
            walk::scan(&crate::paths::internal(temp.path()), 2),
        );
        assert_eq!(out.len(), 5);
        assert_eq!(out[2].names, b"sibling.txt");
        for (id, listing) in out.iter().enumerate() {
            assert_eq!(listing.id as usize, id);
            for e in &listing.ents {
                assert!(e.child == NONE || (e.child as usize) < out.len());
            }
        }
        assert_eq!(out[1].ents[0].child, 3);
        assert_eq!(out[3].ents[0].child, 4);
        assert_eq!(out[4].names, "cloud-🚀.txt".as_bytes());
    }
    #[test]
    fn journal_records_validate_versions_lengths_and_utf16() {
        let mut b = vec![0; 72];
        b[..4].copy_from_slice(&72u32.to_le_bytes());
        b[4..6].copy_from_slice(&2u16.to_le_bytes());
        b[8..16].copy_from_slice(&42u64.to_le_bytes());
        b[56..58].copy_from_slice(&6u16.to_le_bytes());
        b[58..60].copy_from_slice(&60u16.to_le_bytes());
        b[60..66].copy_from_slice(&[65, 0, 0, 216, 66, 0]);
        assert_eq!(
            crate::paths::decode(&entries(&b).unwrap()[0].name),
            [65, 0xd800, 66]
        );
        for n in 0..b.len() {
            if n != 0 {
                assert!(entries(&b[..n]).is_err());
            }
        }
        b[4] = 3;
        assert!(entries(&b).is_err());
    }
    #[test]
    fn journal_reset_wrap_volume_replacement_and_ahead_cursor_are_rejected() {
        let current = Cursor {
            drive: b'C',
            serial: 5,
            journal_id: 8,
            next_usn: 100,
        };
        let mut saved = current.clone();
        saved.next_usn = 80;
        assert!(saved.valid_for(&current, 50));
        assert!(!saved.valid_for(&current, 90));
        saved.journal_id = 9;
        assert!(!saved.valid_for(&current, 50));
        saved = current.clone();
        saved.serial = 6;
        assert!(!saved.valid_for(&current, 50));
        saved = current.clone();
        saved.next_usn = 101;
        assert!(!saved.valid_for(&current, 50));
    }
}
