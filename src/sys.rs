//! Appels Windows : chiffrement du compte (DPAPI), mémoire et carte graphique, ouverture de liens, instance unique.
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, LocalFree, ERROR_ALREADY_EXISTS, HANDLE};
use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ,
};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE, INFINITE,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, MessageBoxW, ASFW_ANY, IDYES, MB_ICONERROR, MB_ICONWARNING, MB_OK, MB_YESNO,
    SW_SHOWNORMAL,
};

pub fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

/// Chiffre ou déchiffre pour l'utilisateur Windows courant (même format que l'ancien launcher Python).
pub fn dpapi(data: &[u8], protect: bool) -> Option<Vec<u8>> {
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() };
        let ok = if protect {
            CryptProtectData(&input, null(), null(), null(), null(), 0, &mut out)
        } else {
            CryptUnprotectData(&input, null_mut(), null(), null(), null(), 0, &mut out)
        };
        if ok == 0 || out.pbData.is_null() {
            return None;
        }
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(out.pbData as _);
        Some(v)
    }
}

/// Mémoire vive totale (Mo).
pub fn total_ram_mb() -> u64 {
    unsafe {
        let mut m: MEMORYSTATUSEX = std::mem::zeroed();
        m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut m) == 0 {
            return 8192;
        }
        m.ullTotalPhys / (1024 * 1024)
    }
}

/// Cartes graphiques déclarées à Windows (registre : instantané, contrairement à une requête WMI).
pub fn gpu_names() -> Vec<String> {
    const CLASS: &str = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
    let mut names = Vec::new();
    unsafe {
        let mut key: HKEY = null_mut();
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, wide(CLASS).as_ptr(), 0, KEY_READ, &mut key) != 0 {
            return names;
        }
        let value = wide("DriverDesc");
        for i in 0..32u32 {
            let mut sub = [0u16; 64];
            let mut len = sub.len() as u32;
            if RegEnumKeyExW(key, i, sub.as_mut_ptr(), &mut len, null(), null_mut(), null_mut(), null_mut()) != 0 {
                break;
            }
            let mut buf = [0u16; 256];
            let mut size = (buf.len() * 2) as u32;
            if RegGetValueW(key, sub.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ, null_mut(), buf.as_mut_ptr() as _, &mut size) == 0 {
                let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                let name = String::from_utf16_lossy(&buf[..n]).trim().to_string();
                let lower = name.to_lowercase();
                if !name.is_empty() && !lower.contains("basic display") && !lower.contains("remote display")
                    && !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        RegCloseKey(key);
    }
    names
}

/// Ouvre un dossier ou une adresse avec l'application par défaut.
pub fn shell_open(target: &str) {
    unsafe {
        ShellExecuteW(null_mut(), wide("open").as_ptr(), wide(target).as_ptr(), null(), null(), SW_SHOWNORMAL);
    }
}

pub fn message(title: &str, text: &str, error: bool) {
    unsafe {
        MessageBoxW(null_mut(), wide(text).as_ptr(), wide(title).as_ptr(), MB_OK | if error { MB_ICONERROR } else { MB_ICONWARNING });
    }
}

pub fn ask(title: &str, text: &str) -> bool {
    unsafe { MessageBoxW(null_mut(), wide(text).as_ptr(), wide(title).as_ptr(), MB_YESNO | MB_ICONWARNING) == IDYES }
}

/// Instance unique par numéro de compte. Si le launcher tourne déjà, on lui demande de se montrer et on renvoie false.
/// Sinon `on_show` est appelé chaque fois qu'une nouvelle copie est lancée.
pub fn single_instance(instance: u32, on_show: impl Fn() + Send + 'static) -> bool {
    let mutex_name = wide(&format!("Local\\SurvivalFrenchLauncher-{instance}"));
    let event_name = wide(&format!("Local\\SurvivalFrenchLauncher-show-{instance}"));
    unsafe {
        let mutex = CreateMutexW(null(), 0, mutex_name.as_ptr());
        if !mutex.is_null() && GetLastError() == ERROR_ALREADY_EXISTS {
            let ev = OpenEventW(EVENT_MODIFY_STATE, 0, event_name.as_ptr());
            if !ev.is_null() {
                AllowSetForegroundWindow(ASFW_ANY);
                SetEvent(ev);
                CloseHandle(ev);
            }
            CloseHandle(mutex);
            return false;
        }
        // Le mutex reste ouvert jusqu'à la fin du processus.
        let ev: HANDLE = CreateEventW(null(), 0, 0, event_name.as_ptr());
        if !ev.is_null() {
            let ev = ev as usize;
            std::thread::spawn(move || loop {
                if WaitForSingleObject(ev as HANDLE, INFINITE) != 0 {
                    return;
                }
                on_show();
            });
        }
    }
    true
}
