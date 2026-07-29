use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use walkdir::WalkDir;

use crate::models::{
    BrowserSubKind, ItemStatus, JunkCategory, JunkItem, JunkTarget, ScanProgress,
    MAX_ITEMS_PER_CATEGORY,
};
use crate::scanner::temp::check_browser_path;
use crate::util::env_paths;

pub fn scan_edge_cache(cancel: &AtomicBool, progress: &Sender<ScanProgress>) -> Vec<JunkItem> {
    scan_browser(
        JunkCategory::EdgeCache,
        r"Microsoft\Edge\User Data",
        cancel,
        progress,
    )
}

pub fn scan_chrome_cache(cancel: &AtomicBool, progress: &Sender<ScanProgress>) -> Vec<JunkItem> {
    scan_browser(
        JunkCategory::ChromeCache,
        r"Google\Chrome\User Data",
        cancel,
        progress,
    )
}

fn scan_browser(
    category: JunkCategory,
    base_relative: &str,
    cancel: &AtomicBool,
    progress: &Sender<ScanProgress>,
) -> Vec<JunkItem> {
    let roots: Vec<PathBuf> = env_paths(&["LOCALAPPDATA"])
        .into_iter()
        .map(|p| p.join(base_relative))
        .filter(|p| p.exists())
        .collect();

    if roots.is_empty() {
        return Vec::new();
    }

    let mut items = Vec::new();
    let mut truncated = false;

    'outer: for user_data in &roots {
        for profile in discover_profiles(user_data) {
            if cancel.load(Ordering::Relaxed) {
                break 'outer;
            }

            for entry in WalkDir::new(&profile)
                .max_depth(8)
                .follow_links(false)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                if cancel.load(Ordering::Relaxed) {
                    break 'outer;
                }

                let path = entry.path();
                if !path.is_file() {
                    continue;
                }

                let Some(sub) = classify_browser_path(&profile, path) else {
                    continue;
                };

                let status = check_browser_path(path);
                let skip_reason = if status == ItemStatus::Skipped {
                    Some("文件被占用，浏览器可能正在运行".into())
                } else {
                    None
                };

                let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                items.push(JunkItem {
                    category,
                    path: path.to_path_buf(),
                    size,
                    status,
                    selected: false,
                    skip_reason,
                    is_recycle_bin: false,
                    browser_sub: Some(sub),
                    app_name: None,
                    target: JunkTarget::File(path.to_path_buf()),
                });

                if items.len() % 200 == 0 {
                    let _ = progress.send(ScanProgress {
                        category,
                        files_found: items.len(),
                        message: format!("{}: 已找到 {} 项", category.label(), items.len()),
                    });
                }

                if items.len() >= MAX_ITEMS_PER_CATEGORY {
                    truncated = true;
                    break 'outer;
                }
            }
        }
    }

    if truncated {
        let _ = progress.send(ScanProgress {
            category,
            files_found: items.len(),
            message: format!("{}: 已达展示上限", category.label()),
        });
    }

    items
}

fn discover_profiles(user_data: &Path) -> Vec<PathBuf> {
    let mut profiles = Vec::new();
    let default = user_data.join("Default");
    if default.is_dir() {
        profiles.push(default);
    }

    if let Ok(entries) = std::fs::read_dir(user_data) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("Profile ") || name == "Guest Profile" {
                profiles.push(path);
            }
        }
    }

    profiles.sort();
    profiles.dedup();
    profiles
}

fn classify_browser_path(profile: &Path, file: &Path) -> Option<BrowserSubKind> {
    let rel = file.strip_prefix(profile).ok()?;
    let rel_str = rel.to_string_lossy().replace('/', "\\");
    let lower = rel_str.to_ascii_lowercase();

    // Cookies (files)
    if lower == "cookies"
        || lower == "network\\cookies"
        || lower == "network\\cookies-journal"
        || lower.ends_with("\\cookies")
            && !lower.contains("\\cache")
            && (lower.starts_with("network\\") || !lower.contains('\\'))
    {
        return Some(BrowserSubKind::Cookies);
    }
    if lower.starts_with("network\\cookies") {
        return Some(BrowserSubKind::Cookies);
    }

    // Local / session storage / IndexedDB
    if lower.starts_with("local storage\\")
        || lower.starts_with("session storage\\")
        || lower.starts_with("indexeddb\\")
        || lower == "local storage"
        || lower == "session storage"
        || lower == "indexeddb"
    {
        return Some(BrowserSubKind::LocalStorage);
    }

    // Code Cache (before generic Cache)
    if lower.starts_with("code cache\\") || lower.contains("\\code cache\\") {
        return Some(BrowserSubKind::CodeCache);
    }

    // GPUCache
    if lower.starts_with("gpucache\\") || lower.contains("\\gpucache\\") || lower == "gpucache" {
        return Some(BrowserSubKind::GpuCache);
    }

    // Media / Service Worker cache
    if lower.starts_with("media cache\\")
        || lower.contains("\\media cache\\")
        || lower.starts_with("service worker\\cachestorage\\")
        || lower.contains("\\service worker\\cachestorage\\")
    {
        return Some(BrowserSubKind::MediaCache);
    }

    // HTTP Cache — Cache dir but not Code Cache
    if (lower.starts_with("cache\\") || lower.contains("\\cache\\") || lower.starts_with("cache_data\\"))
        && !lower.contains("code cache")
        && !lower.contains("media cache")
        && !lower.contains("gpucache")
    {
        return Some(BrowserSubKind::HttpCache);
    }

    None
}
