use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW, RegQueryValueExW, HKEY,
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, REG_SZ, REG_VALUE_TYPE,
};

use crate::models::{
    ItemStatus, JunkCategory, JunkItem, JunkTarget, ScanProgress, MAX_ITEMS_PER_CATEGORY,
};
use crate::scanner::temp::placeholder_admin;
use crate::util::expand_env_path;

const BLACKLIST_FRAGMENTS: &[&str] = &[
    r"Windows NT\CurrentVersion\ProfileList",
    r"Windows NT\CurrentVersion\Windows",
    r"Microsoft\Windows NT\CurrentVersion\Winlogon",
    r"CurrentControlSet\Services",
];

struct HiveRoot {
    hkey: HKEY,
    name: &'static str,
}

pub fn scan_registry_orphans(
    is_admin: bool,
    cancel: &AtomicBool,
    progress: &Sender<ScanProgress>,
) -> Vec<JunkItem> {
    let mut items = Vec::new();

    // HKCU always readable
    scan_hive(
        HiveRoot {
            hkey: HKEY_CURRENT_USER,
            name: "HKCU",
        },
        true,
        cancel,
        progress,
        &mut items,
    );

    if is_admin {
        scan_hive(
            HiveRoot {
                hkey: HKEY_LOCAL_MACHINE,
                name: "HKLM",
            },
            true,
            cancel,
            progress,
            &mut items,
        );
    } else {
        // Placeholder so UI shows admin requirement for HKLM portion
        let mut placeholder = placeholder_admin(
            JunkCategory::RegistryOrphans,
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        );
        placeholder.skip_reason =
            Some("需要管理员权限才能扫描/清理 HKLM 注册表项".into());
        items.push(placeholder);
    }

    items
}

fn scan_hive(
    hive: HiveRoot,
    _can_write: bool,
    cancel: &AtomicBool,
    progress: &Sender<ScanProgress>,
    items: &mut Vec<JunkItem>,
) {
    let uninstall_paths = [
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ];

    for sub in uninstall_paths {
        if cancel.load(Ordering::Relaxed) || items.len() >= MAX_ITEMS_PER_CATEGORY {
            return;
        }
        scan_uninstall(&hive, sub, cancel, progress, items);
    }

    let app_paths = [
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths",
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths",
    ];
    for sub in app_paths {
        if cancel.load(Ordering::Relaxed) || items.len() >= MAX_ITEMS_PER_CATEGORY {
            return;
        }
        scan_app_paths(&hive, sub, cancel, progress, items);
    }

    for sub in [
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce",
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run",
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\RunOnce",
    ] {
        if cancel.load(Ordering::Relaxed) || items.len() >= MAX_ITEMS_PER_CATEGORY {
            return;
        }
        scan_run_values(&hive, sub, cancel, progress, items);
    }
}

fn scan_uninstall(
    hive: &HiveRoot,
    subkey: &str,
    cancel: &AtomicBool,
    progress: &Sender<ScanProgress>,
    items: &mut Vec<JunkItem>,
) {
    let Ok(parent) = open_key(hive.hkey, subkey) else {
        return;
    };

    for child in enum_subkeys(parent) {
        if cancel.load(Ordering::Relaxed) || items.len() >= MAX_ITEMS_PER_CATEGORY {
            break;
        }

        let full_sub = format!("{subkey}\\{child}");
        let display_key = format!("{}\\{}", hive.name, full_sub);
        if is_blacklisted(&display_key) {
            continue;
        }

        let Ok(key) = open_key(hive.hkey, &full_sub) else {
            continue;
        };

        let install_location = query_string(key, "InstallLocation");
        let display_icon = query_string(key, "DisplayIcon");
        let uninstall_string = query_string(key, "UninstallString");
        unsafe {
            let _ = RegCloseKey(key);
        }

        let mut resolved: Vec<PathBuf> = Vec::new();
        if let Some(p) = install_location.as_deref().and_then(normalize_path_value) {
            resolved.push(p);
        }
        if let Some(p) = uninstall_string
            .as_deref()
            .and_then(extract_exe_from_command)
        {
            resolved.push(p);
        }
        if resolved.is_empty() {
            if let Some(p) = display_icon.as_deref().and_then(normalize_icon_path) {
                resolved.push(p);
            }
        }

        if resolved.is_empty() || resolved.iter().any(|p| path_exists(p)) {
            continue;
        }

        let missing = resolved[0].clone();
        push_registry_item(
            items,
            display_key.clone(),
            display_key,
            None,
            missing,
            false,
            progress,
        );
    }

    unsafe {
        let _ = RegCloseKey(parent);
    }
}

fn scan_app_paths(
    hive: &HiveRoot,
    subkey: &str,
    cancel: &AtomicBool,
    progress: &Sender<ScanProgress>,
    items: &mut Vec<JunkItem>,
) {
    let Ok(parent) = open_key(hive.hkey, subkey) else {
        return;
    };

    for child in enum_subkeys(parent) {
        if cancel.load(Ordering::Relaxed) || items.len() >= MAX_ITEMS_PER_CATEGORY {
            break;
        }

        let full_sub = format!("{subkey}\\{child}");
        let display_key = format!("{}\\{}", hive.name, full_sub);
        if is_blacklisted(&display_key) {
            continue;
        }

        let Ok(key) = open_key(hive.hkey, &full_sub) else {
            continue;
        };

        let default_val = query_string(key, "");
        let path_val = query_string(key, "Path");
        unsafe {
            let _ = RegCloseKey(key);
        }

        let exe = default_val
            .as_deref()
            .and_then(extract_exe_from_command)
            .or_else(|| path_val.as_deref().and_then(normalize_path_value));

        let Some(exe_path) = exe else {
            continue;
        };
        if path_exists(&exe_path) {
            continue;
        }

        push_registry_item(
            items,
            display_key.clone(),
            display_key,
            None,
            exe_path,
            false,
            progress,
        );
    }

    unsafe {
        let _ = RegCloseKey(parent);
    }
}

fn scan_run_values(
    hive: &HiveRoot,
    subkey: &str,
    cancel: &AtomicBool,
    progress: &Sender<ScanProgress>,
    items: &mut Vec<JunkItem>,
) {
    let Ok(key) = open_key(hive.hkey, subkey) else {
        return;
    };

    let store_key = format!("{}\\{}", hive.name, subkey);
    if is_blacklisted(&store_key) {
        unsafe {
            let _ = RegCloseKey(key);
        }
        return;
    }

    for (name, data) in enum_values(key) {
        if cancel.load(Ordering::Relaxed) || items.len() >= MAX_ITEMS_PER_CATEGORY {
            break;
        }
        if name.is_empty() {
            continue;
        }

        let Some(exe) = extract_exe_from_command(&data) else {
            continue;
        };
        if path_exists(&exe) {
            continue;
        }

        let display_key = format!("{store_key} [{name}]");
        push_registry_item(
            items,
            display_key,
            store_key.clone(),
            Some(name),
            exe,
            false,
            progress,
        );
    }

    unsafe {
        let _ = RegCloseKey(key);
    }
}

fn push_registry_item(
    items: &mut Vec<JunkItem>,
    display_path: String,
    store_key: String,
    value_name: Option<String>,
    missing_path: PathBuf,
    needs_admin: bool,
    progress: &Sender<ScanProgress>,
) {
    let status = if needs_admin {
        ItemStatus::NeedsAdmin
    } else {
        ItemStatus::Normal
    };

    let reason = format!("指向的文件已不存在: {}", missing_path.display());

    items.push(JunkItem {
        category: JunkCategory::RegistryOrphans,
        path: PathBuf::from(&display_path),
        size: 0,
        status,
        selected: false,
        skip_reason: Some(reason),
        is_recycle_bin: false,
        browser_sub: None,
        app_name: None,
        target: JunkTarget::RegistryKey {
            key: store_key,
            value_name,
        },
    });

    if items.len() % 50 == 0 {
        let _ = progress.send(ScanProgress {
            category: JunkCategory::RegistryOrphans,
            files_found: items.len(),
            message: format!(
                "{}: 已找到 {} 项",
                JunkCategory::RegistryOrphans.label(),
                items.len()
            ),
        });
    }
}

fn is_blacklisted(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    BLACKLIST_FRAGMENTS
        .iter()
        .any(|f| lower.contains(&f.to_ascii_lowercase()))
}

fn path_exists(path: &Path) -> bool {
    path.exists()
}

fn normalize_path_value(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        return None;
    }
    let expanded = expand_env_path(trimmed);
    if expanded.as_os_str().is_empty() {
        return None;
    }
    Some(expanded)
}

fn normalize_icon_path(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim().trim_matches('"');
    // Strip ,0 or ,-1 icon index
    let without_index = if let Some(idx) = trimmed.rfind(',') {
        let suffix = &trimmed[idx + 1..];
        if suffix.chars().all(|c| c == '-' || c.is_ascii_digit()) {
            &trimmed[..idx]
        } else {
            trimmed
        }
    } else {
        trimmed
    };
    normalize_path_value(without_index.trim().trim_matches('"'))
}

fn extract_exe_from_command(cmd: &str) -> Option<PathBuf> {
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return None;
    }

    if cmd.starts_with('"') {
        let end = cmd[1..].find('"')?;
        return normalize_path_value(&cmd[1..1 + end]);
    }

    // First whitespace-separated token; may include args after
    let token = cmd.split_whitespace().next()?;
    normalize_path_value(token)
}

fn open_key(root: HKEY, subkey: &str) -> Result<HKEY, WIN32_ERROR> {
    let wide: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let mut hkey = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            root,
            PCWSTR(wide.as_ptr()),
            0,
            KEY_READ | KEY_WOW64_64KEY,
            &mut hkey,
        )
    };
    if status == ERROR_SUCCESS {
        Ok(hkey)
    } else {
        Err(status)
    }
}

fn enum_subkeys(key: HKEY) -> Vec<String> {
    let mut names = Vec::new();
    let mut index = 0u32;
    loop {
        let mut name_buf = [0u16; 256];
        let mut name_len = name_buf.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                key,
                index,
                PWSTR(name_buf.as_mut_ptr()),
                &mut name_len,
                None,
                PWSTR::null(),
                None,
                None,
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        if status != ERROR_SUCCESS {
            break;
        }
        let name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
        names.push(name);
        index += 1;
    }
    names
}

fn enum_values(key: HKEY) -> Vec<(String, String)> {
    let mut values = Vec::new();
    let mut index = 0u32;
    loop {
        let mut name_buf = [0u16; 512];
        let mut name_len = name_buf.len() as u32;
        let mut data_buf = [0u8; 4096];
        let mut data_len = data_buf.len() as u32;
        let mut reg_type = 0u32;

        let status = unsafe {
            RegEnumValueW(
                key,
                index,
                PWSTR(name_buf.as_mut_ptr()),
                &mut name_len,
                None,
                Some(&mut reg_type as *mut u32),
                Some(data_buf.as_mut_ptr()),
                Some(&mut data_len),
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        if status != ERROR_SUCCESS {
            break;
        }

        let name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
        // REG_SZ = 1, REG_EXPAND_SZ = 2
        let data = if reg_type == 1 || reg_type == 2 {
            let wide_len = (data_len as usize / 2).saturating_sub(1);
            let wide: Vec<u16> = data_buf[..data_len as usize]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .take(wide_len)
                .collect();
            String::from_utf16_lossy(&wide)
        } else {
            index += 1;
            continue;
        };

        values.push((name, data));
        index += 1;
    }
    values
}

fn query_string(key: HKEY, value_name: &str) -> Option<String> {
    let wide_name: Vec<u16> = if value_name.is_empty() {
        vec![0]
    } else {
        value_name.encode_utf16().chain(std::iter::once(0)).collect()
    };

    let mut reg_type = REG_VALUE_TYPE::default();
    let mut data_len = 0u32;
    let status = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR(wide_name.as_ptr()),
            None,
            Some(&mut reg_type),
            None,
            Some(&mut data_len),
        )
    };
    if status != ERROR_SUCCESS || data_len == 0 {
        return None;
    }

    let mut data = vec![0u8; data_len as usize];
    let status = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR(wide_name.as_ptr()),
            None,
            Some(&mut reg_type),
            Some(data.as_mut_ptr()),
            Some(&mut data_len),
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }

    if reg_type != REG_SZ && reg_type.0 != 2 {
        return None;
    }

    let wide_len = (data_len as usize / 2).saturating_sub(1);
    let wide: Vec<u16> = data[..data_len as usize]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take(wide_len)
        .collect();
    let s = String::from_utf16_lossy(&wide);
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}
