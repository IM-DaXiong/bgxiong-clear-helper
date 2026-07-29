use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;

use crate::models::{ItemStatus, JunkCategory, JunkItem, ScanProgress};
use crate::util::env_paths;

pub fn scan_thumbnail_cache(
    _cancel: &AtomicBool,
    _progress: &Sender<ScanProgress>,
) -> Vec<JunkItem> {
    let explorer_dirs: Vec<PathBuf> = env_paths(&["LOCALAPPDATA"])
        .into_iter()
        .map(|p| p.join(r"Microsoft\Windows\Explorer"))
        .filter(|p| p.exists())
        .collect();

    let mut items = Vec::new();

    for dir in explorer_dirs {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");

                if name.starts_with("thumbcache_") && name.ends_with(".db") {
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    items.push(JunkItem::file(
                        JunkCategory::ThumbnailCache,
                        path,
                        size,
                        ItemStatus::Normal,
                    ));
                }
            }
        }
    }

    items
}
