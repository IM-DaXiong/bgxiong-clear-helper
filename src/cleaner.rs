use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteKeyExW, RegDeleteTreeW, RegDeleteValueW, RegOpenKeyExW, HKEY,
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS, KEY_WOW64_64KEY,
};

use crate::models::{CleanError, CleanResult, JunkItem, JunkTarget};
use crate::util::is_locked_error;

pub fn clean_selected(items: &[JunkItem], is_admin: bool) -> CleanResult {
    let mut result = CleanResult::default();
    let mut recycle_bin_requested = false;

    let registry_items: Vec<&JunkItem> = items
        .iter()
        .filter(|i| i.selected && i.can_clean(is_admin) && i.is_registry())
        .collect();

    if !registry_items.is_empty() {
        if let Err(e) = backup_registry_items(&registry_items) {
            result.errors.push(CleanError {
                path: PathBuf::from("注册表备份"),
                reason: format!("备份失败，已中止注册表清理: {e}"),
            });
            result.fail_count += registry_items.len();
            // Still clean files below
        } else {
            for item in &registry_items {
                match &item.target {
                    JunkTarget::RegistryKey { key, value_name } => {
                        match delete_registry_entry(key, value_name.as_deref()) {
                            Ok(()) => {
                                result.success_count += 1;
                            }
                            Err(e) => {
                                result.fail_count += 1;
                                result.errors.push(CleanError {
                                    path: item.path.clone(),
                                    reason: e,
                                });
                            }
                        }
                    }
                    JunkTarget::File(_) => {}
                }
            }
        }
    }

    for item in items {
        if !item.selected || !item.can_clean(is_admin) {
            continue;
        }

        if item.is_registry() {
            continue;
        }

        if item.is_recycle_bin {
            recycle_bin_requested = true;
            continue;
        }

        let path = match &item.target {
            JunkTarget::File(p) => p,
            JunkTarget::RegistryKey { .. } => &item.path,
        };
        delete_path(path, item.size, &mut result);
    }

    if recycle_bin_requested {
        match empty_recycle_bin() {
            Ok(freed) => {
                result.success_count += 1;
                result.freed_bytes += freed;
            }
            Err(e) => {
                result.fail_count += 1;
                result.errors.push(CleanError {
                    path: PathBuf::from("回收站"),
                    reason: e,
                });
            }
        }
    }

    result
}

fn backup_registry_items(items: &[&JunkItem]) -> Result<PathBuf, String> {
    let temp = std::env::var("TEMP")
        .or_else(|_| std::env::var("TMP"))
        .unwrap_or_else(|_| r"C:\Windows\Temp".into());
    let dir = PathBuf::from(temp).join("bgxiong-reg-backup");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建备份目录失败: {e}"))?;

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("backup-{ts}.reg"));

    let mut content = String::from("Windows Registry Editor Version 5.00\r\n\r\n");
    content.push_str("; BGXiong Clear Helper — registry deletion manifest\r\n");
    content.push_str("; This file lists keys/values that were removed. Per-key exports\r\n");
    content.push_str("; (if successful) are saved alongside as export-N.reg for restore.\r\n\r\n");

    for (i, item) in items.iter().enumerate() {
        if let JunkTarget::RegistryKey { key, value_name } = &item.target {
            content.push_str(&format!("; [{i}] KEY: {key}\r\n"));
            if let Some(vn) = value_name {
                content.push_str(&format!(";     VALUE: {vn}\r\n"));
            }
            if let Some(reason) = &item.skip_reason {
                content.push_str(&format!(";     {reason}\r\n"));
            }
            content.push_str("\r\n");

            // Best-effort native export for restore
            let export_path = dir.join(format!("export-{i}.reg"));
            let _ = export_reg_key(key, &export_path);
        }
    }

    // Write UTF-16 LE with BOM for Windows .reg compatibility
    let mut bytes = vec![0xFF, 0xFE];
    for unit in content.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    std::fs::write(&path, bytes).map_err(|e| format!("写入备份失败: {e}"))?;
    Ok(path)
}

fn export_reg_key(key: &str, dest: &PathBuf) -> Result<(), String> {
    let full = key
        .replace("HKCU\\", "HKCU\\")
        .replace("HKLM\\", "HKLM\\");
    let status = std::process::Command::new("reg")
        .args(["export", &full, &dest.to_string_lossy(), "/y"])
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("reg export 失败: {key}"))
    }
}

fn delete_registry_entry(key_path: &str, value_name: Option<&str>) -> Result<(), String> {
    let (hive, subkey) = split_hive(key_path)?;

    if let Some(vn) = value_name {
        let hkey = open_key_write(hive, subkey)?;
        let wide: Vec<u16> = vn.encode_utf16().chain(std::iter::once(0)).collect();
        let status = unsafe { RegDeleteValueW(hkey, PCWSTR(wide.as_ptr())) };
        unsafe {
            let _ = RegCloseKey(hkey);
        }
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!("删除注册表值失败 (错误码 {})", status.0))
        }
    } else {
        // Delete subkey: open parent and delete child, or RegDeleteTree on full path
        let (parent, child) = match subkey.rsplit_once('\\') {
            Some((p, c)) => (p, c),
            None => {
                return Err("拒绝删除顶级注册表键".into());
            }
        };

        let parent_key = open_key_write(hive, parent)?;
        let wide: Vec<u16> = child.encode_utf16().chain(std::iter::once(0)).collect();

        // Prefer delete tree for uninstall keys that may have subkeys
        let status = unsafe { RegDeleteTreeW(parent_key, PCWSTR(wide.as_ptr())) };
        if status != ERROR_SUCCESS {
            let status2 = unsafe { RegDeleteKeyExW(parent_key, PCWSTR(wide.as_ptr()), KEY_WOW64_64KEY.0, 0) };
            unsafe {
                let _ = RegCloseKey(parent_key);
            }
            if status2 == ERROR_SUCCESS {
                Ok(())
            } else {
                Err(format!(
                    "删除注册表键失败 (错误码 {} / {})",
                    status.0, status2.0
                ))
            }
        } else {
            unsafe {
                let _ = RegCloseKey(parent_key);
            }
            Ok(())
        }
    }
}

fn split_hive(key_path: &str) -> Result<(HKEY, &str), String> {
    if let Some(rest) = key_path.strip_prefix("HKCU\\") {
        Ok((HKEY_CURRENT_USER, rest))
    } else if let Some(rest) = key_path.strip_prefix("HKLM\\") {
        Ok((HKEY_LOCAL_MACHINE, rest))
    } else if let Some(rest) = key_path.strip_prefix("HKEY_CURRENT_USER\\") {
        Ok((HKEY_CURRENT_USER, rest))
    } else if let Some(rest) = key_path.strip_prefix("HKEY_LOCAL_MACHINE\\") {
        Ok((HKEY_LOCAL_MACHINE, rest))
    } else {
        Err(format!("无法识别的注册表根: {key_path}"))
    }
}

fn open_key_write(root: HKEY, subkey: &str) -> Result<HKEY, String> {
    let wide: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let mut hkey = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            root,
            PCWSTR(wide.as_ptr()),
            0,
            KEY_ALL_ACCESS | KEY_WOW64_64KEY,
            &mut hkey,
        )
    };
    if status == ERROR_SUCCESS {
        Ok(hkey)
    } else {
        Err(format!("打开注册表键失败 (错误码 {})", status.0))
    }
}

fn delete_path(path: &PathBuf, size: u64, result: &mut CleanResult) {
    match std::fs::remove_file(path) {
        Ok(()) => {
            result.success_count += 1;
            result.freed_bytes += size;
        }
        Err(file_err) => {
            if path.is_dir() {
                match std::fs::remove_dir_all(path) {
                    Ok(()) => {
                        result.success_count += 1;
                        result.freed_bytes += size;
                    }
                    Err(dir_err) => {
                        result.fail_count += 1;
                        result.errors.push(CleanError {
                            path: path.clone(),
                            reason: format!("{file_err}; 目录删除: {dir_err}"),
                        });
                    }
                }
            } else {
                let reason = if is_locked_error(&file_err) {
                    format!("{file_err}（文件可能被占用）")
                } else {
                    file_err.to_string()
                };
                result.fail_count += 1;
                result.errors.push(CleanError {
                    path: path.clone(),
                    reason,
                });
            }
        }
    }
}

fn empty_recycle_bin() -> Result<u64, String> {
    use windows::Win32::UI::Shell::{SHEmptyRecycleBinW, SHERB_NOCONFIRMATION};

    let items = crate::scanner::scan_recycle_bin();
    if items.is_empty() {
        return Ok(0);
    }

    let size = items[0].size;

    unsafe {
        SHEmptyRecycleBinW(None, None, SHERB_NOCONFIRMATION)
            .map_err(|e| format!("清空回收站失败: {e}"))?;
    }

    Ok(size)
}
