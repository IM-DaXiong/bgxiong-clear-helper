use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use walkdir::WalkDir;

use crate::models::{
    ItemStatus, JunkCategory, JunkItem, JunkTarget, ScanProgress, MAX_ITEMS_PER_CATEGORY,
};
use crate::util::{env_paths, file_size};

/// Named whitelist entries: (app_name, relative path under LOCALAPPDATA or APPDATA, is_cache).
struct NamedTarget {
    app_name: &'static str,
    relative: &'static str,
    is_cache: bool,
    /// If true, expand one directory level for `*` segment.
    glob_one: bool,
}

const NAMED_TARGETS: &[NamedTarget] = &[
    NamedTarget {
        app_name: "Discord",
        relative: r"discord\Cache",
        is_cache: true,
        glob_one: false,
    },
    NamedTarget {
        app_name: "Discord",
        relative: r"discord\Code Cache",
        is_cache: true,
        glob_one: false,
    },
    NamedTarget {
        app_name: "Discord",
        relative: r"discord\GPUCache",
        is_cache: true,
        glob_one: false,
    },
    NamedTarget {
        app_name: "Steam",
        relative: r"Steam\htmlcache",
        is_cache: true,
        glob_one: false,
    },
    NamedTarget {
        app_name: "Steam",
        relative: r"Steam\appcache\httpcache",
        is_cache: true,
        glob_one: false,
    },
    NamedTarget {
        app_name: "VS Code",
        relative: r"Code\Cache",
        is_cache: true,
        glob_one: false,
    },
    NamedTarget {
        app_name: "VS Code",
        relative: r"Code\CachedData",
        is_cache: true,
        glob_one: false,
    },
    NamedTarget {
        app_name: "VS Code",
        relative: r"Code\Code Cache",
        is_cache: true,
        glob_one: false,
    },
    NamedTarget {
        app_name: "VS Code",
        relative: r"Code\GPUCache",
        is_cache: true,
        glob_one: false,
    },
    NamedTarget {
        app_name: "Cursor",
        relative: r"Cursor\Cache",
        is_cache: true,
        glob_one: false,
    },
    NamedTarget {
        app_name: "Cursor",
        relative: r"Cursor\CachedData",
        is_cache: true,
        glob_one: false,
    },
    NamedTarget {
        app_name: "Cursor",
        relative: r"Cursor\Code Cache",
        is_cache: true,
        glob_one: false,
    },
    NamedTarget {
        app_name: "Cursor",
        relative: r"Cursor\GPUCache",
        is_cache: true,
        glob_one: false,
    },
    NamedTarget {
        app_name: "JetBrains",
        relative: r"JetBrains\*\caches",
        is_cache: true,
        glob_one: true,
    },
    NamedTarget {
        app_name: "npm",
        relative: r"npm-cache",
        is_cache: true,
        glob_one: false,
    },
    NamedTarget {
        app_name: "yarn",
        relative: r"Yarn\Cache",
        is_cache: true,
        glob_one: false,
    },
    NamedTarget {
        app_name: "pnpm",
        relative: r"pnpm-cache",
        is_cache: true,
        glob_one: false,
    },
    NamedTarget {
        app_name: "NuGet",
        relative: r"NuGet\v3-cache",
        is_cache: true,
        glob_one: false,
    },
    NamedTarget {
        app_name: "pip",
        relative: r"pip\Cache",
        is_cache: true,
        glob_one: false,
    },
    NamedTarget {
        app_name: "Telegram",
        relative: r"Telegram Desktop\tdata\user_data",
        is_cache: true,
        glob_one: false,
    },
    NamedTarget {
        app_name: "Spotify",
        relative: r"Spotify\Storage",
        is_cache: true,
        glob_one: false,
    },
    NamedTarget {
        app_name: "Spotify",
        relative: r"Spotify\Data",
        is_cache: true,
        glob_one: false,
    },
    NamedTarget {
        app_name: "Firefox",
        relative: r"Mozilla\Firefox\Profiles\*\cache2",
        is_cache: true,
        glob_one: true,
    },
    NamedTarget {
        app_name: "WeChat",
        relative: r"Tencent\WeChat\*\Cache",
        is_cache: true,
        glob_one: true,
    },
    // Config-side: logs / crash dumps under known apps
    NamedTarget {
        app_name: "VS Code",
        relative: r"Code\logs",
        is_cache: false,
        glob_one: false,
    },
    NamedTarget {
        app_name: "Cursor",
        relative: r"Cursor\logs",
        is_cache: false,
        glob_one: false,
    },
    NamedTarget {
        app_name: "Discord",
        relative: r"discord\logs",
        is_cache: false,
        glob_one: false,
    },
    NamedTarget {
        app_name: "JetBrains",
        relative: r"JetBrains\*\log",
        is_cache: false,
        glob_one: true,
    },
];

const CACHE_DIR_NAMES: &[&str] = &[
    "Cache",
    "Caches",
    "cache",
    "Code Cache",
    "GPUCache",
    "Temp",
    "tmp",
];

const CONFIG_DIR_NAMES: &[&str] = &["logs", "Log", "CrashDumps"];

const EXCLUDE_PREFIXES: &[&str] = &[
    r"Microsoft\Edge\User Data",
    r"Google\Chrome\User Data",
    r"Microsoft\Windows\Explorer",
    "Temp",
];

pub fn scan_app_caches(cancel: &AtomicBool, progress: &Sender<ScanProgress>) -> Vec<JunkItem> {
    scan_appdata(JunkCategory::AppCaches, true, cancel, progress)
}

pub fn scan_app_configs(cancel: &AtomicBool, progress: &Sender<ScanProgress>) -> Vec<JunkItem> {
    scan_appdata(JunkCategory::AppConfigs, false, cancel, progress)
}

fn scan_appdata(
    category: JunkCategory,
    want_cache: bool,
    cancel: &AtomicBool,
    progress: &Sender<ScanProgress>,
) -> Vec<JunkItem> {
    let roots = env_paths(&["LOCALAPPDATA", "APPDATA"]);
    let mut items = Vec::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

    // Named whitelist
    for target in NAMED_TARGETS {
        if target.is_cache != want_cache {
            continue;
        }
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        for root in &roots {
            for dir in resolve_named(root, target) {
                if !dir.is_dir() {
                    continue;
                }
                if is_excluded(root, &dir) {
                    continue;
                }
                collect_files(
                    category,
                    &dir,
                    Some(target.app_name.to_string()),
                    cancel,
                    progress,
                    &mut items,
                    &mut seen,
                );
                if items.len() >= MAX_ITEMS_PER_CATEGORY {
                    return items;
                }
            }
        }
    }

    // Heuristic directory names
    for root in &roots {
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        for entry in WalkDir::new(root)
            .max_depth(4)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if cancel.load(Ordering::Relaxed) {
                break;
            }

            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if is_excluded(root, path) {
                continue;
            }

            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let match_cache = CACHE_DIR_NAMES.contains(&name);
            let match_config = CONFIG_DIR_NAMES.contains(&name);

            if want_cache && !match_cache {
                continue;
            }
            if !want_cache && !match_config {
                continue;
            }

            let app_name = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .map(|s| s.to_string());

            collect_files(
                category,
                path,
                app_name,
                cancel,
                progress,
                &mut items,
                &mut seen,
            );

            if items.len() >= MAX_ITEMS_PER_CATEGORY {
                let _ = progress.send(ScanProgress {
                    category,
                    files_found: items.len(),
                    message: format!("{}: 已达展示上限", category.label()),
                });
                return items;
            }
        }
    }

    items
}

fn resolve_named(root: &Path, target: &NamedTarget) -> Vec<PathBuf> {
    if !target.glob_one {
        return vec![root.join(target.relative)];
    }

    // Expand a single `*` segment, e.g. JetBrains\*\caches
    let parts: Vec<&str> = target.relative.split('\\').collect();
    let Some(star_idx) = parts.iter().position(|p| *p == "*") else {
        return vec![root.join(target.relative)];
    };

    let prefix: PathBuf = parts[..star_idx].iter().fold(root.to_path_buf(), |a, p| a.join(p));
    let suffix: PathBuf = parts[star_idx + 1..].iter().fold(PathBuf::new(), |a, p| a.join(p));

    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&prefix) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let full = if suffix.as_os_str().is_empty() {
                    path
                } else {
                    path.join(&suffix)
                };
                out.push(full);
            }
        }
    }
    out
}

fn is_excluded(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    let rel_str = rel.to_string_lossy().replace('/', "\\");
    EXCLUDE_PREFIXES.iter().any(|prefix| {
        rel_str.eq_ignore_ascii_case(prefix)
            || rel_str
                .to_ascii_lowercase()
                .starts_with(&format!("{}\\", prefix.to_ascii_lowercase()))
    })
}

fn collect_files(
    category: JunkCategory,
    dir: &Path,
    app_name: Option<String>,
    cancel: &AtomicBool,
    progress: &Sender<ScanProgress>,
    items: &mut Vec<JunkItem>,
    seen: &mut std::collections::HashSet<PathBuf>,
) {
    for entry in WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if cancel.load(Ordering::Relaxed) {
            return;
        }

        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        // Never collect settings bodies into AppConfigs
        if category == JunkCategory::AppConfigs {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if name == "settings.json"
                || name == "preferences"
                || name == "secure preferences"
                || name.ends_with(".json") && name.contains("setting")
            {
                continue;
            }
        }

        if !seen.insert(path.to_path_buf()) {
            continue;
        }

        let size = file_size(path);
        items.push(JunkItem {
            category,
            path: path.to_path_buf(),
            size,
            status: ItemStatus::Normal,
            selected: false,
            skip_reason: None,
            is_recycle_bin: false,
            browser_sub: None,
            app_name: app_name.clone(),
            target: JunkTarget::File(path.to_path_buf()),
        });

        if items.len() % 500 == 0 {
            let _ = progress.send(ScanProgress {
                category,
                files_found: items.len(),
                message: format!("{}: 已找到 {} 项", category.label(), items.len()),
            });
        }

        if items.len() >= MAX_ITEMS_PER_CATEGORY {
            return;
        }
    }
}
