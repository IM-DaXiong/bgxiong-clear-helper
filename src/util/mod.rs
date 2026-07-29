mod elevate;

pub use elevate::restart_as_admin;

use std::path::{Path, PathBuf};

use windows::Win32::Security::{
    GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::IsUserAnAdmin;

pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

pub fn expand_env_path(raw: &str) -> PathBuf {
    if raw.contains('%') {
        let mut out = String::with_capacity(raw.len());
        let mut chars = raw.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '%' {
                let mut var = String::new();
                while let Some(&next) = chars.peek() {
                    if next == '%' {
                        chars.next();
                        break;
                    }
                    var.push(chars.next().unwrap());
                }
                let value = std::env::var(&var).unwrap_or_default();
                out.push_str(&value);
            } else {
                out.push(ch);
            }
        }
        PathBuf::from(out)
    } else {
        PathBuf::from(raw)
    }
}

pub fn is_elevated() -> bool {
    unsafe {
        if IsUserAnAdmin().as_bool() {
            return true;
        }

        let mut token = windows::Win32::Foundation::HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }

        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
        .is_ok();

        ok && elevation.TokenIsElevated != 0
    }
}

pub fn env_paths(vars: &[&str]) -> Vec<PathBuf> {
    vars.iter()
        .filter_map(|v| std::env::var(v).ok())
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .collect()
}

pub fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

pub fn is_locked_error(err: &std::io::Error) -> bool {
    matches!(
        err.raw_os_error(),
        Some(32) | Some(33) | Some(5) // sharing violation, lock, access denied
    )
}
