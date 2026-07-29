use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use walkdir::WalkDir;

use crate::models::{ItemStatus, JunkCategory, JunkItem, ScanProgress, MAX_ITEMS_PER_CATEGORY};
use crate::scanner::temp::{placeholder_admin, scan_directory_files};

pub fn scan_prefetch(cancel: &AtomicBool, progress: &Sender<ScanProgress>) -> Vec<JunkItem> {
    let root = PathBuf::from(r"C:\Windows\Prefetch");
    scan_directory_files(
        JunkCategory::Prefetch,
        &[root],
        cancel,
        progress,
        ItemStatus::Normal,
        Some(&["pf"]),
    )
}

pub fn scan_prefetch_needs_admin() -> Vec<JunkItem> {
    vec![placeholder_admin(JunkCategory::Prefetch, r"C:\Windows\Prefetch")]
}

pub fn scan_windows_update(cancel: &AtomicBool, progress: &Sender<ScanProgress>) -> Vec<JunkItem> {
    let root = PathBuf::from(r"C:\Windows\SoftwareDistribution\Download");
    scan_directory_files(
        JunkCategory::WindowsUpdate,
        &[root],
        cancel,
        progress,
        ItemStatus::Normal,
        None,
    )
}

pub fn scan_windows_update_needs_admin() -> Vec<JunkItem> {
    vec![placeholder_admin(
        JunkCategory::WindowsUpdate,
        r"C:\Windows\SoftwareDistribution\Download",
    )]
}

pub fn scan_windows_logs(cancel: &AtomicBool, progress: &Sender<ScanProgress>) -> Vec<JunkItem> {
    let root = PathBuf::from(r"C:\Windows\Logs");
    scan_logs(JunkCategory::WindowsLogs, &root, cancel, progress)
}

pub fn scan_windows_logs_needs_admin() -> Vec<JunkItem> {
    vec![placeholder_admin(JunkCategory::WindowsLogs, r"C:\Windows\Logs")]
}

fn scan_logs(
    category: JunkCategory,
    root: &PathBuf,
    cancel: &AtomicBool,
    progress: &Sender<ScanProgress>,
) -> Vec<JunkItem> {
    if !root.exists() {
        return Vec::new();
    }

    let mut items = Vec::new();
    let mut truncated = false;

    for entry in WalkDir::new(root)
        .max_depth(8)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        if ext != "log" && ext != "etl" {
            continue;
        }

        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        items.push(JunkItem::file(
            category,
            path.to_path_buf(),
            size,
            ItemStatus::Normal,
        ));

        if items.len() % 200 == 0 {
            let _ = progress.send(ScanProgress {
                category,
                files_found: items.len(),
                message: format!("{}: 已找到 {} 项", category.label(), items.len()),
            });
        }

        if items.len() >= MAX_ITEMS_PER_CATEGORY {
            truncated = true;
            break;
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
