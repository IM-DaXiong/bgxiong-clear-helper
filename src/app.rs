use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;

use eframe::egui;

use crate::cleaner;
use crate::models::{
    AppPhase, BrowserSubKind, CategorySummary, CleanResult, ItemStatus, JunkCategory, JunkItem,
    ScanProgress,
};
use crate::scanner;
use crate::util::{format_size, is_elevated, restart_as_admin};

const ROW_HEIGHT: f32 = 22.0;

pub struct ClearHelperApp {
    phase: AppPhase,
    is_admin: bool,
    items: Vec<JunkItem>,
    filter_category: Option<JunkCategory>,
    scan_progress: String,
    scan_cancel: Option<Arc<AtomicBool>>,
    progress_rx: Option<Receiver<ScanProgress>>,
    scan_result_rx: Option<Receiver<Vec<JunkItem>>>,
    clean_result_rx: Option<Receiver<CleanResult>>,
    show_confirm_dialog: bool,
    show_result_dialog: bool,
    last_clean_result: Option<CleanResult>,
    elevate_error: Option<String>,
    search_filter: String,
    /// Cached category summaries, invalidated when items change.
    cached_summaries: Option<HashMap<JunkCategory, CategorySummary>>,
    /// Cached filtered row indices for detail view.
    cached_filter_key: String,
    cached_filtered_indices: Vec<usize>,
}

impl ClearHelperApp {
    pub fn new() -> Self {
        Self {
            phase: AppPhase::Idle,
            is_admin: is_elevated(),
            items: Vec::new(),
            filter_category: None,
            scan_progress: String::new(),
            scan_cancel: None,
            progress_rx: None,
            scan_result_rx: None,
            clean_result_rx: None,
            show_confirm_dialog: false,
            show_result_dialog: false,
            last_clean_result: None,
            elevate_error: None,
            search_filter: String::new(),
            cached_summaries: None,
            cached_filter_key: String::new(),
            cached_filtered_indices: Vec::new(),
        }
    }

    fn invalidate_cache(&mut self) {
        self.cached_summaries = None;
        self.cached_filter_key.clear();
        self.cached_filtered_indices.clear();
    }

    fn category_summaries(&mut self) -> &HashMap<JunkCategory, CategorySummary> {
        if self.cached_summaries.is_none() {
            let mut map: HashMap<JunkCategory, CategorySummary> = HashMap::new();
            for item in &self.items {
                let entry = map.entry(item.category).or_default();
                entry.count += 1;
                entry.total_size += item.size;
            }
            self.cached_summaries = Some(map);
        }
        self.cached_summaries.as_ref().unwrap()
    }

    fn category_selected_count(&self, category: JunkCategory) -> (usize, u64) {
        self.items
            .iter()
            .filter(|i| i.category == category && i.selected && i.can_clean(self.is_admin))
            .fold((0, 0), |(c, s), i| (c + 1, s + i.size))
    }

    fn is_category_fully_selected(&self, category: JunkCategory) -> bool {
        let cleanable: Vec<_> = self
            .items
            .iter()
            .filter(|i| i.category == category && i.can_clean(self.is_admin))
            .collect();
        !cleanable.is_empty() && cleanable.iter().all(|i| i.selected)
    }

    fn is_category_partially_selected(&self, category: JunkCategory) -> bool {
        let cleanable: Vec<_> = self
            .items
            .iter()
            .filter(|i| i.category == category && i.can_clean(self.is_admin))
            .collect();
        let selected = cleanable.iter().filter(|i| i.selected).count();
        selected > 0 && selected < cleanable.len()
    }

    fn set_category_selection(&mut self, category: JunkCategory, selected: bool) {
        for item in &mut self.items {
            if item.category == category && item.can_clean(self.is_admin) {
                item.selected = selected;
            }
        }
    }

    fn set_browser_sub_selection(
        &mut self,
        category: JunkCategory,
        sub: BrowserSubKind,
        selected: bool,
    ) {
        for item in &mut self.items {
            if item.category == category
                && item.browser_sub == Some(sub)
                && item.can_clean(self.is_admin)
            {
                item.selected = selected;
            }
        }
    }

    fn is_browser_sub_fully_selected(&self, category: JunkCategory, sub: BrowserSubKind) -> bool {
        let cleanable: Vec<_> = self
            .items
            .iter()
            .filter(|i| {
                i.category == category && i.browser_sub == Some(sub) && i.can_clean(self.is_admin)
            })
            .collect();
        !cleanable.is_empty() && cleanable.iter().all(|i| i.selected)
    }

    fn browser_sub_count(&self, category: JunkCategory, sub: BrowserSubKind) -> usize {
        self.items
            .iter()
            .filter(|i| i.category == category && i.browser_sub == Some(sub))
            .count()
    }

    fn selected_has_registry(&self) -> bool {
        self.items
            .iter()
            .any(|i| i.selected && i.can_clean(self.is_admin) && i.is_registry())
    }

    fn select_all_cleanable(&mut self) {
        for item in &mut self.items {
            if item.can_clean(self.is_admin) {
                item.selected = true;
            }
        }
    }

    fn deselect_all(&mut self) {
        for item in &mut self.items {
            item.selected = false;
        }
    }

    fn filtered_indices(&mut self) -> &[usize] {
        let key = format!(
            "{:?}|{}",
            self.filter_category,
            self.search_filter.to_lowercase()
        );
        if key != self.cached_filter_key {
            self.cached_filtered_indices = self
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| {
                    if let Some(cat) = self.filter_category {
                        if item.category != cat {
                            return false;
                        }
                    }
                    if !self.search_filter.is_empty() {
                        let path = item.path.to_string_lossy().to_lowercase();
                        let filter = self.search_filter.to_lowercase();
                        if !path.contains(&filter) {
                            return false;
                        }
                    }
                    true
                })
                .map(|(i, _)| i)
                .collect();
            self.cached_filter_key = key;
        }
        &self.cached_filtered_indices
    }

    fn selected_stats(&self) -> (usize, u64) {
        self.items
            .iter()
            .filter(|i| i.selected && i.can_clean(self.is_admin))
            .fold((0, 0), |(c, s), i| (c + 1, s + i.size))
    }

    fn start_scan(&mut self) {
        self.phase = AppPhase::Scanning;
        self.items.clear();
        self.invalidate_cache();
        self.scan_progress = "正在启动扫描...".into();
        self.filter_category = None;

        let cancel = Arc::new(AtomicBool::new(false));
        self.scan_cancel = Some(cancel.clone());

        let (progress_tx, progress_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();

        self.progress_rx = Some(progress_rx);
        self.scan_result_rx = Some(result_rx);

        thread::spawn(move || {
            let items = scanner::scan_all(&cancel, &progress_tx);
            let _ = result_tx.send(items);
        });
    }

    fn poll_scan(&mut self) {
        if let Some(rx) = &self.progress_rx {
            while let Ok(progress) = rx.try_recv() {
                self.scan_progress = progress.message;
            }
        }

        if let Some(rx) = &self.scan_result_rx {
            if let Ok(items) = rx.try_recv() {
                self.items = items;
                self.invalidate_cache();
                self.phase = AppPhase::Results;
                self.scan_progress = format!("扫描完成，共 {} 项", self.items.len());
                self.progress_rx = None;
                self.scan_result_rx = None;
                self.scan_cancel = None;
            }
        }
    }

    fn poll_clean(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.clean_result_rx {
            if let Ok(result) = rx.try_recv() {
                self.finish_clean(result);
                self.clean_result_rx = None;
                ctx.request_repaint();
            }
        }
    }

    fn start_clean(&mut self) {
        self.phase = AppPhase::Cleaning;
        self.show_confirm_dialog = false;

        let to_clean: Vec<JunkItem> = self
            .items
            .iter()
            .filter(|i| i.selected && i.can_clean(self.is_admin))
            .cloned()
            .collect();

        let is_admin = self.is_admin;
        let (tx, rx) = mpsc::channel();
        self.clean_result_rx = Some(rx);

        thread::spawn(move || {
            let result = cleaner::clean_selected(&to_clean, is_admin);
            let _ = tx.send(result);
        });
    }

    fn finish_clean(&mut self, result: CleanResult) {
        let failed_paths: std::collections::HashSet<_> =
            result.errors.iter().map(|e| e.path.clone()).collect();

        let had_recycle = self.items.iter().any(|i| i.is_recycle_bin && i.selected);
        let recycle_failed = result
            .errors
            .iter()
            .any(|e| e.path.to_string_lossy() == "回收站");
        let registry_backup_failed = result
            .errors
            .iter()
            .any(|e| e.path.to_string_lossy() == "注册表备份");

        self.items.retain(|item| {
            if item.is_recycle_bin && had_recycle && !recycle_failed {
                return false;
            }
            if item.selected && item.can_clean(self.is_admin) && !item.is_recycle_bin {
                if item.is_registry() && registry_backup_failed {
                    return true;
                }
                return failed_paths.contains(&item.path);
            }
            true
        });

        for item in &mut self.items {
            item.selected = false;
        }

        self.invalidate_cache();
        self.last_clean_result = Some(result);
        self.phase = AppPhase::Results;
        self.show_result_dialog = true;
    }

    fn render_category_overview(&mut self, ui: &mut egui::Ui) {
        ui.label("在左侧选择类别查看文件详情，或直接勾选类别后清理。");
        ui.add_space(4.0);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("category_grid")
                    .num_columns(5)
                    .spacing([12.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("选");
                        ui.strong("类别");
                        ui.strong("文件数");
                        ui.strong("大小");
                        ui.strong("");
                        ui.end_row();

                        for &category in JunkCategory::all() {
                            let summary = self
                                .category_summaries()
                                .get(&category)
                                .cloned()
                                .unwrap_or_default();

                            let cleanable_count = self
                                .items
                                .iter()
                                .filter(|i| i.category == category && i.can_clean(self.is_admin))
                                .count();

                            let mut selected = self.is_category_fully_selected(category);
                            let partial = self.is_category_partially_selected(category);

                            ui.horizontal(|ui| {
                                if cleanable_count == 0 {
                                    ui.add_enabled(false, egui::Checkbox::without_text(&mut false));
                                } else if partial {
                                    let mut tri = selected;
                                    if ui.checkbox(&mut tri, "").clicked() {
                                        self.set_category_selection(category, !selected);
                                    }
                                } else {
                                    if ui.checkbox(&mut selected, "").changed() {
                                        self.set_category_selection(category, selected);
                                    }
                                }
                            });

                            ui.label(category.label());

                            if summary.count > 0 {
                                ui.label(format!("{}", summary.count));
                                ui.label(format_size(summary.total_size));
                            } else if cleanable_count == 0
                                && self.items.iter().any(|i| i.category == category)
                            {
                                ui.label("—");
                                ui.label("—");
                            } else {
                                ui.label("0");
                                ui.label("0 B");
                            }

                            ui.horizontal(|ui| {
                                if summary.count > 0 || cleanable_count > 0 {
                                    if ui.small_button("查看").clicked() {
                                        self.filter_category = Some(category);
                                        self.cached_filter_key.clear();
                                    }
                                }
                            });

                            ui.end_row();
                        }
                    });
            });
    }

    fn render_file_list(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("路径过滤:");
            let changed = ui.text_edit_singleline(&mut self.search_filter).changed();
            if changed {
                self.cached_filter_key.clear();
            }
            if let Some(cat) = self.filter_category {
                if ui.button("← 返回类别总览").clicked() {
                    self.filter_category = None;
                    self.search_filter.clear();
                    self.cached_filter_key.clear();
                }
                ui.label(format!("当前: {}", cat.label()));
            }
        });

        if let Some(cat) = self.filter_category {
            if cat.is_browser() {
                ui.add_space(4.0);
                ui.label("二级分类（勾选可批量选择该子类）:");
                ui.horizontal_wrapped(|ui| {
                    for &sub in BrowserSubKind::all() {
                        let count = self.browser_sub_count(cat, sub);
                        if count == 0 {
                            continue;
                        }
                        let mut checked = self.is_browser_sub_fully_selected(cat, sub);
                        let label = format!("{} ({})", sub.label(), count);
                        if ui.checkbox(&mut checked, label).changed() {
                            self.set_browser_sub_selection(cat, sub, checked);
                        }
                    }
                });
                ui.colored_label(
                    egui::Color32::from_rgb(210, 153, 34),
                    "清理 Cookie / 本地存储将退出网站登录。",
                );
            }
            if cat == JunkCategory::RegistryOrphans {
                ui.colored_label(
                    egui::Color32::from_rgb(210, 153, 34),
                    "失效注册表：清理前将备份到 %TEMP%\\bgxiong-reg-backup\\",
                );
            }
        }

        ui.separator();

        let indices: Vec<usize> = self.filtered_indices().to_vec();
        let total = indices.len();

        if total == 0 {
            ui.label("没有匹配的项目。");
            return;
        }

        ui.label(format!("共 {} 项（虚拟滚动，仅渲染可见行）", total));

        ui.horizontal(|ui| {
            ui.strong("选");
            ui.add_space(28.0);
            ui.strong("路径");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.strong("状态");
                ui.add_space(60.0);
                ui.strong("大小");
            });
        });
        ui.separator();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, ROW_HEIGHT, total, |ui, row_range| {
                for row in row_range {
                    let idx = indices[row];
                    let item = &mut self.items[idx];
                    let can_select = item.can_clean(self.is_admin);
                    let sub_tag = item
                        .browser_sub
                        .map(|s| format!("[{}] ", s.short_label()))
                        .unwrap_or_default();
                    let app_tag = item
                        .app_name
                        .as_ref()
                        .map(|a| format!("[{a}] "))
                        .unwrap_or_default();
                    let path_str = item.path.to_string_lossy().to_string();
                    let hover = item
                        .skip_reason
                        .clone()
                        .unwrap_or_else(|| path_str.clone());
                    let display = format!("{sub_tag}{app_tag}{path_str}");
                    let status = item.status;
                    let status_text = match status {
                        ItemStatus::Normal => item.status.label().to_string(),
                        ItemStatus::NeedsAdmin => item
                            .skip_reason
                            .clone()
                            .unwrap_or_else(|| "需管理员".into()),
                        ItemStatus::Skipped => item
                            .skip_reason
                            .clone()
                            .unwrap_or_else(|| "跳过".into()),
                    };
                    let size = item.size;
                    let is_reg = item.is_registry();

                    ui.horizontal(|ui| {
                        ui.add_enabled_ui(can_select, |ui| {
                            ui.checkbox(&mut item.selected, "");
                        });

                        ui.label(&display).on_hover_text(&hover);

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let color = match status {
                                ItemStatus::Normal => egui::Color32::GRAY,
                                ItemStatus::NeedsAdmin => egui::Color32::from_rgb(210, 153, 34),
                                ItemStatus::Skipped => egui::Color32::from_rgb(248, 81, 73),
                            };
                            ui.colored_label(color, status_text);
                            ui.add_space(8.0);
                            if is_reg {
                                ui.label("—");
                            } else {
                                ui.label(format_size(size));
                            }
                        });
                    });
                }
            });
    }
}

impl eframe::App for ClearHelperApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.phase == AppPhase::Scanning {
            self.poll_scan();
            ctx.request_repaint();
        }
        if self.phase == AppPhase::Cleaning {
            self.poll_clean(ctx);
            ctx.request_repaint();
        }

        // 底栏固定：清理按钮始终可见
        egui::TopBottomPanel::bottom("bottom_bar")
            .min_height(48.0)
            .show(ctx, |ui| {
                let (selected_count, selected_size) = self.selected_stats();
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "已选 {} 项，合计 {}",
                        selected_count,
                        format_size(selected_size)
                    ));

                    if self.phase == AppPhase::Cleaning {
                        ui.spinner();
                        ui.label("正在清理，请稍候...");
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let can_clean =
                            selected_count > 0 && self.phase != AppPhase::Cleaning && self.phase != AppPhase::Scanning;

                        if ui
                            .add_enabled(can_clean, egui::Button::new("清理选中项"))
                            .clicked()
                        {
                            self.show_confirm_dialog = true;
                        }

                        let can_select =
                            self.phase == AppPhase::Results && !self.items.is_empty();
                        if ui
                            .add_enabled(can_select, egui::Button::new("全不选"))
                            .clicked()
                        {
                            self.deselect_all();
                        }

                        if ui
                            .add_enabled(can_select, egui::Button::new("全选可清理"))
                            .clicked()
                        {
                            self.select_all_cleanable();
                        }
                    });
                });
            });

        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("BGXiong C盘清理助手");
                ui.separator();
                let admin_label = if self.is_admin {
                    "管理员模式"
                } else {
                    "普通用户模式"
                };
                ui.label(
                    egui::RichText::new(admin_label).color(if self.is_admin {
                        egui::Color32::from_rgb(46, 160, 67)
                    } else {
                        egui::Color32::from_rgb(210, 153, 34)
                    }),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !self.is_admin {
                        if ui.button("以管理员重新启动").clicked() {
                            match restart_as_admin() {
                                Ok(()) => {}
                                Err(e) => self.elevate_error = Some(e),
                            }
                        }
                    }

                    let can_scan =
                        self.phase != AppPhase::Scanning && self.phase != AppPhase::Cleaning;
                    if ui
                        .add_enabled(can_scan, egui::Button::new("开始扫描"))
                        .clicked()
                    {
                        self.start_scan();
                    }

                    if self.phase == AppPhase::Scanning {
                        if ui.button("取消扫描").clicked() {
                            if let Some(cancel) = &self.scan_cancel {
                                cancel.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                });
            });

            if !self.scan_progress.is_empty() {
                ui.label(&self.scan_progress);
            }

            if let Some(err) = &self.elevate_error {
                ui.colored_label(egui::Color32::RED, err);
            }
        });

        egui::SidePanel::left("categories")
            .resizable(true)
            .default_width(240.0)
            .show(ctx, |ui| {
                ui.heading("类别");
                ui.separator();

                let summaries: HashMap<JunkCategory, CategorySummary> =
                    self.category_summaries().clone();

                let all_selected = self.filter_category.is_none();
                if ui.selectable_label(all_selected, "类别总览").clicked() {
                    self.filter_category = None;
                    self.cached_filter_key.clear();
                }

                ui.separator();

                for &category in JunkCategory::all() {
                    let summary = summaries.get(&category).cloned().unwrap_or_default();
                    let (sel_count, sel_size) = self.category_selected_count(category);
                    let label = if summary.count > 0 {
                        format!(
                            "{} ({} / {})",
                            category.label(),
                            summary.count,
                            format_size(summary.total_size)
                        )
                    } else {
                        category.label().to_string()
                    };

                    let selected = self.filter_category == Some(category);
                    ui.horizontal(|ui| {
                        let cleanable = self
                            .items
                            .iter()
                            .any(|i| i.category == category && i.can_clean(self.is_admin));
                        let mut cat_checked = self.is_category_fully_selected(category);
                        if cleanable {
                            if ui.checkbox(&mut cat_checked, "").changed() {
                                self.set_category_selection(category, cat_checked);
                            }
                        } else {
                            ui.add_enabled(false, egui::Checkbox::without_text(&mut false));
                        }

                        if ui.selectable_label(selected, &label).clicked() {
                            self.filter_category = Some(category);
                            self.cached_filter_key.clear();
                        }
                    });

                    if selected {
                        if sel_count > 0 {
                            ui.small(format!("  已选 {} / {}", sel_count, format_size(sel_size)));
                        }
                        if let Some(warning) = category.warning() {
                            ui.colored_label(
                                egui::Color32::from_rgb(210, 153, 34),
                                format!("  {warning}"),
                            );
                        }
                    }
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.phase == AppPhase::Scanning {
                ui.vertical_centered(|ui| {
                    ui.add_space(80.0);
                    ui.spinner();
                    ui.label("正在扫描，请稍候...");
                    ui.label(&self.scan_progress);
                });
                return;
            }

            if self.phase == AppPhase::Idle && self.items.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(80.0);
                    ui.heading("点击「开始扫描」查找 C 盘垃圾文件");
                    ui.label("扫描后勾选类别或文件，点击底部「清理选中项」删除。");
                });
                return;
            }

            if self.filter_category.is_none() {
                self.render_category_overview(ui);
            } else {
                self.render_file_list(ui);
            }
        });

        if self.show_confirm_dialog {
            egui::Window::new("确认清理")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    let (count, size) = self.selected_stats();
                    let has_reg = self.selected_has_registry();
                    ui.heading(format!("将删除 {} 个项目", count));
                    ui.label(format!("释放空间约: {}", format_size(size)));
                    ui.add_space(4.0);
                    ui.colored_label(
                        egui::Color32::from_rgb(248, 81, 73),
                        "此操作不可撤销！回收站内容将永久删除。",
                    );
                    if has_reg {
                        ui.colored_label(
                            egui::Color32::from_rgb(210, 153, 34),
                            "含注册表项：清理前将自动备份到 %TEMP%\\bgxiong-reg-backup\\",
                        );
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("确认删除").clicked() {
                            self.start_clean();
                        }
                        if ui.button("取消").clicked() {
                            self.show_confirm_dialog = false;
                        }
                    });
                });
        }

        if self.show_result_dialog {
            if let Some(result) = self.last_clean_result.clone() {
                egui::Window::new("清理结果")
                    .collapsible(false)
                    .resizable(true)
                    .default_width(500.0)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(format!("成功删除: {} 项", result.success_count));
                        ui.label(format!("失败: {} 项", result.fail_count));
                        ui.label(format!("释放空间: {}", format_size(result.freed_bytes)));

                        if !result.errors.is_empty() {
                            ui.separator();
                            ui.label("失败详情:");
                            egui::ScrollArea::vertical()
                                .max_height(200.0)
                                .show(ui, |ui| {
                                    for err in &result.errors {
                                        ui.label(format!(
                                            "{}: {}",
                                            err.path.to_string_lossy(),
                                            err.reason
                                        ));
                                    }
                                });
                        }

                        ui.separator();
                        if ui.button("关闭").clicked() {
                            self.show_result_dialog = false;
                        }
                    });
            }
        }
    }
}
