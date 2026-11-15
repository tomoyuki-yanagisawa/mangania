use std::cmp::Ordering;
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use zip::ZipArchive;

use crate::debug_log;

const MAX_ENTRIES: usize = 100_000;
const MAX_ENTRY_BYTES: u64 = 512 * 1024 * 1024;

pub struct Archive {
    zip: ZipArchive<File>,
    pub names: Vec<String>,
    pub pages: Vec<usize>,
}

impl Archive {
    pub fn open(path: &Path) -> Result<Self, String> {
        let path = resolve_shortcut(path)?;
        debug_log::log(format_args!("archive: open {}", path.display()));
        let file = File::open(&path).map_err(|e| format!("open {}: {e}", path.display()))?;
        let file_len = file
            .metadata()
            .map_err(|e| format!("stat {}: {e}", path.display()))?
            .len();
        debug_log::log(format_args!("archive: size={file_len} bytes"));
        let zip = ZipArchive::new(file)
            .map_err(|e| format!("read zip {} ({file_len} bytes): {e}", path.display()))?;
        if zip.len() > MAX_ENTRIES {
            return Err(format!("too many zip entries: {}", zip.len()));
        }

        let mut archive = Self {
            zip,
            names: Vec::new(),
            pages: Vec::new(),
        };
        let mut pages = Vec::new();
        for entry in 0..archive.zip.len() {
            // Skip entries whose local header or name is unreadable instead of
            // failing the whole archive.
            let Ok(file) = archive.zip.by_index(entry) else {
                continue;
            };
            if !file.is_file() {
                continue;
            }
            let Ok(name) = file.name() else {
                continue;
            };
            if !is_page_name(&name) || file.size() > MAX_ENTRY_BYTES {
                continue;
            }
            pages.push((name.into_owned(), entry));
        }

        pages.sort_by(|a, b| natural_cmp(&a.0, &b.0));
        let (names, pages): (Vec<_>, Vec<_>) = pages.into_iter().unzip();
        debug_log::log(format_args!(
            "archive: entries={} pages={}",
            archive.zip.len(),
            pages.len()
        ));
        archive.names = names;
        archive.pages = pages;
        Ok(archive)
    }

    pub fn read_index(&mut self, entry: usize) -> Result<Vec<u8>, String> {
        let file = self
            .zip
            .by_index(entry)
            .map_err(|e| format!("entry {entry}: {e}"))?;
        if file.size() > MAX_ENTRY_BYTES {
            return Err(format!("entry {entry} is too large"));
        }
        debug_log::log(format_args!(
            "archive: entry {entry} declared_size={}",
            file.size()
        ));

        // `size()` is metadata from the archive; never trust it as an allocation bound.
        let mut data = Vec::new();
        file.take(MAX_ENTRY_BYTES + 1)
            .read_to_end(&mut data)
            .map_err(|e| format!("extract entry {entry}: {e}"))?;
        if data.len() as u64 > MAX_ENTRY_BYTES {
            return Err(format!("entry {entry} exceeds the size limit"));
        }
        Ok(data)
    }
}

fn is_shortcut(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
}

#[cfg(windows)]
fn resolve_shortcut(path: &Path) -> Result<PathBuf, String> {
    if !is_shortcut(path) {
        return Ok(path.to_path_buf());
    }
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
    if is_shortcut(path) {
        Err(format!(
            "{} is a Windows shortcut (.lnk); drop the target file instead",
            path.display()
        ))
    } else {
        Ok(path.to_path_buf())
    }
}

fn is_page_name(name: &str) -> bool {
    if !is_safe_name(name) {
        return false;
    }
    let path = Path::new(name);
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(extension.as_str(), "jpg" | "jpeg" | "png" | "webp" | "bmp")
}

fn is_safe_name(name: &str) -> bool {
    if name.is_empty() || name.contains('\\') || name.contains('\0') {
        return false;
    }
    let path = Path::new(name);
    if path.is_absolute() {
        return false;
    }
    path.components().all(|component| match component {
        Component::Normal(part) => {
            let part = part.to_string_lossy();
            !part.starts_with('.') && part != "__MACOSX"
        }
        Component::CurDir => true,
        Component::ParentDir | Component::RootDir | Component::Prefix(_) => false,
    })
}

/// Orders names by their digit runs so `2.jpg` comes before `10.jpg`.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let a: Vec<char> = a.chars().flat_map(char::to_lowercase).collect();
    let b: Vec<char> = b.chars().flat_map(char::to_lowercase).collect();
    let (mut i, mut j) = (0, 0);

    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let end_a = digit_end(&a, i);
            let end_b = digit_end(&b, j);
            let start_a = skip_zeroes(&a, i, end_a);
            let start_b = skip_zeroes(&b, j, end_b);
            let len_a = end_a - start_a;
            let len_b = end_b - start_b;

            let ordering = len_a
                .cmp(&len_b)
                .then_with(|| a[start_a..end_a].cmp(&b[start_b..end_b]));
            if ordering != Ordering::Equal {
                return ordering;
            }
            i = end_a;
            j = end_b;
        } else {
            let ordering = a[i].cmp(&b[j]);
            if ordering != Ordering::Equal {
                return ordering;
            }
            i += 1;
            j += 1;
        }
    }

    a[i..].cmp(&b[j..])
}

fn digit_end(chars: &[char], start: usize) -> usize {
    let mut end = start;
    while end < chars.len() && chars[end].is_ascii_digit() {
        end += 1;
    }
    end
}

fn skip_zeroes(chars: &[char], start: usize, end: usize) -> usize {
    let mut start = start;
    while start < end && chars[start] == '0' {
        start += 1;
    }
    start
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_compares_numbers() {
        assert_eq!(natural_cmp("2.jpg", "10.jpg"), Ordering::Less);
        assert_eq!(natural_cmp("10.jpg", "2.jpg"), Ordering::Greater);
        assert_eq!(natural_cmp("page_009.png", "page_10.png"), Ordering::Less);
        assert_eq!(natural_cmp("a1", "a01"), Ordering::Equal);
        assert_eq!(natural_cmp("ch2/10.jpg", "ch10/2.jpg"), Ordering::Less);
    }
}
