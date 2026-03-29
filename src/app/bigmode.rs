use std::sync::mpsc;
use std::thread::sleep;

use crate::app::{PartyConfig, load_cfg};
use crate::handler::Handler;
use crate::input::*;
use crate::instance::*;
use crate::launch::launch_common;
use crate::monitor::Monitor;
use crate::profiles::{auto_assign_profile, scan_profiles};

use eframe::egui::{self, StrokeKind};

const LAYOUT_ANIM_SECS: f32 = 0.25;

pub struct BigModeApp {
    pub monitors: Vec<Monitor>,
    pub handler: Handler,
    pub cfg: PartyConfig,
    pub input_devices: Vec<InputDevice>,
    pub instances: Vec<Instance>,
    pub instance_add_dev: Option<usize>,
    pub profiles: Vec<String>,
    pub should_exit: bool,
    pub loading_msg: Option<String>,
    pub loading_since: Option<std::time::Instant>,
    pub task: Option<std::thread::JoinHandle<()>>,
    rescan_rx: mpsc::Receiver<Vec<InputDevice>>,

    pub arrow_left_instance: Option<usize>,
    pub arrow_right_instance: Option<usize>,

    pub layout_rotation: u8,
}

impl BigModeApp {
    pub fn new(monitors: Vec<Monitor>, handler: Handler) -> Self {
        let cfg = load_cfg();
        let input_devices = scan_input_devices(&cfg.pad_filter_type);
        let profiles = scan_profiles(false);

        let (rescan_tx, rescan_rx) = mpsc::channel();
        let filter = cfg.pad_filter_type.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            if rescan_tx.send(scan_input_devices(&filter)).is_err() {
                break;
            }
        });

        let mut app = Self {
            monitors,
            handler,
            cfg,
            input_devices,
            instances: Vec::new(),
            instance_add_dev: None,
            profiles,
            rescan_rx,
            should_exit: false,
            loading_msg: None,
            loading_since: None,
            task: None,
            arrow_left_instance: None,
            arrow_right_instance: None,
            layout_rotation: 0,
        };

        let device_count = app.input_devices.len();
        app.auto_add_gamepads(0, device_count);

        app
    }

    fn spawn_task<F>(&mut self, msg: &str, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.loading_msg = Some(msg.to_string());
        self.loading_since = Some(std::time::Instant::now());
        self.task = Some(std::thread::spawn(f));
    }

    fn remove_device(&mut self, dev: usize) {
        let Some(idx) = self.instances.iter().rposition(|inst| inst.devices.contains(&dev)) else {
            return;
        };
        if remove_device_from_instance(&mut self.instances, idx, dev) {
            self.layout_rotation = 0;
            if let Some(pending_inst_idx) = self.instance_add_dev {
                if pending_inst_idx == idx {
                    self.instance_add_dev = None;
                } else if pending_inst_idx > idx {
                    self.instance_add_dev = Some(pending_inst_idx - 1);
                }
            }
        }
    }

    fn can_add_device_to_instance(&self, dev_index: usize) -> bool {
        let device = &self.input_devices[dev_index];

        if device.device_type() != DeviceType::Gamepad && !self.cfg.kbm_support {
            return false;
        }

        if is_device_in_any_instance(&self.instances, dev_index) {
            if device.device_type() != DeviceType::Gamepad {
                return false;
            }
            if !self.cfg.allow_multiple_instances_on_same_device {
                return false;
            }
        }

        true
    }

    fn create_instance_with_device(&mut self, dev_index: usize) {
        let profselection = auto_assign_profile(&self.instances, &mut self.profiles);
        self.instances.push(Instance {
            devices: vec![dev_index],
            profname: String::new(),
            profselection,
            monitor: 0,
            width: 0,
            height: 0,
        });
        self.layout_rotation = 0;
    }

    fn auto_add_gamepads(&mut self, start_index: usize, end_index: usize) {
        for i in start_index..end_index {
            if !self.input_devices[i].enabled() {
                continue;
            }
            if self.input_devices[i].device_type() != DeviceType::Gamepad {
                continue;
            }
            if is_device_in_any_instance(&self.instances, i)
                && !self.cfg.allow_multiple_instances_on_same_device
            {
                continue;
            }
            self.create_instance_with_device(i);
        }
    }

    fn sync_input_devices(&mut self, new_devices: Vec<InputDevice>) {
        use std::collections::HashMap;

        let old_device_count = self.input_devices.len();
        let mut old_idx_by_path: HashMap<String, usize> = std::mem::take(&mut self.input_devices)
            .into_iter()
            .enumerate()
            .map(|(idx, dev)| (dev.path().to_string(), idx))
            .collect();

        let mut new_idx_by_old: Vec<Option<usize>> = vec![None; old_device_count];
        let mut added_idxs: Vec<usize> = Vec::new();
        let mut next_devices: Vec<InputDevice> = Vec::with_capacity(new_devices.len());

        for (new_idx, fresh_dev) in new_devices.into_iter().enumerate() {
            let path = fresh_dev.path().to_string();

            if let Some(old_idx) = old_idx_by_path.remove(&path) {
                new_idx_by_old[old_idx] = Some(new_idx);
            } else {
                added_idxs.push(new_idx);
            }

            next_devices.push(fresh_dev);
        }

        self.input_devices = next_devices;

        for instance in &mut self.instances {
            let mut remapped_devices = Vec::with_capacity(instance.devices.len());
            for &old_idx in &instance.devices {
                if let Some(Some(new_idx)) = new_idx_by_old.get(old_idx) {
                    remapped_devices.push(*new_idx);
                }
            }
            remapped_devices.sort_unstable();
            remapped_devices.dedup();
            instance.devices = remapped_devices;
        }

        let mut inst_idx = 0;
        let mut removed_any = false;
        while inst_idx < self.instances.len() {
            if self.instances[inst_idx].devices.is_empty() {
                self.instances.remove(inst_idx);
                removed_any = true;
                if let Some(pending_inst_idx) = self.instance_add_dev {
                    if pending_inst_idx == inst_idx {
                        self.instance_add_dev = None;
                    } else if pending_inst_idx > inst_idx {
                        self.instance_add_dev = Some(pending_inst_idx - 1);
                    }
                }
            } else {
                inst_idx += 1;
            }
        }

        if removed_any {
            self.layout_rotation = 0;
        }

        if let Some(pending_inst_idx) = self.instance_add_dev
            && pending_inst_idx >= self.instances.len()
        {
            self.instance_add_dev = None;
        }

        for new_idx in added_idxs {
            if !self.input_devices[new_idx].enabled() {
                continue;
            }
            if self.input_devices[new_idx].device_type() != DeviceType::Gamepad {
                continue;
            }
            if !self.can_add_device_to_instance(new_idx) {
                continue;
            }
            self.create_instance_with_device(new_idx);
        }
    }

    fn cycle_profile(&mut self, instance_index: usize, direction: i32) {
        if self.profiles.is_empty() {
            return;
        }
        let instance = &mut self.instances[instance_index];
        let len = self.profiles.len();
        if direction > 0 {
            instance.profselection = (instance.profselection + 1) % len;
        } else {
            instance.profselection = (instance.profselection + len - 1) % len;
        }
    }

    fn prepare_game_launch(&mut self) {
        if self.task.is_some() {
            return;
        }

        let dev_infos: Vec<DeviceInfo> = self.input_devices.iter().map(|p| p.info()).collect();

        let mut instances = crate::instance::build_launch_instances(
            &self.instances,
            &self.profiles,
            &self.monitors,
            &self.cfg,
            self.layout_rotation,
        );

        let cfg = self.cfg.clone();
        let monitors = self.monitors.clone();
        let layout_rotation = self.layout_rotation;
        let handler = self.handler.clone();

        self.spawn_task("Launching...\n\nDon't press any buttons or move any analog sticks or mice.", move || {
                sleep(std::time::Duration::from_millis(1500));
                launch_common(&handler, &dev_infos, &mut instances, &monitors, &cfg, layout_rotation);
            },
        );
    }

    fn handle_input(&mut self, raw_input: &mut egui::RawInput) {
        if !raw_input.focused || self.task.is_some() {
            return;
        }

        self.arrow_left_instance = None;
        self.arrow_right_instance = None;

        let device_count = self.input_devices.len();
        for i in 0..device_count {
            if !self.input_devices[i].enabled() {
                continue;
            }
            let btn = self.input_devices[i].poll();
            match btn {
                Some(PadButton::ABtn) | Some(PadButton::ZKey) | Some(PadButton::RightClick) => {
                    if !self.can_add_device_to_instance(i) {
                        continue;
                    }
                    match self.instance_add_dev {
                        Some(inst) => {
                            if !self.instances[inst].devices.contains(&i) {
                                self.instances[inst].devices.push(i);
                                self.instance_add_dev = None;
                            }
                        }
                        None => {
                            self.create_instance_with_device(i);
                        }
                    }
                }
                Some(PadButton::Left) => {
                    if let Some(inst_idx) = find_device_instance(&self.instances, i) {
                        self.arrow_left_instance = Some(inst_idx);
                        self.cycle_profile(inst_idx, -1);
                    }
                }
                Some(PadButton::Right) => {
                    if let Some(inst_idx) = find_device_instance(&self.instances, i) {
                        self.arrow_right_instance = Some(inst_idx);
                        self.cycle_profile(inst_idx, 1);
                    }
                }
                Some(PadButton::StartBtn) => {
                    if !self.instances.is_empty() && is_device_in_any_instance(&self.instances, i) {
                        self.prepare_game_launch();
                        return;
                    }
                }
                Some(PadButton::YBtn) => {
                    if let Some(inst_idx) = find_device_instance(&self.instances, i) {
                        if self.instance_add_dev == Some(inst_idx) {
                            self.instance_add_dev = None;
                        } else {
                            self.instance_add_dev = Some(inst_idx);
                        }
                    }
                }
                Some(PadButton::XBtn) => {
                    if self.instances.len() >= 2 && is_device_in_any_instance(&self.instances, i) {
                        self.layout_rotation = self.layout_rotation.wrapping_add(1);
                    }
                }
                Some(PadButton::BackspaceKey) => {
                    if self.input_devices[i].device_type() == DeviceType::Keyboard
                        && is_device_in_any_instance(&self.instances, i)
                    {
                        self.remove_device(i);
                    }
                }
                Some(PadButton::MiddleClick) => {
                    if self.input_devices[i].device_type() == DeviceType::Mouse
                        && is_device_in_any_instance(&self.instances, i)
                    {
                        self.remove_device(i);
                    }
                }
                Some(PadButton::BBtn) => {
                    if self.instance_add_dev.is_some() {
                        self.instance_add_dev = None;
                    } else if is_device_in_any_instance(&self.instances, i) {
                        self.remove_device(i);
                    } else if self.instances.is_empty() {
                        self.should_exit = true;
                    }
                }
                _ => {}
            }
        }
    }

    fn render(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::BLACK))
            .show(ctx, |ui| {
                if self.task.is_some() {
                    ui.disable();
                }

                let panel_rect = ui.available_rect_before_wrap();
                let margin = 150.0;
                let outer_rect = panel_rect.shrink(margin);

                ui.painter().rect_stroke(
                    outer_rect,
                    12.0,
                    egui::Stroke::new(4.0, egui::Color32::WHITE),
                    StrokeKind::Inside,
                );

                self.display_handler_logo(ui, panel_rect, outer_rect);

                if self.instances.is_empty() {
                    self.display_instruction_box(ui, outer_rect);
                } else {
                    let layout = build_layout(
                        self.instances.len(),
                        self.cfg.vertical_two_player,
                        self.layout_rotation,
                    );
                    let target_regions = layout_regions_egui(&layout, outer_rect);
                    let regions: Vec<egui::Rect> = target_regions
                        .iter()
                        .enumerate()
                        .map(|(i, &r)| {
                            let instance_index = layout.region_to_instance.get(i).copied().unwrap_or(i);
                            animate_rect(ctx, &format!("region_{}", instance_index), r)
                        })
                        .collect();

                    display_divider_lines_animated(ctx, ui, &layout, outer_rect);

                    for (region_idx, &region) in regions.iter().enumerate() {
                        if let Some(&inst_idx) = layout.region_to_instance.get(region_idx) {
                            self.display_player_box(ui, inst_idx, region);
                        }
                    }
                }

                display_button_guide(ui, panel_rect, outer_rect);
            });

        if let Some(msg) = &self.loading_msg {
            egui::Area::new("loading".into())
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .interactable(false)
                .show(ctx, |ui| {
                    egui::Frame::NONE
                        .fill(egui::Color32::from_rgba_premultiplied(0, 0, 0, 192))
                        .corner_radius(6.0)
                        .inner_margin(egui::Margin::symmetric(16, 12))
                        .show(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.add(egui::widgets::Spinner::new().size(40.0));
                                ui.add_space(8.0);
                                ui.label(msg);
                            });
                        });
                });
        }
    }

    fn display_handler_logo(
        &self,
        ui: &mut egui::Ui,
        panel_rect: egui::Rect,
        outer_rect: egui::Rect,
    ) {
        let logo_path = self.handler.img_paths.iter().find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| {
                    n.eq_ignore_ascii_case("logo.png") || n.eq_ignore_ascii_case("logo.jpg")
                })
        });

        let Some(logo_path) = logo_path else {
            return;
        };

        let gap_height = outer_rect.min.y - panel_rect.min.y;
        let max_height = (gap_height - 60.0).max(24.0);
        let center = egui::pos2(panel_rect.center().x, (panel_rect.min.y + outer_rect.min.y) / 2.0);
        let logo_rect = egui::Rect::from_center_size(center, egui::Vec2::new(max_height * 4.0, max_height));

        let mut child_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(logo_rect)
                .layout(egui::Layout::centered_and_justified(
                    egui::Direction::TopDown,
                )),
        );

        child_ui.add(
            egui::Image::new(format!("file://{}", logo_path.display()))
                .max_height(max_height)
                .maintain_aspect_ratio(true),
        );
    }

    fn display_instruction_box(&self, ui: &mut egui::Ui, area: egui::Rect) {
        let emoji_width = if self.cfg.kbm_support {
            56.0 * 3.0 + 24.0 * 2.0
        } else {
            56.0
        };
        let box_width = emoji_width + 24.0 * 2.0 + 300.0;
        let box_height = 130.0;

        let box_rect = egui::Rect::from_center_size(area.center(), egui::Vec2::new(box_width, box_height));

        let mut child_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(box_rect)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );

        egui::Frame::NONE
            .fill(egui::Color32::from_rgb(10, 10, 15))
            .stroke(egui::Stroke::new(2.0, egui::Color32::WHITE))
            .corner_radius(16.0)
            .inner_margin(egui::Margin::symmetric(24, 16))
            .show(&mut child_ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("PRESS INPUT TO ADD PLAYER")
                            .size(26.0)
                            .strong()
                            .color(egui::Color32::WHITE),
                    );
                    ui.add_space(8.0);

                    ui.allocate_ui_with_layout(
                        egui::Vec2::new(emoji_width, 56.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.label(egui::RichText::new("\u{1f3ae}").size(56.0));
                            if self.cfg.kbm_support {
                                ui.add_space(24.0);
                                ui.label(egui::RichText::new("\u{1f5ae}").size(56.0));
                                ui.add_space(24.0);
                                ui.label(egui::RichText::new("\u{1f5b1}").size(56.0));
                            }
                        },
                    );
                });
            });
    }

    fn display_player_box(&mut self, ui: &mut egui::Ui, inst_idx: usize, region: egui::Rect) {
        let (profselection, devices) = {
            let instance = &self.instances[inst_idx];
            (instance.profselection, instance.devices.clone())
        };
        let is_waiting = self.instance_add_dev == Some(inst_idx);

        let profile_name = if profselection < self.profiles.len() {
            self.profiles[profselection].clone()
        } else {
            "???".to_string()
        };

        let card_width = (region.width() * 0.6).min(200.0);
        let card_height = (region.height() * 0.7).min(234.0);
        let card_rect = egui::Rect::from_center_size(
            region.center(),
            egui::Vec2::new(card_width, card_height),
        );

        let mut child_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(card_rect)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );

        egui::Frame::NONE
            .fill(egui::Color32::from_rgb(15, 15, 20))
            .stroke(egui::Stroke::new(1.0, egui::Color32::WHITE))
            .corner_radius(16.0)
            .inner_margin(egui::Margin::symmetric(8, 8))
            .show(&mut child_ui, |ui| {
                ui.set_max_height(card_height - 16.0);

                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new(&profile_name)
                            .size(22.0)
                            .strong()
                            .color(egui::Color32::WHITE),
                    );

                    ui.separator();

                    let dev_icon_w = 60.0_f32;
                    let dev_spacing = 8.0_f32;
                    for row_start in (0..devices.len()).step_by(2) {
                        let row_count = (devices.len() - row_start).min(2);
                        let row_width = row_count as f32 * dev_icon_w
                            + (row_count as f32 - 1.0).max(0.0) * dev_spacing;
                        ui.allocate_ui_with_layout(
                            egui::Vec2::new(row_width, 70.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.spacing_mut().item_spacing.x = dev_spacing;
                                for d_idx in row_start..(row_start + 2).min(devices.len()) {
                                    let dev_idx = devices[d_idx];
                                    if dev_idx < self.input_devices.len() {
                                        let emoji =
                                            self.input_devices[dev_idx].emoji().to_string();
                                        let short_name = self.input_devices[dev_idx]
                                            .fancyname()
                                            .split_whitespace()
                                            .next()
                                            .unwrap_or("")
                                            .to_string();
                                        ui.allocate_ui_with_layout(
                                            egui::Vec2::new(dev_icon_w, 70.0),
                                            egui::Layout::top_down(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new(&emoji).size(41.0),
                                                );
                                                ui.label(
                                                    egui::RichText::new(&short_name)
                                                        .size(12.0)
                                                        .color(egui::Color32::LIGHT_GRAY),
                                                );
                                            },
                                        );
                                    }
                                }
                            },
                        );
                    }

                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                        let btn_size = egui::Vec2::new(32.0, 32.0);
                        let center_btn_size = egui::Vec2::new(28.0, 28.0);
                        let spacing = ui.spacing().item_spacing.x;
                        let total_btns_width =
                            btn_size.x + center_btn_size.x + btn_size.x + spacing * 2.0;

                        ui.allocate_ui_with_layout(
                            egui::Vec2::new(total_btns_width, 32.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                if arrow_btn(
                                    ui,
                                    "\u{25c0}",
                                    self.arrow_left_instance == Some(inst_idx),
                                    btn_size,
                                )
                                .clicked()
                                {
                                    self.cycle_profile(inst_idx, -1);
                                }

                                let (text, fill, text_color) = if is_waiting {
                                    ("\u{23f3}", egui::Color32::WHITE, egui::Color32::BLACK)
                                } else {
                                    (
                                        "+",
                                        egui::Color32::from_rgb(30, 30, 35),
                                        egui::Color32::WHITE,
                                    )
                                };
                                let center_btn = egui::Button::new(
                                    egui::RichText::new(text).color(text_color).size(14.0),
                                )
                                .fill(fill)
                                .stroke(egui::Stroke::new(1.0, egui::Color32::WHITE))
                                .corner_radius(4.0)
                                .min_size(center_btn_size);
                                if ui.add(center_btn).clicked() {
                                    if is_waiting {
                                        self.instance_add_dev = None;
                                    } else {
                                        self.instance_add_dev = Some(inst_idx);
                                    }
                                }

                                if arrow_btn(
                                    ui,
                                    "\u{25b6}",
                                    self.arrow_right_instance == Some(inst_idx),
                                    btn_size,
                                )
                                .clicked()
                                {
                                    self.cycle_profile(inst_idx, 1);
                                }
                            },
                        );
                    });
                });
            });
    }
}

impl eframe::App for BigModeApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.handle_input(raw_input);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.should_exit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        while let Ok(fresh) = self.rescan_rx.try_recv() {
            self.sync_input_devices(fresh);
        }

        if let Some(handle) = self.task.take() {
            if handle.is_finished() {
                let _ = handle.join();
                self.loading_since = None;
                self.loading_msg = None;
                self.should_exit = true;
            } else {
                self.task = Some(handle);
            }
        }
        if let Some(start) = self.loading_since
            && start.elapsed() > std::time::Duration::from_secs(60)
        {
            self.loading_msg = Some("Operation timed out".to_string());
        }

        self.render(ctx);

        if ctx.input(|input| input.focused) {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }
}

fn arrow_btn(ui: &mut egui::Ui, glyph: &str, active: bool, size: egui::Vec2) -> egui::Response {
    let (fill, text_color) = if active {
        (egui::Color32::WHITE, egui::Color32::BLACK)
    } else {
        (egui::Color32::from_rgb(30, 30, 35), egui::Color32::WHITE)
    };
    ui.add(
        egui::Button::new(egui::RichText::new(glyph).color(text_color).size(14.0))
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, egui::Color32::WHITE))
            .corner_radius(4.0)
            .min_size(size),
    )
}

fn animate_rect(ctx: &egui::Context, id_salt: &str, target: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(
            ctx.animate_value_with_time(
                egui::Id::new(id_salt).with("min_x"),
                target.min.x,
                LAYOUT_ANIM_SECS,
            ),
            ctx.animate_value_with_time(
                egui::Id::new(id_salt).with("min_y"),
                target.min.y,
                LAYOUT_ANIM_SECS,
            ),
        ),
        egui::pos2(
            ctx.animate_value_with_time(
                egui::Id::new(id_salt).with("max_x"),
                target.max.x,
                LAYOUT_ANIM_SECS,
            ),
            ctx.animate_value_with_time(
                egui::Id::new(id_salt).with("max_y"),
                target.max.y,
                LAYOUT_ANIM_SECS,
            ),
        ),
    )
}

fn animate_pos(ctx: &egui::Context, id_salt: &str, target: egui::Pos2) -> egui::Pos2 {
    egui::pos2(
        ctx.animate_value_with_time(
            egui::Id::new(id_salt).with("x"),
            target.x,
            LAYOUT_ANIM_SECS,
        ),
        ctx.animate_value_with_time(
            egui::Id::new(id_salt).with("y"),
            target.y,
            LAYOUT_ANIM_SECS,
        ),
    )
}

fn norm_pos_to_egui(outer: egui::Rect, x: f32, y: f32) -> egui::Pos2 {
    egui::pos2(
        outer.min.x + outer.width() * x,
        outer.min.y + outer.height() * y,
    )
}

fn layout_regions_egui(layout: &Layout, outer: egui::Rect) -> Vec<egui::Rect> {
    layout
        .regions
        .iter()
        .map(|region| {
            let min = norm_pos_to_egui(outer, region.x, region.y);
            let max = norm_pos_to_egui(outer, region.x + region.w, region.y + region.h);
            egui::Rect::from_min_max(min, max)
        })
        .collect()
}

fn display_divider_lines_animated(
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    layout: &Layout,
    outer: egui::Rect,
) {
    let stroke = egui::Stroke::new(2.0, egui::Color32::WHITE);
    let painter = ui.painter();
    for (line_index, divider) in layout.dividers.iter().enumerate() {
        let target_start = norm_pos_to_egui(outer, divider.x1, divider.y1);
        let target_end = norm_pos_to_egui(outer, divider.x2, divider.y2);

        let start = if divider.x1 == 0.0 || divider.x1 == 1.0 || divider.y1 == 0.0 || divider.y1 == 1.0 {
            target_start
        } else {
            animate_pos(ctx, &format!("div_{}_start", line_index), target_start)
        };
        let end = if divider.x2 == 0.0 || divider.x2 == 1.0 || divider.y2 == 0.0 || divider.y2 == 1.0 {
            target_end
        } else {
            animate_pos(ctx, &format!("div_{}_end", line_index), target_end)
        };

        if (start.x - end.x).abs() > 0.5 || (start.y - end.y).abs() > 0.5 {
            painter.line_segment([start, end], stroke);
        }
    }
}

fn display_button_guide(ui: &mut egui::Ui, panel_rect: egui::Rect, outer_rect: egui::Rect) {
    let guide_entries: &[(&str, egui::ImageSource<'_>)] = &[
        ("Remove Device", egui::include_image!("../../res/BTN_B.png")),
        ("Add Device", egui::include_image!("../../res/BTN_Y.png")),
        ("Start", egui::include_image!("../../res/BTN_MENU.png")),
        //("Options Menu", egui::include_image!("../../res/BTN_VIEW.png")),
        ("Change Profile", egui::include_image!("../../res/BTN_DPAD.png")),
        ("Change Layout", egui::include_image!("../../res/BTN_X.png")),
    ];

    let icon_size = egui::vec2(24.0, 24.0);
    let gap_between_entries = 24.0;
    let inner_gap = 4.0;
    let center_y = (outer_rect.max.y + panel_rect.max.y) / 2.0;
    let guide_height = 32.0;
    let font = egui::FontId::proportional(14.0);
    let total_entries = guide_entries.len();

    let item_spacing = ui.spacing().item_spacing.x;
    let mut total_width: f32 = 0.0;
    for (i, (label, _)) in guide_entries.iter().enumerate() {
        let text_width = ui
            .painter()
            .layout_no_wrap(label.to_string(), font.clone(), egui::Color32::WHITE)
            .size()
            .x;
        total_width += icon_size.x + inner_gap + text_width + item_spacing * 2.0;
        if i < total_entries - 1 {
            total_width += gap_between_entries;
        }
    }

    let guide_rect = egui::Rect::from_center_size(
        egui::pos2(panel_rect.center().x, center_y),
        egui::Vec2::new(total_width, guide_height),
    );

    let mut child_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(guide_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );

    for (i, (label, icon_source)) in guide_entries.iter().enumerate() {
        child_ui.add(egui::Image::new(icon_source.clone()).fit_to_exact_size(icon_size));
        child_ui.add_space(inner_gap);
        child_ui.label(
            egui::RichText::new(*label)
                .size(14.0)
                .color(egui::Color32::LIGHT_GRAY),
        );
        if i < total_entries - 1 {
            child_ui.add_space(gap_between_entries);
        }
    }
}
