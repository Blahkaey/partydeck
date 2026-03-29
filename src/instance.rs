use crate::app::PartyConfig;
use crate::monitor::Monitor;
use crate::profiles::GUEST_NAMES;

#[derive(Clone)]
pub struct Instance {
    pub devices: Vec<usize>,
    pub profname: String,
    pub profselection: usize,
    pub monitor: usize,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectNorm {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineNorm {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub regions: Vec<RectNorm>,
    pub region_to_instance: Vec<usize>,
    pub dividers: Vec<LineNorm>,
}

const fn rect(x: f32, y: f32, w: f32, h: f32) -> RectNorm {
    RectNorm { x, y, w, h }
}

fn line(x1: f32, y1: f32, x2: f32, y2: f32) -> LineNorm {
    LineNorm { x1, y1, x2, y2 }
}

const FULL: RectNorm = rect(0.0, 0.0, 1.0, 1.0);
const TOP: RectNorm = rect(0.0, 0.0, 1.0, 0.5);
const BOTTOM: RectNorm = rect(0.0, 0.5, 1.0, 0.5);
const LEFT: RectNorm = rect(0.0, 0.0, 0.5, 1.0);
const RIGHT: RectNorm = rect(0.5, 0.0, 0.5, 1.0);
const TOP_LEFT: RectNorm = rect(0.0, 0.0, 0.5, 0.5);
const TOP_RIGHT: RectNorm = rect(0.5, 0.0, 0.5, 0.5);
const BOTTOM_LEFT: RectNorm = rect(0.0, 0.5, 0.5, 0.5);
const BOTTOM_RIGHT: RectNorm = rect(0.5, 0.5, 0.5, 0.5);

pub fn build_launch_instances(
    instances: &[Instance],
    profiles: &[String],
    monitors: &[Monitor],
    cfg: &PartyConfig,
    layout_rotation: u8,
) -> Vec<Instance> {
    let layout = build_layout(instances.len(), cfg.vertical_two_player, layout_rotation);
    let mut reordered = reorder_instances(instances, &layout);

    for instance in &mut reordered {
        instance.profname = profiles[instance.profselection].clone();
    }

    set_instance_resolutions(&mut reordered, monitors, cfg, layout_rotation);

    reordered
}

pub fn set_instance_names(instances: &mut Vec<Instance>, profiles: &[String]) {
    let mut guests = GUEST_NAMES.to_vec();

    for instance in instances {
        if instance.profselection == 0 {
            let i = fastrand::usize(..guests.len());
            instance.profname = format!(".{}", guests[i]);
            guests.swap_remove(i);
        } else {
            instance.profname = profiles[instance.profselection].to_owned();
        }
    }
}

pub fn reorder_instances(instances: &[Instance], layout: &Layout) -> Vec<Instance> {
    layout
        .region_to_instance
        .iter()
        .filter_map(|&i| instances.get(i).cloned())
        .collect()
}

pub fn set_instance_resolutions(
    instances: &mut [Instance],
    monitors: &[Monitor],
    cfg: &PartyConfig,
    layout_rotation: u8,
) {
    let Some(primary_monitor) = monitors.first() else {
        return;
    };

    let regions = instance_layout_regions(instances, cfg.vertical_two_player, layout_rotation);

    for (instance, region) in instances.iter_mut().zip(regions) {
        let monitor = monitors.get(instance.monitor).unwrap_or(primary_monitor);
        apply_region_resolution(instance, monitor, cfg, region);
    }
}

pub fn instance_layout_regions(
    instances: &[Instance],
    vertical_two_player: bool,
    layout_rotation: u8,
) -> Vec<RectNorm> {
    let max_monitor = instances.iter().map(|i| i.monitor).max().unwrap_or(0);
    let mut monitor_playercounts = vec![0usize; max_monitor + 1];
    for instance in instances {
        monitor_playercounts[instance.monitor] += 1;
    }

    let mut monitor_next = vec![0usize; monitor_playercounts.len()];
    instances
        .iter()
        .map(|instance| {
            let count = monitor_playercounts[instance.monitor];
            let idx = monitor_next[instance.monitor];
            monitor_next[instance.monitor] += 1;
            build_layout(count, vertical_two_player, layout_rotation)
                .regions
                .get(idx)
                .copied()
                .unwrap_or(FULL)
        })
        .collect()
}

fn apply_region_resolution(instance: &mut Instance, monitor: &Monitor, cfg: &PartyConfig, region: RectNorm) {
    let mut w = (monitor.width() as f32 * region.w).round() as u32;
    let mut h = (monitor.height() as f32 * region.h).round() as u32;

    if w == 0 {
        w = 1;
    }
    if h == 0 {
        h = 1;
    }

    if h < 600 && cfg.gamescope_fix_lowres {
        let ratio = w as f32 / h as f32;
        h = 600;
        w = (h as f32 * ratio) as u32;
    }

    instance.width = w;
    instance.height = h;
}

pub fn build_layout(
    player_count: usize,
    vertical_two_player: bool,
    layout_rotation: u8,
) -> Layout {
    let rotation = layout_rotation % 4;
    let base: u8 = if vertical_two_player { 1 } else { 0 };

    let regions = match player_count {
        0 => vec![],
        1 => vec![FULL],
        2 => {
            if vertical_two_player ^ (rotation % 2 == 1) {
                vec![LEFT, RIGHT]
            } else {
                vec![TOP, BOTTOM]
            }
        }
        3 => match (base + rotation) % 4 {
            0 => vec![TOP, BOTTOM_LEFT, BOTTOM_RIGHT],
            1 => vec![RIGHT, TOP_LEFT, BOTTOM_LEFT],
            2 => vec![BOTTOM, TOP_LEFT, TOP_RIGHT],
            3 => vec![LEFT, TOP_RIGHT, BOTTOM_RIGHT],
            _ => unreachable!(),
        },
        4 => vec![TOP_LEFT, TOP_RIGHT, BOTTOM_LEFT, BOTTOM_RIGHT],
        _ => grid_regions(player_count),
    };

    let edge_mid = [(0.0, 0.5), (0.5, 0.0), (1.0, 0.5), (0.5, 1.0)];
    let dividers = match player_count {
        0 | 1 => vec![],
        2 => {
            let a = (base + rotation) % 4;
            let (x1, y1) = edge_mid[a as usize];
            let (x2, y2) = edge_mid[((a + 2) % 4) as usize];
            vec![line(x1, y1, x2, y2)]
        }
        3 => {
            let big = (base + rotation) % 4;
            let (ax1, ay1) = edge_mid[big as usize];
            let (ax2, ay2) = edge_mid[((big + 2) % 4) as usize];
            let (bx, by) = edge_mid[((big + 3) % 4) as usize];
            vec![line(ax1, ay1, ax2, ay2), line(0.5, 0.5, bx, by)]
        }
        4 => vec![line(0.0, 0.5, 1.0, 0.5), line(0.5, 0.0, 0.5, 1.0)],
        _ => grid_dividers(player_count),
    };

    Layout {
        regions,
        region_to_instance: region_to_instance_map(player_count, vertical_two_player, layout_rotation),
        dividers,
    }
}

fn region_to_instance_map(player_count: usize, vertical_two_player: bool, layout_rotation: u8) -> Vec<usize> {
    let rotation = layout_rotation % 4;
    match player_count {
        0 | 1 => (0..player_count).collect(),
        2 => {
            let swap = ((rotation + if vertical_two_player { 0 } else { 1 }) % 4) >= 2;
            if swap {
                vec![1, 0]
            } else {
                vec![0, 1]
            }
        }
        3 => {
            let swap_smalls = if vertical_two_player {
                rotation == 1 || rotation == 2
            } else {
                rotation == 2 || rotation == 3
            };
            if swap_smalls {
                vec![0, 2, 1]
            } else {
                vec![0, 1, 2]
            }
        }
        4 => match rotation % 4 {
            0 => vec![0, 1, 2, 3],
            1 => vec![2, 0, 3, 1],
            2 => vec![3, 2, 1, 0],
            3 => vec![1, 3, 0, 2],
            _ => unreachable!(),
        },
        _ => {
            let (cols, rows) = grid_dimensions(player_count);
            let snake = snake_order(cols, rows, player_count);
            let shift = layout_rotation as usize;

            let mut snake_pos = vec![0; player_count];
            for (i, &region) in snake.iter().enumerate() {
                snake_pos[region] = i;
            }

            let mut map = vec![0; player_count];
            for inst in 0..player_count {
                let new_region = snake[(snake_pos[inst] + shift) % player_count];
                map[new_region] = inst;
            }
            map
        }
    }
}

fn grid_regions(player_count: usize) -> Vec<RectNorm> {
    let (cols, rows) = grid_dimensions(player_count);
    let cell_w = 1.0 / cols as f32;
    let cell_h = 1.0 / rows as f32;

    (0..player_count)
        .map(|index| {
            let row = index / cols;
            let col = index % cols;
            rect(col as f32 * cell_w, row as f32 * cell_h, cell_w, cell_h)
        })
        .collect()
}

fn grid_dividers(player_count: usize) -> Vec<LineNorm> {
    let (cols, rows) = grid_dimensions(player_count);
    let mut dividers = Vec::new();

    for c in 1..cols {
        let x = c as f32 / cols as f32;
        dividers.push(line(x, 0.0, x, 1.0));
    }
    for r in 1..rows {
        let y = r as f32 / rows as f32;
        dividers.push(line(0.0, y, 1.0, y));
    }

    dividers
}

fn grid_dimensions(player_count: usize) -> (usize, usize) {
    let cols = (player_count as f32).sqrt().ceil() as usize;
    let rows = (player_count + cols - 1) / cols;
    (cols.max(1), rows.max(1))
}

fn snake_order(cols: usize, rows: usize, count: usize) -> Vec<usize> {
    let mut order = Vec::with_capacity(count);
    for r in 0..rows {
        for i in 0..cols {
            let c = if r % 2 == 0 { i } else { cols - 1 - i };
            let idx = r * cols + c;
            if idx < count {
                order.push(idx);
            }
        }
    }
    order
}

pub fn is_device_in_any_instance(instances: &[Instance], dev: usize) -> bool {
    instances.iter().any(|instance| instance.devices.contains(&dev))
}

pub fn instance_has_device(instances: &[Instance], instance_index: usize, dev: usize) -> bool {
    instances
        .get(instance_index)
        .is_some_and(|instance| instance.devices.contains(&dev))
}

pub fn find_device_instance(instances: &[Instance], dev: usize) -> Option<usize> {
    instances.iter().position(|instance| instance.devices.contains(&dev))
}

pub fn remove_device_from_instance(instances: &mut Vec<Instance>, instance_index: usize, dev: usize) -> bool {
    let Some(instance) = instances.get_mut(instance_index) else {
        return false;
    };

    let Some(pos) = instance.devices.iter().position(|&d| d == dev) else {
        return false;
    };

    instance.devices.remove(pos);
    if instance.devices.is_empty() {
        instances.remove(instance_index);
        return true;
    }

    false
}
