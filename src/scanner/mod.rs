mod appdata;
mod browser;
mod recycle_bin;
mod registry;
mod system;
mod temp;
mod thumbnail;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use rayon::prelude::*;

use crate::models::{JunkCategory, JunkItem, ScanProgress};
use crate::util::is_elevated;

pub use recycle_bin::scan_recycle_bin;

pub fn scan_all(cancel: &AtomicBool, progress: &Sender<ScanProgress>) -> Vec<JunkItem> {
    let is_admin = is_elevated();

    let results: Vec<Vec<JunkItem>> = JunkCategory::all()
        .par_iter()
        .map(|&category| {
            if cancel.load(Ordering::Relaxed) {
                return Vec::new();
            }

            let _ = progress.send(ScanProgress {
                category,
                files_found: 0,
                message: format!("正在扫描: {}", category.label()),
            });

            let items = scan_category(category, is_admin, cancel, progress);

            let _ = progress.send(ScanProgress {
                category,
                files_found: items.len(),
                message: format!("完成: {} ({} 项)", category.label(), items.len()),
            });

            items
        })
        .collect();

    let mut all: Vec<JunkItem> = results.into_iter().flatten().collect();
    all.sort_by(|a, b| {
        a.category
            .label()
            .cmp(b.category.label())
            .then_with(|| a.path.cmp(&b.path))
    });
    all
}

fn scan_category(
    category: JunkCategory,
    is_admin: bool,
    cancel: &AtomicBool,
    progress: &Sender<ScanProgress>,
) -> Vec<JunkItem> {
    match category {
        JunkCategory::UserTemp => temp::scan_user_temp(cancel, progress),
        JunkCategory::SystemTemp => {
            if is_admin {
                temp::scan_system_temp(cancel, progress)
            } else {
                temp::scan_system_temp_needs_admin()
            }
        }
        JunkCategory::RecycleBin => recycle_bin::scan_recycle_bin(),
        JunkCategory::ThumbnailCache => thumbnail::scan_thumbnail_cache(cancel, progress),
        JunkCategory::Prefetch => {
            if is_admin {
                system::scan_prefetch(cancel, progress)
            } else {
                system::scan_prefetch_needs_admin()
            }
        }
        JunkCategory::EdgeCache => browser::scan_edge_cache(cancel, progress),
        JunkCategory::ChromeCache => browser::scan_chrome_cache(cancel, progress),
        JunkCategory::WindowsUpdate => {
            if is_admin {
                system::scan_windows_update(cancel, progress)
            } else {
                system::scan_windows_update_needs_admin()
            }
        }
        JunkCategory::WindowsLogs => {
            if is_admin {
                system::scan_windows_logs(cancel, progress)
            } else {
                system::scan_windows_logs_needs_admin()
            }
        }
        JunkCategory::AppCaches => appdata::scan_app_caches(cancel, progress),
        JunkCategory::AppConfigs => appdata::scan_app_configs(cancel, progress),
        JunkCategory::RegistryOrphans => {
            registry::scan_registry_orphans(is_admin, cancel, progress)
        }
    }
}
