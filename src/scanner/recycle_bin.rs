use std::path::PathBuf;

use crate::models::{ItemStatus, JunkCategory, JunkItem};

pub fn scan_recycle_bin() -> Vec<JunkItem> {
    let (count, size) = enumerate_recycle_bin();

    if count == 0 {
        return Vec::new();
    }

    let mut item = JunkItem::file(
        JunkCategory::RecycleBin,
        PathBuf::from("回收站"),
        size,
        ItemStatus::Normal,
    );
    item.is_recycle_bin = true;
    vec![item]
}

fn enumerate_recycle_bin() -> (usize, u64) {
    let recycle_root = PathBuf::from(r"C:\$Recycle.Bin");
    if !recycle_root.exists() {
        return (0, 0);
    }

    let mut count = 0usize;
    let mut total = 0u64;

    if let Ok(entries) = std::fs::read_dir(&recycle_root) {
        for sid in entries.flatten() {
            if let Ok(files) = std::fs::read_dir(sid.path()) {
                for file in files.flatten() {
                    let path = file.path();
                    if path.is_file() {
                        count += 1;
                        total += file.metadata().map(|m| m.len()).unwrap_or(0);
                    }
                }
            }
        }
    }

    (count, total)
}
