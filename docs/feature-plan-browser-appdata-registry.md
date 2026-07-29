# 清理功能扩展设计：浏览器二级分类 · 软件缓存/配置 · 失效注册表

> 版本：0.2 · 状态：已定稿 · 实现以此文档为单一事实来源

## 1. 背景与目标

现有 BGXiong C 盘清理助手（Rust + eframe/egui）提供 9 类白名单扫描，浏览器仅匹配 `Cache` / `GPUCache` / `Code Cache`，不覆盖 Cookie/媒体缓存，不扫描 `%APPDATA%` 下常见软件缓存，也不具备注册表清理能力。

本期目标：

1. **浏览器二级分类**：Edge / Chrome 下按 HTTP 缓存、GPU、代码缓存、媒体缓存、Cookie、本地存储等子类勾选清理。
2. **用户目录软件扫描**：将 `%LOCALAPPDATA%` / `%APPDATA%` 下常见软件缓存与可安全清理的日志/崩溃产物纳入扫描。
3. **失效注册表清理**：定向扫描「指向已删除文件」的注册表项，支持备份后删除。

安全原则不变：白名单/定向扫描、默认不勾选、二次确认、失败不中断整批。

## 2. 架构总览

```
scan_all (rayon)
├── browser.rs          → EdgeCache / ChromeCache + BrowserSubKind
├── appdata.rs          → AppCaches / AppConfigs
├── registry.rs         → RegistryOrphans (JunkTarget::RegistryKey)
├── temp / system / …   → 既有类别
↓
JunkItem (统一列表)
↓
app.rs UI：类别总览 + 浏览器二级勾选条 + 文件/注册表行
↓
cleaner.rs：文件删除 | 注册表备份+.reg 后删键/值
```

## 3. 数据模型

### 3.1 新增 / 变更类型

```rust
pub enum BrowserSubKind {
    HttpCache,    // HTTP 缓存
    GpuCache,     // GPU 缓存
    CodeCache,    // 代码缓存
    MediaCache,   // 媒体 / SW CacheStorage
    Cookies,      // Cookie
    LocalStorage, // Local/Session Storage、IndexedDB
}

pub enum JunkTarget {
    File(PathBuf),
    RegistryKey {
        key: String,                 // 如 HKCU\Software\...
        value_name: Option<String>,  // None = 删除整个子键
    },
}

// JunkCategory 新增：
// AppCaches, AppConfigs, RegistryOrphans

// JunkItem 新增字段：
// browser_sub: Option<BrowserSubKind>
// app_name: Option<String>
// target: JunkTarget
```

### 3.2 权限与警告

| 类别 | 管理员 | warning |
|------|--------|---------|
| EdgeCache / ChromeCache | 否 | 建议关闭浏览器；Cookie/本地存储会退出网站登录 |
| AppCaches | 否 | 关闭对应应用后再清理更稳妥 |
| AppConfigs | 否 | 仅日志/崩溃等产物；误删可能影响诊断信息 |
| RegistryOrphans | 是（HKLM） | 误删可能导致卸载项/快捷方式异常；清理前自动备份 .reg |

## 4. 浏览器二级分类

### 4.1 扫描根路径

| 类别 | 根路径 |
|------|--------|
| EdgeCache | `%LOCALAPPDATA%\Microsoft\Edge\User Data` |
| ChromeCache | `%LOCALAPPDATA%\Google\Chrome\User Data` |

### 4.2 Profile 发现

在 User Data 下识别：

- `Default`
- `Profile *`（如 `Profile 1`）
- 可选：`Guest Profile`（若存在）

不对整个 User Data 无差别深扫；以 Profile 为根再按子类匹配。

### 4.3 子类路径表

相对各 Profile 目录：

| BrowserSubKind | 显示名 | 匹配 |
|----------------|--------|------|
| HttpCache | HTTP 缓存 | 目录名 `Cache`，或路径含 `\Cache\`（排除 Code Cache） |
| GpuCache | GPU 缓存 | `GPUCache` |
| CodeCache | 代码缓存 | `Code Cache` |
| MediaCache | 媒体缓存 | `Media Cache`；`Service Worker\CacheStorage` |
| Cookies | Cookie | `Network\Cookies`、`Cookies`、`Network\Cookies-journal` |
| LocalStorage | 本地存储 | `Local Storage`、`Session Storage`、`IndexedDB` |

规则：

- 收集匹配目录下的**文件**（与现网一致）；Cookie DB 本身作为文件项。
- 占用中的文件标记 `Skipped`（浏览器可能正在运行）。
- 所有项默认 `selected: false`；LocalStorage / Cookies 在 UI 二级条上额外强调风险。

### 4.4 UI

进入 Edge/Chrome 详情时，顶部显示二级复选条：

- 每个子类一个 Checkbox，勾选/取消 = 批量设置该子类下所有 `can_clean` 项。
- 列表行前缀短标签，如 `[Cookie]`、`[HTTP]`。
- 类别总览仍一行显示 Edge/Chrome 合计，不挤占空间。

## 5. 软件缓存 / 配置扫描

### 5.1 新类别

| 类别 | 含义 |
|------|------|
| AppCaches | 软件缓存目录下的文件 |
| AppConfigs | 日志、CrashDumps、明确可删的临时产物（**不删** settings.json / Preferences 等设置本体） |

根：`%LOCALAPPDATA%`、`%APPDATA%`（当前用户）。

### 5.2 具名白名单（首期）

相对 `%LOCALAPPDATA%` 或 `%APPDATA%`（扫描时两根都尝试）：

| app_name | 相对路径 | 归类 |
|----------|----------|------|
| Discord | `discord\Cache`、`discord\Code Cache`、`discord\GPUCache` | AppCaches |
| Steam | `Steam\htmlcache`、`Steam\appcache\httpcache` | AppCaches |
| VS Code | `Code\Cache`、`Code\CachedData`、`Code\Code Cache`、`Code\GPUCache` | AppCaches |
| Cursor | `Cursor\Cache`、`Cursor\CachedData`、`Cursor\Code Cache`、`Cursor\GPUCache` | AppCaches |
| JetBrains | `JetBrains\*\caches`（一层通配） | AppCaches |
| npm | `npm-cache` | AppCaches |
| yarn | `Yarn\Cache` | AppCaches |
| pnpm | `pnpm-cache` | AppCaches |
| NuGet | `NuGet\v3-cache` | AppCaches |
| pip | `pip\Cache` | AppCaches |
| Telegram | `Telegram Desktop\tdata\user_data` | AppCaches |
| Spotify | `Spotify\Storage`、`Spotify\Data` | AppCaches |
| Adobe | `Adobe\*\Cache`（启发式内覆盖） | AppCaches |
| Firefox | `Mozilla\Firefox\Profiles\*\cache2` | AppCaches |
| WeChat | `Tencent\WeChat\*\Cache`（若存在） | AppCaches |
| 通用日志 | 白名单应用下的 `logs`、`Log`、`CrashDumps`、`*.log` | AppConfigs |

### 5.3 通用目录名启发式

在 `%LOCALAPPDATA%` / `%APPDATA%` 下深度有限遍历（建议 max_depth ≤ 4），目录名属于：

`Cache`、`Caches`、`cache`、`Code Cache`、`GPUCache`、`Temp`、`tmp`、`logs`、`Log`、`CrashDumps`

则归入 AppCaches（Cache*）或 AppConfigs（logs/CrashDumps）。

**排除前缀**（避免与既有类别重复）：

- `Microsoft\Edge\User Data`
- `Google\Chrome\User Data`
- `Microsoft\Windows\Explorer`
- `Temp`（已由 UserTemp 覆盖）

**不做**：`C:\Users\*` 全用户、`Program Files`、删除未知软件整棵配置树。

## 6. 注册表失效检测

### 6.1 「全量」定义

覆盖常见存放「文件系统路径」的注册表位置并校验路径是否存在；**不做** `HKEY_CLASSES_ROOT` 整树盲扫。

### 6.2 扫描范围

| Hive + 路径 | 检查值 | 失效条件 | 清理动作 |
|-------------|--------|----------|----------|
| HKLM/HKCU `\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*` | InstallLocation, DisplayIcon, UninstallString | 解析出的主路径不存在 | 删除该卸载子键 |
| 同上 `WOW6432Node\...\Uninstall\*` | 同上 | 同上 | 删除子键 |
| HKLM/HKCU `\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\*` | (默认), Path | exe 不存在 | 删除该 App Paths 子键 |
| HKLM/HKCU `\SOFTWARE\Microsoft\Windows\CurrentVersion\Run` | 各值 | 命令行首个可解析 exe 不存在 | 删除该值 |
| 同上 `RunOnce` | 各值 | 同上 | 删除该值 |

路径解析：

- 去掉引号；取命令行第一个 token；展开 `%VAR%`；若为相对路径则结合同键 `Path` 或忽略。
- DisplayIcon 去掉 `,0` 等图标索引后缀。

### 6.3 黑名单（永不扫描/删除）

键路径包含以下片段则跳过：

- `Windows NT\CurrentVersion\ProfileList`
- `Windows NT\CurrentVersion\Windows`
- `Microsoft\Windows NT\CurrentVersion\Winlogon`
- `CurrentControlSet\Services`

### 6.4 清理与备份

1. 清理前将待删项写入 `%TEMP%\bgxiong-reg-backup\<timestamp>.reg`（REGEDIT4/UTF-16 LE 标准格式）。
2. 仅删除命中的子键或值，不删父键。
3. HKLM 需管理员；非管理员对 HKLM 相关项标记 `NeedsAdmin`。
4. UI 确认框对含注册表项的清理追加提示。

## 7. UI 交互要点

- 类别总览自动列出全部 `JunkCategory::all()`（含 3 个新类）。
- 浏览器详情：二级子类勾选条。
- 注册表行：路径列显示完整键路径；可 hover 显示 `missing_path` / 原值（`skip_reason` 或扩展字段存于 reason 旁注）。
- 底部「清理选中项」逻辑不变；确认对话框若包含注册表项则显示备份提示。

## 8. 模块与文件清单

| 文件 | 改动 |
|------|------|
| `src/models.rs` | BrowserSubKind、JunkTarget、新类别、JunkItem 字段 |
| `src/scanner/browser.rs` | Profile + 子类匹配 |
| `src/scanner/appdata.rs` | 新建 |
| `src/scanner/registry.rs` | 新建 |
| `src/scanner/mod.rs` | 接入 |
| `src/scanner/temp.rs` 等 | JunkItem 构造适配 |
| `src/cleaner.rs` | 注册表备份与删除 |
| `src/app.rs` | 二级勾选、注册表展示、确认文案 |
| `Cargo.toml` | `Win32_System_Registry` |
| `README.md` | 功能表同步 |

## 9. 分阶段里程碑

1. 本文档落地（`docs/`）
2. 模型 + 浏览器二级分类（扫描 + UI）
3. AppCaches / AppConfigs
4. 注册表扫描展示 + NeedsAdmin
5. 注册表清理 + .reg 备份
6. README 与手动测试

## 10. 测试清单

- [ ] 关闭 Edge/Chrome 后扫描，可见 HTTP/GPU/Code/Media/Cookie/本地存储子类
- [ ] 浏览器运行中，占用文件为「跳过」
- [ ] 二级勾选仅影响对应子类
- [ ] AppCaches 能扫到本机已装软件的 Cache（如 VS Code / Discord）
- [ ] AppConfigs 不含 settings.json / Preferences
- [ ] 非管理员：RegistryOrphans 中 HKLM 项为「需管理员」
- [ ] 管理员：可勾选失效卸载项并清理；`%TEMP%\bgxiong-reg-backup\` 产生 .reg
- [ ] 清理后失败项仍留在列表；成功项移除
- [ ] 既有 9 类行为无回归

## 11. 未纳入本期

- Firefox 等独立顶层浏览器类别（Firefox cache2 已在 AppCaches 白名单）
- HKCR / CLSID / 壳扩展全量清理
- 多用户 `C:\Users\*` 扫描
- 云端规则更新
- StartupApproved 交叉（可后续迭代）
