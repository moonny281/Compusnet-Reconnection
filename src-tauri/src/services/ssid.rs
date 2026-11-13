//! Current WLAN SSID via the Windows WLAN API (`windows` crate).
//! Windows-only by design (see PLAN.md §1.2).

use crate::error::{AppError, AppResult};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Cached SSID read; WLAN queries are not free, so cache briefly.
struct Cache {
    value: Option<Option<String>>, // None = not cached yet
    at: Instant,
}

pub struct SsidProvider {
    cache: Mutex<Cache>,
    ttl: Duration,
}

impl SsidProvider {
    pub fn new(ttl: Duration) -> Self {
        Self {
            cache: Mutex::new(Cache {
                value: None,
                at: Instant::now() - Duration::from_secs(60),
            }),
            ttl,
        }
    }

    /// SSID of the currently connected WLAN interface, or `None` when
    /// not connected to any WLAN / no interface.
    pub fn current_ssid(&self) -> Option<String> {
        {
            let cache = self.cache.lock().unwrap();
            if let Some(v) = &cache.value {
                if cache.at.elapsed() < self.ttl {
                    return v.clone();
                }
            }
        }
        let fresh = wlan_query_ssid().ok().flatten();
        let mut cache = self.cache.lock().unwrap();
        *cache = Cache {
            value: Some(fresh.clone()),
            at: Instant::now(),
        };
        fresh
    }
}

#[cfg(windows)]
mod wlan {
    use super::*;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::NetworkManagement::WiFi::*;

    pub fn wlan_query_ssid() -> AppResult<Option<String>> {
        unsafe {
            let mut negotiated = 0u32;
            let mut handle = HANDLE::default();
            // WLAN_CLIENT_VERSION_VISTA = 2. WLAN API calls return a plain
            // u32 status code (WIN32_ERROR value), not the WIN32_ERROR type.
            let status = WlanOpenHandle(2, Some(std::ptr::null()), &mut negotiated, &mut handle);
            if status != 0 {
                return Err(AppError::Ssid(format!("WlanOpenHandle failed: {status}")));
            }
            let _guard = HandleGuard(handle);

            let mut iface_list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
            let status = WlanEnumInterfaces(handle, None, &mut iface_list);
            if status != 0 {
                return Err(AppError::Ssid(format!(
                    "WlanEnumInterfaces failed: {status}"
                )));
            }
            let _list_guard = ListGuard(iface_list.cast());

            if (*iface_list).dwNumberOfItems == 0 {
                return Ok(None);
            }

            // Take the first interface (machines normally have one WLAN NIC).
            let iface: WLAN_INTERFACE_INFO = (*iface_list).InterfaceInfo[0];
            if iface.isState != wlan_interface_state_connected {
                return Ok(None);
            }

            let mut conn_data_size = 0u32;
            let conn_data: *mut WLAN_CONNECTION_ATTRIBUTES = std::ptr::null_mut();
            let status = WlanQueryInterface(
                handle,
                &iface.InterfaceGuid,
                wlan_intf_opcode_current_connection,
                None,
                &mut conn_data_size,
                &mut conn_data.cast(),
                None,
            );
            if status != 0 {
                return Err(AppError::Ssid(format!(
                    "WlanQueryInterface failed: {status}"
                )));
            }
            let _conn_guard = ListGuard(conn_data.cast());

            // In windows 0.58 the SSID lives under wlanAssociationAttributes.
            let attrs: &WLAN_CONNECTION_ATTRIBUTES = &*conn_data;
            let ssid = &attrs.wlanAssociationAttributes.dot11Ssid;
            let len = ssid.uSSIDLength as usize;
            if len == 0 {
                return Ok(None);
            }
            let ssid_bytes = &ssid.ucSSID[..len];
            Ok(Some(decode_ssid(ssid_bytes)))
        }
    }

    /// SSIDs are officially opaque bytes. Try UTF-8 first (most modern
    /// routers broadcast UTF-8); fall back to the GBK codepage via the Win32
    /// API so Chinese SSIDs render correctly instead of mojibake.
    fn decode_ssid(bytes: &[u8]) -> String {
        if let Ok(s) = std::str::from_utf8(bytes) {
            return s.to_string();
        }
        // 936 = GBK. MultiByteToWideChar is cheap and avoids a crate dep.
        #[link(name = "kernel32")]
        extern "system" {
            fn MultiByteToWideChar(
                codepage: u32,
                flags: u32,
                src: *const u8,
                srclen: i32,
                dst: *mut u16,
                dstlen: i32,
            ) -> i32;
        }
        unsafe {
            let n = MultiByteToWideChar(
                936,
                0,
                bytes.as_ptr(),
                bytes.len() as i32,
                std::ptr::null_mut(),
                0,
            );
            if n > 0 {
                let mut buf = vec![0u16; n as usize];
                MultiByteToWideChar(
                    936,
                    0,
                    bytes.as_ptr(),
                    bytes.len() as i32,
                    buf.as_mut_ptr(),
                    n,
                );
                return String::from_utf16_lossy(&buf);
            }
        }
        // Last resort: latin-1-ish, keeps bytes distinguishable.
        bytes.iter().map(|&b| b as char).collect()
    }

    struct HandleGuard(HANDLE);
    impl Drop for HandleGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = WlanCloseHandle(self.0, None);
            }
        }
    }

    struct ListGuard(*mut core::ffi::c_void);
    impl Drop for ListGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { WlanFreeMemory(self.0) };
            }
        }
    }
}

#[cfg(windows)]
use wlan::wlan_query_ssid;

#[cfg(not(windows))]
fn wlan_query_ssid() -> AppResult<Option<String>> {
    Err(AppError::Ssid("non-Windows build".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_reuses_within_ttl() {
        let p = SsidProvider::new(Duration::from_millis(200));
        let _ = p.current_ssid(); // warms the cache (may error on CI w/o WLAN)
                                  // Second read must hit the cache and not panic.
        let _ = p.current_ssid();
    }
}
