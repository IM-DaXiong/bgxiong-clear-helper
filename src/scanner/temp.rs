use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use walkdir::WalkDir;

use crate::models::{ItemStatus, JunkCategory, JunkItem, ScanProgress, MAX_ITEMS_PER_CATEGORY};
use crate::util::{env_paths, file_size, is_locked_error};

pub fn scan_user_temp(
    cancel: &AtomicBool,
    progress: &Sender<ScanProgress>,
) -> Vec<JunkItem> {
    let mut paths = env_paths(&["TEMP", "TMP"]);
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let local_temp = PathBuf::from(&local).join("Temp");
        if local_temp.exists() {
            paths.push(local_temp);
        }
    }
    paths.sort();
    paths.dedup();

    scan_directory_files(
        JunkCategory::UserTemp,
        &paths,
        cancel,
        progress,
        ItemStatus::Normal,
        None,
    )
}

pub fn scan_system_temp(
    cancel: &AtomicBool,
    progress: &Sender<ScanProgress>,
) -> Vec<JunkItem> {
    let root = PathBuf::from(r"C:\Windows\Temp");
    scan_directory_files(
        JunkCategory::SystemTemp,
        &[root],
        cancel,
        progress,
        ItemStatus::Normal,
        None,
    )
}

pub fn scan_system_temp_needs_admin() -> Vec<JunkItem> {
    vec![placeholder_admin(JunkCategory::SystemTemp, r"C:\Windows\Temp")]
}

pub fn scan_directory_files(
    category: JunkCategory,
    roots: &[PathBuf],
    cancel: &AtomicBool,
    progress: &Sender<ScanProgress>,
    status: ItemStatus,
    extensions: Option<&[&str]>,
) -> Vec<JunkItem> {
    let mut items = Vec::new();
    let mut truncated = false;

    'roots: for root in roots {
        if !root.exists() {
            continue;
        }

        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if cancel.load(Ordering::Relaxed) {
                break 'roots;
            }

            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            if let Some(exts) = extensions {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if !exts.iter().any(|e| *e == ext) {
                    continue;
                }
            }

            let size = file_size(path);
            items.push(JunkItem::file(
                category,
                path.to_path_buf(),
                size,
                status,
            ));

            if items.len() % 500 == 0 {
                let _ = progress.send(ScanProgress {
                    category,
                    files_found: items.len(),
                    message: format!("{}: 已找到 {} 项", category.label(), items.len()),
                });
            }

            if items.len() >= MAX_ITEMS_PER_CATEGORY {
                truncated = true;
                break 'roots;
            }
        }
    }

    if truncated {
        let _ = progress.send(ScanProgress {
            category,
            files_found: items.len(),
            message: format!(
                "{}: 已达上限 {} 项，其余可通过汇总大小查看",
                category.label(),
                MAX_ITEMS_PER_CATEGORY
            ),
        });
    }

    items
}

pub fn placeholder_admin(category: JunkCategory, path: &str) -> JunkItem {
    let mut item = JunkItem::file(
        category,
        PathBuf::from(path),
        0,
        ItemStatus::NeedsAdmin,
    );
    item.skip_reason = Some("需要管理员权限才能扫描和清理".into());
    item
}

pub fn check_browser_path(path: &Path) -> ItemStatus {
    if let Err(e) = std::fs::OpenOptions::new().write(true).open(path) {
        if is_locked_error(&e) {
            return ItemStatus::Skipped;
        }
    }
    ItemStatus::Normal
}
