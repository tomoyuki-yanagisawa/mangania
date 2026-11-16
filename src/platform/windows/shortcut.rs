use std::ffi::{OsString, c_void};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr;

use windows_sys::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows_sys::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize, STGM_READ,
};
use windows_sys::core::{GUID, HRESULT};

use crate::debug_log;

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

/// Resolves a Windows shortcut to its target path.
pub(crate) fn resolve(path: &Path) -> Result<PathBuf, String> {
    debug_log::log(format_args!("shortcut: resolving {}", path.display()));
    let target =
        shortcut_target(path).map_err(|e| format!("resolve shortcut {}: {e}", path.display()))?;
    debug_log::log(format_args!("shortcut: target={}", target.display()));
    Ok(target)
}

fn shortcut_target(path: &Path) -> Result<PathBuf, String> {
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
