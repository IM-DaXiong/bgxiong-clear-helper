use std::path::PathBuf;

pub const MAX_ITEMS_PER_CATEGORY: usize = 50_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JunkCategory {
    UserTemp,
    SystemTemp,
    RecycleBin,
    ThumbnailCache,
    Prefetch,
    EdgeCache,
    ChromeCache,
    WindowsUpdate,
    WindowsLogs,
    AppCaches,
    AppConfigs,
    RegistryOrphans,
}

impl JunkCategory {
    pub fn all() -> &'static [JunkCategory] {
        &[
            JunkCategory::UserTemp,
            JunkCategory::SystemTemp,
            JunkCategory::RecycleBin,
            JunkCategory::ThumbnailCache,
            JunkCategory::Prefetch,
            JunkCategory::EdgeCache,
            JunkCategory::ChromeCache,
            JunkCategory::WindowsUpdate,
            JunkCategory::WindowsLogs,
            JunkCategory::AppCaches,
            JunkCategory::AppConfigs,
            JunkCategory::RegistryOrphans,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            JunkCategory::UserTemp => "用户临时文件",
            JunkCategory::SystemTemp => "系统临时文件",
            JunkCategory::RecycleBin => "回收站",
            JunkCategory::ThumbnailCache => "缩略图缓存",
            JunkCategory::Prefetch => "Prefetch",
            JunkCategory::EdgeCache => "Edge 缓存",
            JunkCategory::ChromeCache => "Chrome 缓存",
            JunkCategory::WindowsUpdate => "Windows 更新缓存",
            JunkCategory::WindowsLogs => "系统日志",
            JunkCategory::AppCaches => "软件缓存",
            JunkCategory::AppConfigs => "软件日志/崩溃",
            JunkCategory::RegistryOrphans => "失效注册表",
        }
    }

    pub fn requires_admin(self) -> bool {
        matches!(
            self,
            JunkCategory::SystemTemp
                | JunkCategory::Prefetch
                | JunkCategory::WindowsUpdate
                | JunkCategory::WindowsLogs
                | JunkCategory::RegistryOrphans
        )
    }

    pub fn warning(self) -> Option<&'static str> {
        match self {
            JunkCategory::WindowsUpdate => Some(
                "清理 Windows 更新缓存可能影响待安装更新，请确认无挂起更新后再清理。",
            ),
            JunkCategory::EdgeCache | JunkCategory::ChromeCache => {
                Some("建议关闭浏览器后再清理。清理 Cookie/本地存储将退出网站登录。")
            }
            JunkCategory::AppCaches => Some("建议关闭对应应用后再清理缓存。"),
            JunkCategory::AppConfigs => {
                Some("仅包含日志与崩溃转储等产物，不包含软件设置文件。")
            }
            JunkCategory::RegistryOrphans => Some(
                "误删可能导致卸载项或启动快捷方式异常；清理前将自动备份为 .reg 文件。",
            ),
            _ => None,
        }
    }

    pub fn is_browser(self) -> bool {
        matches!(self, JunkCategory::EdgeCache | JunkCategory::ChromeCache)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BrowserSubKind {
    HttpCache,
    GpuCache,
    CodeCache,
    MediaCache,
    Cookies,
    LocalStorage,
}

impl BrowserSubKind {
    pub fn all() -> &'static [BrowserSubKind] {
        &[
            BrowserSubKind::HttpCache,
            BrowserSubKind::GpuCache,
            BrowserSubKind::CodeCache,
            BrowserSubKind::MediaCache,
            BrowserSubKind::Cookies,
            BrowserSubKind::LocalStorage,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            BrowserSubKind::HttpCache => "HTTP 缓存",
            BrowserSubKind::GpuCache => "GPU 缓存",
            BrowserSubKind::CodeCache => "代码缓存",
            BrowserSubKind::MediaCache => "媒体缓存",
            BrowserSubKind::Cookies => "Cookie",
            BrowserSubKind::LocalStorage => "本地存储",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            BrowserSubKind::HttpCache => "HTTP",
            BrowserSubKind::GpuCache => "GPU",
            BrowserSubKind::CodeCache => "Code",
            BrowserSubKind::MediaCache => "Media",
            BrowserSubKind::Cookies => "Cookie",
            BrowserSubKind::LocalStorage => "Storage",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JunkTarget {
    File(PathBuf),
    RegistryKey {
        key: String,
        value_name: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemStatus {
    Normal,
    NeedsAdmin,
    Skipped,
}

impl ItemStatus {
    pub fn label(self) -> &'static str {
        match self {
            ItemStatus::Normal => "正常",
            ItemStatus::NeedsAdmin => "需管理员",
            ItemStatus::Skipped => "跳过",
        }
    }
}

#[derive(Debug, Clone)]
pub struct JunkItem {
    pub category: JunkCategory,
    /// Display path (file path or registry key path).
    pub path: PathBuf,
    pub size: u64,
    pub status: ItemStatus,
    pub selected: bool,
    pub skip_reason: Option<String>,
    /// Recycle bin aggregate item (empty path, use category-level clean).
    pub is_recycle_bin: bool,
    pub browser_sub: Option<BrowserSubKind>,
    pub app_name: Option<String>,
    pub target: JunkTarget,
}

impl JunkItem {
    pub fn file(category: JunkCategory, path: PathBuf, size: u64, status: ItemStatus) -> Self {
        Self {
            category,
            path: path.clone(),
            size,
            status,
            selected: false,
            skip_reason: None,
            is_recycle_bin: false,
            browser_sub: None,
            app_name: None,
            target: JunkTarget::File(path),
        }
    }

    pub fn can_clean(&self, is_admin: bool) -> bool {
        match self.status {
            ItemStatus::Skipped => false,
            ItemStatus::NeedsAdmin => is_admin,
            ItemStatus::Normal => true,
        }
    }

    pub fn is_registry(&self) -> bool {
        matches!(self.target, JunkTarget::RegistryKey { .. })
    }
}

#[derive(Debug, Clone)]
pub struct ScanProgress {
    pub category: JunkCategory,
    pub files_found: usize,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppPhase {
    Idle,
    Scanning,
    Results,
    Confirming,
    Cleaning,
}

#[derive(Debug, Default, Clone)]
pub struct CategorySummary {
    pub count: usize,
    pub total_size: u64,
    pub truncated: bool,
}

#[derive(Debug, Default, Clone)]
pub struct CleanResult {
    pub success_count: usize,
    pub fail_count: usize,
    pub freed_bytes: u64,
    pub errors: Vec<CleanError>,
}

#[derive(Debug, Clone)]
pub struct CleanError {
    pub path: PathBuf,
    pub reason: String,
}
