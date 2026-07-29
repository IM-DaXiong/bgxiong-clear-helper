# 比格熊C盘清理助手 / bgxiong-clear-helper

Windows C 盘垃圾文件清理工具，带图形界面。扫描常见垃圾文件与失效注册表项，由用户手动勾选确认后再删除。

| | |
|---|---|
| **中文名** | 比格熊C盘清理助手 |
| **英文名** | bgxiong-clear-helper |
| **作者** | im.daxiong（[bgxiong.ai@gmail.com](mailto:bgxiong.ai@gmail.com)） |
| **官网** | [bgxiong.com](https://bgxiong.com) |
| **仓库** | [github.com/IM-DaXiong/bgxiong-clear-helper](https://github.com/IM-DaXiong/bgxiong-clear-helper) |
| **许可证** | [PolyForm Noncommercial](LICENSE)（个人/非商免费；商用需授权） |

## 下载使用（推荐）

无需安装 Rust，下载后双击即可运行：

- [`release/bgxiong-clear-helper.exe`](release/bgxiong-clear-helper.exe)（Windows x64，约 4.8 MB）

清理系统目录 / 注册表时，建议右键「以管理员身份运行」。

## 功能

扫描以下类别：

| 类别 | 说明 | 权限 |
|------|------|------|
| 用户临时文件 | `%TEMP%`、`%LOCALAPPDATA%\Temp` | 普通用户 |
| 系统临时文件 | `C:\Windows\Temp` | 管理员 |
| 回收站 | 回收站中的文件 | 普通用户 |
| 缩略图缓存 | Explorer 缩略图数据库 | 普通用户 |
| Prefetch | `C:\Windows\Prefetch\*.pf` | 管理员 |
| Edge 缓存 | Edge 浏览器（HTTP/GPU/代码/媒体缓存、Cookie、本地存储二级分类） | 普通用户 |
| Chrome 缓存 | Chrome 浏览器（同上二级分类） | 普通用户 |
| Windows 更新缓存 | `SoftwareDistribution\Download` | 管理员 |
| 系统日志 | `C:\Windows\Logs` 中的 `.log`/`.etl` | 管理员 |
| 软件缓存 | `%LOCALAPPDATA%` / `%APPDATA%` 下常见软件 Cache 等 | 普通用户 |
| 软件日志/崩溃 | 日志、CrashDumps 等（不含 settings 本体） | 普通用户 |
| 失效注册表 | 卸载项 / App Paths / Run 中指向已删除文件的项 | 管理员（HKLM） |

**浏览器二级分类：** 进入 Edge/Chrome 详情后，可按 HTTP 缓存、GPU 缓存、代码缓存、媒体缓存、Cookie、本地存储分别勾选。

**注册表清理：** 清理前自动备份到 `%TEMP%\bgxiong-reg-backup\`（含 manifest 与 `reg export` 导出）。

**安全设计：**
- 仅扫描白名单 / 定向路径，不递归整个 C 盘
- 默认全部不勾选，需用户主动选择
- 清理前二次确认对话框
- 删除失败时记录原因，不中断整批操作

详细设计见 [`docs/feature-plan-browser-appdata-registry.md`](docs/feature-plan-browser-appdata-registry.md)。

## 构建

需要安装 [Rust](https://rustup.rs/)（stable 工具链）。

### 方式一：一键打包（推荐）

双击 **`package.bat`**（或 `build.bat`），或在命令行执行：

```powershell
.\package.bat
```

脚本会：
1. 从 `Cargo.toml` 读取版本号
2. 执行 `cargo build --release`
3. 输出到 `dist\`：可直接运行的 exe、带版本号的发布目录、以及 zip

```
dist\bgxiong-clear-helper.exe
dist\bgxiong-clear-helper-v{version}-windows-x64\
dist\bgxiong-clear-helper-v{version}-windows-x64.zip
```

### 方式二：手动构建

```powershell
cd bgxiong-clear-helper
cargo build --release
```

生成的可执行文件：

```
target\release\bgxiong-clear-helper.exe
```

双击即可运行，无需安装 Rust 运行时。

## 使用说明

1. 启动程序（建议清理系统目录 / 注册表时右键「以管理员身份运行」，或点击界面内「以管理员重新启动」）
2. 点击 **开始扫描**，等待扫描完成
3. 在 **类别总览** 中勾选要清理的类别，或进入单个类别查看/勾选
4. Edge/Chrome 详情中可使用顶部二级分类批量勾选
5. 窗口 **底部** 点击 **清理选中项**（始终可见），在确认对话框中再次确认
6. 查看清理结果（成功数、失败数、释放空间）

**界面说明：**
- 默认显示「类别总览」，不会因几万文件卡顿
- 点击「查看」或左侧类别名可进入详情，使用虚拟滚动仅渲染可见行
- 左侧类别前的复选框可一键选中/取消整个类别
- 底部提供「全选可清理」「全不选」快捷按钮

**建议：**
- 清理浏览器缓存前，先关闭 Edge / Chrome
- 清理 Cookie / 本地存储会导致网站退出登录
- 清理 Windows 更新缓存前，确认没有挂起的系统更新
- 清理注册表前确认备份目录可写；需要时可从 `%TEMP%\bgxiong-reg-backup\` 恢复

## 手动测试清单

- [ ] 关闭浏览器后扫描，Edge/Chrome 可见二级子类
- [ ] 浏览器运行中，占用文件显示为「跳过」
- [ ] 二级勾选仅影响对应子类
- [ ] 「软件缓存」能扫到本机已装软件的 Cache（如 VS Code / Discord）
- [ ] 「软件日志/崩溃」不含 settings.json / Preferences
- [ ] 非管理员：失效注册表中 HKLM 显示「需管理员」
- [ ] 管理员：可清理失效项，并生成 `%TEMP%\bgxiong-reg-backup\`
- [ ] 既有临时文件 / 回收站等类别行为正常

## 技术栈

- Rust + [eframe](https://github.com/emilk/egui) / egui（原生 GUI）
- walkdir + rayon（并行文件扫描）
- windows crate（回收站、管理员检测、注册表）

## 许可证

本项目采用 [PolyForm Noncommercial License 1.0.0](LICENSE)。

| 用途 | 是否允许 |
|------|----------|
| 个人使用、学习、爱好项目 | 免费允许 |
| 慈善、教育、科研、政府等非营利组织 | 免费允许 |
| 商业使用（含公司内部商用、转售、捆绑销售等） | **需另行授权** |

商用授权请联系：[bgxiong.ai@gmail.com](mailto:bgxiong.ai@gmail.com) · 官网：[bgxiong.com](https://bgxiong.com)

完整条款见仓库根目录 [`LICENSE`](LICENSE)。本协议**不是** OSI 意义上的宽松开源协议（如 MIT）：源代码可公开查阅与非商用使用，商用须获作者授权。

---

作者：[im.daxiong](mailto:bgxiong.ai@gmail.com)（bgxiong.ai@gmail.com） · 官网：[bgxiong.com](https://bgxiong.com)
