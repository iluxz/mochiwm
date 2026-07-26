use windows::Win32::Foundation::HWND;

#[derive(Debug, Clone)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Layout {
    MasterStack,
    Grid,
    Horizontal,
    Vertical,
}

impl Default for Layout {
    fn default() -> Self { Self::MasterStack }
}

fn clamp(val: i32, min: i32) -> i32 {
    val.max(min)
}

pub fn compute_master_stack(windows: &[HWND], monitor: &Rect, gaps: i32, inner_gap: i32) -> Vec<Rect> {
    if windows.is_empty() {
        return vec![];
    }

    let count = windows.len();
    let m = monitor;

    if count == 1 {
        return vec![Rect {
            x: m.x + gaps,
            y: m.y + gaps,
            w: clamp(m.w - gaps * 2, 1),
            h: clamp(m.h - gaps * 2, 1),
        }];
    }

    let master_w = (m.w as f32 * 0.55) as i32;
    let stack_w = m.w - master_w - inner_gap;
    let stack_count = count - 1;
    let stack_h = (m.h - gaps * 2) / stack_count as i32;

    let mut result = vec![];

    result.push(Rect {
        x: m.x + gaps,
        y: m.y + gaps,
        w: clamp(master_w - gaps - inner_gap / 2, 1),
        h: clamp(m.h - gaps * 2, 1),
    });

    for i in 0..stack_count {
        result.push(Rect {
            x: m.x + master_w + inner_gap / 2,
            y: m.y + gaps + i as i32 * stack_h + if i > 0 { inner_gap / 2 } else { 0 },
            w: clamp(stack_w - gaps - inner_gap / 2, 1),
            h: clamp(stack_h - inner_gap, 1),
        });
    }

    result
}

pub fn compute_grid(windows: &[HWND], monitor: &Rect, gaps: i32, inner_gap: i32) -> Vec<Rect> {
    if windows.is_empty() {
        return vec![];
    }

    let count = windows.len();
    let cols = (count as f32).sqrt().ceil() as i32;
    let rows = ((count as f32) / cols as f32).ceil() as i32;
    let cell_w = clamp((monitor.w - gaps * 2 - inner_gap * (cols - 1)) / cols, 1);
    let cell_h = clamp((monitor.h - gaps * 2 - inner_gap * (rows - 1)) / rows, 1);

    windows.iter().enumerate().map(|(i, _)| {
        let col = i as i32 % cols;
        let row = i as i32 / cols;
        Rect {
            x: monitor.x + gaps + col * (cell_w + inner_gap),
            y: monitor.y + gaps + row * (cell_h + inner_gap),
            w: cell_w,
            h: cell_h,
        }
    }).collect()
}

pub fn compute_horizontal(windows: &[HWND], monitor: &Rect, gaps: i32, inner_gap: i32) -> Vec<Rect> {
    if windows.is_empty() {
        return vec![];
    }

    let count = windows.len();
    let strip_h = clamp((monitor.h - gaps * 2 - inner_gap * (count as i32 - 1)) / count as i32, 1);

    windows.iter().enumerate().map(|(i, _)| {
        Rect {
            x: monitor.x + gaps,
            y: monitor.y + gaps + i as i32 * (strip_h + inner_gap),
            w: clamp(monitor.w - gaps * 2, 1),
            h: strip_h,
        }
    }).collect()
}

pub fn compute_vertical(windows: &[HWND], monitor: &Rect, gaps: i32, inner_gap: i32) -> Vec<Rect> {
    if windows.is_empty() {
        return vec![];
    }

    let count = windows.len();
    let col_w = clamp((monitor.w - gaps * 2 - inner_gap * (count as i32 - 1)) / count as i32, 1);

    windows.iter().enumerate().map(|(i, _)| {
        Rect {
            x: monitor.x + gaps + i as i32 * (col_w + inner_gap),
            y: monitor.y + gaps,
            w: col_w,
            h: clamp(monitor.h - gaps * 2, 1),
        }
    }).collect()
}
