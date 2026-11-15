use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::archive::Archive;
use crate::debug_log;

/// Enough bytes to recognize every supported container signature.
const PROBE_BYTES: usize = 1024;
/// An empty ZIP (End of Central Directory only) is 22 bytes.
const MIN_ZIP_BYTES: u64 = 22;
const MAX_SHORTCUT_DEPTH: usize = 8;

const ZIP_LOCAL_HEADER: &[u8] = b"PK\x03\x04";
const ZIP_END_OF_CENTRAL_DIRECTORY: &[u8] = b"PK\x05\x06";
const ZIP_SPANNING: &[u8] = b"PK\x07\x08";
const RAR_MAGIC: &[u8] = b"Rar!\x1A\x07";
const SEVEN_ZIP_MAGIC: &[u8] = b"7z\xBC\xAF\x27\x1C";
const SHELL_LINK_HEADER: &[u8] = &[
    0x4c, 0x00, 0x00, 0x00, 0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc0, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x46,
];

/// Container format detected from the file contents and, for `.lnk`, the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Zip,
    Rar,
    SevenZip,
    ShellLink,
    Html,
    Empty,
    Truncated,
    Unknown,
}

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Self::Zip => "ZIP",
            Self::Rar => "RAR/CBR",
            Self::SevenZip => "7z",
            Self::ShellLink => "Windows shortcut",
            Self::Html => "HTML",
            Self::Empty => "empty",
            Self::Truncated => "truncated ZIP",
            Self::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// An opened archive with its path already resolved.
///
/// `Entry::open` follows Windows shortcuts, so callers can keep
/// `entry.path` and reopen it without resolving the shortcut again.
pub struct Entry {
    /// Path after following a Windows shortcut.
    pub path: PathBuf,
    /// Detected container format. A successfully opened entry is always ZIP.
    pub format: Format,
    pub archive: Archive,
}

impl Entry {
    /// Resolves a shortcut, detects the file format, and opens a ZIP archive.
    ///
    /// ZIP is the only supported container. RAR/CBR, 7z, HTML, empty, truncated,
    /// and unknown files get a format-specific error instead of a raw EOCD
    /// failure.
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut path = path.to_path_buf();
        for _ in 0..MAX_SHORTCUT_DEPTH {
            let probe = probe(&path)?;
            if probe.format != Format::ShellLink {
                return Self::from_probe(path, probe);
            }
            path = resolve_shortcut(&path)?;
        }
        Err(format!(
            "{}: too many nested Windows shortcuts",
            path.display()
        ))
    }

    fn from_probe(path: PathBuf, probe: Probe) -> Result<Self, String> {
        let Probe { file, len, format } = probe;
        if format == Format::Empty {
            return Err(format!("{}: file is empty", path.display()));
        }
        debug_log::log(format_args!(
            "open: {} ({} bytes, detected {format})",
            path.display(),
            len
        ));
        match Archive::from_file(file) {
            Ok(archive) => Ok(Self {
                path,
                format: Format::Zip,
                archive,
            }),
            Err(error) => Err(open_error(&path, format, len, &error)),
        }
    }

    pub fn page_count(&self) -> usize {
        self.archive.pages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.archive.pages.is_empty()
    }

    pub fn page_name(&self, page: usize) -> Option<&str> {
        self.archive.names.get(page).map(String::as_str)
    }

    pub fn read_page(&mut self, page: usize) -> Result<Vec<u8>, String> {
        let entry = *self
            .archive
            .pages
            .get(page)
            .ok_or_else(|| format!("page {} is out of range", page + 1))?;
        self.archive.read_index(entry)
    }
}

struct Probe {
    file: File,
    len: u64,
    format: Format,
}

fn probe(path: &Path) -> Result<Probe, String> {
    let mut file = File::open(path).map_err(|error| format!("open {}: {error}", path.display()))?;
    let len = file
        .metadata()
        .map_err(|error| format!("stat {}: {error}", path.display()))?
        .len();
    let mut prefix = Vec::with_capacity(PROBE_BYTES);
    file.by_ref()
        .take(PROBE_BYTES as u64)
        .read_to_end(&mut prefix)
        .map_err(|error| format!("read {}: {error}", path.display()))?;
    let format = detect_prefix(path, &prefix, len);
    file.seek(SeekFrom::Start(0))
        .map_err(|error| format!("seek {}: {error}", path.display()))?;
    Ok(Probe { file, len, format })
}

/// Detects the container format without opening the archive.
///
/// `Unknown` can still be a ZIP with leading data; [`Entry::open`] tries the
/// ZIP parser before returning a format error.
pub fn detect(path: &Path) -> Format {
    if is_shortcut(path) {
        return Format::ShellLink;
    }
    let Ok(metadata) = std::fs::metadata(path) else {
        return Format::Unknown;
    };
    let Ok(mut file) = File::open(path) else {
        return Format::Unknown;
    };
    let mut prefix = Vec::with_capacity(PROBE_BYTES);
    if file
        .by_ref()
        .take(PROBE_BYTES as u64)
        .read_to_end(&mut prefix)
        .is_err()
    {
        return Format::Unknown;
    }
    detect_prefix(path, &prefix, metadata.len())
}

fn detect_prefix(path: &Path, prefix: &[u8], len: u64) -> Format {
    if is_shortcut(path) || looks_like_shell_link(prefix) {
        return Format::ShellLink;
    }
    if len == 0 {
        return Format::Empty;
    }
    if prefix.starts_with(RAR_MAGIC) {
        return Format::Rar;
    }
    if prefix.starts_with(SEVEN_ZIP_MAGIC) {
        return Format::SevenZip;
    }
    if looks_like_html(prefix) {
        return Format::Html;
    }
    if starts_with_zip_signature(prefix) {
        return if len < MIN_ZIP_BYTES {
            Format::Truncated
        } else {
            Format::Zip
        };
    }
    if prefix.starts_with(b"PK") {
        return Format::Truncated;
    }
    Format::Unknown
}

fn starts_with_zip_signature(prefix: &[u8]) -> bool {
    [ZIP_LOCAL_HEADER, ZIP_END_OF_CENTRAL_DIRECTORY, ZIP_SPANNING]
        .iter()
        .any(|signature| prefix.starts_with(signature))
}

fn looks_like_shell_link(prefix: &[u8]) -> bool {
    prefix.starts_with(SHELL_LINK_HEADER)
}

fn looks_like_html(prefix: &[u8]) -> bool {
    let mut prefix = prefix;
    if prefix.starts_with(&[0xef, 0xbb, 0xbf]) {
        prefix = &prefix[3..];
    }
    let start = prefix
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(prefix.len());
    let prefix = &prefix[start..];
    [
        b"<!doctype html".as_slice(),
        b"<html".as_slice(),
        b"<head".as_slice(),
        b"<body".as_slice(),
    ]
    .iter()
    .any(|start| starts_with_ignore_ascii_case(prefix, start))
}

fn starts_with_ignore_ascii_case(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len() && haystack[..needle.len()].eq_ignore_ascii_case(needle)
}

fn is_shortcut(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
}

fn open_error(path: &Path, format: Format, len: u64, error: &str) -> String {
    let path = path.display();
    match format {
        Format::Rar => {
            format!("{path}: RAR/CBR archives are not supported; convert to ZIP/CBZ")
        }
        Format::SevenZip => format!("{path}: 7z archives are not supported; convert to ZIP/CBZ"),
        Format::Html => format!("{path}: looks like an HTML file, not a ZIP archive"),
        Format::Empty => format!("{path}: file is empty"),
        Format::Truncated => format!("{path}: truncated ZIP archive ({len} bytes): {error}"),
        Format::Zip => format!("{path}: invalid or truncated ZIP archive ({len} bytes): {error}"),
        Format::ShellLink => {
            format!("{path}: Windows shortcut does not point to a readable archive: {error}")
        }
        Format::Unknown => format!(
            "{path}: unsupported or unrecognized file format ({len} bytes); MangaNia reads ZIP/CBZ archives"
        ),
    }
}

#[cfg(windows)]
fn resolve_shortcut(path: &Path) -> Result<PathBuf, String> {
    debug_log::log(format_args!("shortcut: resolving {}", path.display()));
    let target = windows_shortcut_target(path)
        .map_err(|e| format!("resolve shortcut {}: {e}", path.display()))?;
    debug_log::log(format_args!("shortcut: target={}", target.display()));
    Ok(target)
}

#[cfg(windows)]
fn windows_shortcut_target(path: &Path) -> Result<PathBuf, String> {
    use std::ffi::{OsString, c_void};
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::ptr;

    use windows_sys::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows_sys::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize, STGM_READ,
    };
    use windows_sys::core::{GUID, HRESULT};

    const CLSID_SHELL_LINK: GUID = GUID {
        data1: 0x0002_1401,
        data2: 0,
        data3: 0,
        data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
    };
    const IID_ISHELL_LINK_W: GUID = GUID {
        data1: 0x0002_14f9,
        data2: 0,
        data3: 0,
        data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
    };
    const IID_IPERSIST_FILE: GUID = GUID {
        data1: 0x0000_010b,
        data2: 0,
        data3: 0,
        data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
    };

    type ComPtr = *mut c_void;

    unsafe fn release(ptr: ComPtr) {
        if ptr.is_null() {
            return;
        }
        let vtbl = unsafe { *(ptr as *mut *mut *mut c_void) };
        let release: unsafe extern "system" fn(ComPtr) -> u32 =
            unsafe { std::mem::transmute(*vtbl.add(2)) };
        unsafe { release(ptr) };
    }

    struct Guard(ComPtr);

    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe { release(self.0) };
        }
    }

    let init = unsafe { CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32) };
    if init < 0 && init != RPC_E_CHANGED_MODE {
        return Err(format!("CoInitializeEx failed: 0x{init:08X}"));
    }
    let uninit = init >= 0;

    let result = (|| {
        let mut link: ComPtr = ptr::null_mut();
        let hr = unsafe {
            CoCreateInstance(
                &CLSID_SHELL_LINK,
                ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID_ISHELL_LINK_W,
                &mut link,
            )
        };
        if hr < 0 {
            return Err(format!("CoCreateInstance failed: 0x{hr:08X}"));
        }
        let link = Guard(link);
        let link_ptr = link.0;

        let vtbl = unsafe { *(link_ptr as *mut *mut *mut c_void) };
        let query: unsafe extern "system" fn(ComPtr, *const GUID, *mut ComPtr) -> HRESULT =
            unsafe { std::mem::transmute(*vtbl.add(0)) };

        let mut persist: ComPtr = ptr::null_mut();
        let hr = unsafe { query(link_ptr, &IID_IPERSIST_FILE, &mut persist) };
        if hr < 0 {
            return Err(format!("IPersistFile query failed: 0x{hr:08X}"));
        }
        let persist = Guard(persist);
        let persist_ptr = persist.0;

        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let persist_vtbl = unsafe { *(persist_ptr as *mut *mut *mut c_void) };
        let load: unsafe extern "system" fn(ComPtr, *const u16, u32) -> HRESULT =
            unsafe { std::mem::transmute(*persist_vtbl.add(5)) };
        let hr = unsafe { load(persist_ptr, wide.as_ptr(), STGM_READ) };
        if hr < 0 {
            return Err(format!("IPersistFile::Load failed: 0x{hr:08X}"));
        }

        let get_path: unsafe extern "system" fn(
            ComPtr,
            *mut u16,
            i32,
            *mut c_void,
            u32,
        ) -> HRESULT = unsafe { std::mem::transmute(*vtbl.add(3)) };
        let mut buffer = vec![0u16; 32_768];
        let hr = unsafe {
            get_path(
                link_ptr,
                buffer.as_mut_ptr(),
                buffer.len() as i32,
                ptr::null_mut(),
                0,
            )
        };
        if hr < 0 {
            return Err(format!("IShellLink::GetPath failed: 0x{hr:08X}"));
        }

        let len = buffer
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(buffer.len());
        if len == 0 {
            return Err("shortcut target is empty".to_string());
        }
        Ok(PathBuf::from(OsString::from_wide(&buffer[..len])))
    })();

    if uninit {
        unsafe { CoUninitialize() };
    }
    result
}

#[cfg(not(windows))]
fn resolve_shortcut(path: &Path) -> Result<PathBuf, String> {
    Err(format!(
        "{} is a Windows shortcut (.lnk); drop the target file instead",
        path.display()
    ))
}
