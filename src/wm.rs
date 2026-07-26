use crate::config::Config;
use crate::layout::{self, Layout, Rect};
use std::collections::HashMap;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub struct WindowManager {
    config: Config,
    workspaces: HashMap<u32, Vec<HWND>>,
    current_workspace: u32,
    layout: Layout,
    tiling_enabled: bool,
    focused: Option<HWND>,
}

impl WindowManager {
    pub fn new(config: Config) -> Self {
        let count = config.workspace.count;
        let mut workspaces = HashMap::new();
        for i in 1..=count {
            workspaces.insert(i, vec![]);
        }
        Self {
            config,
            workspaces,
            current_workspace: 1,
            layout: Layout::MasterStack,
            tiling_enabled: true,
            focused: None,
        }
    }

    pub fn run(&mut self) {
        unsafe {
            let instance = GetModuleHandleW(None).unwrap();
            let class_name = w!("MochiWMClass");

            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                lpszClassName: class_name,
                ..std::mem::zeroed()
            };

            RegisterClassExW(&wc);

            let hwnd = CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TRANSPARENT,
                class_name,
                w!("MochiWM"),
                WS_OVERLAPPEDWINDOW,
                0, 0, 0, 0,
                None, None, instance, None,
            ).unwrap();

            SetLayeredWindowAttributes(hwnd, COLORREF(0), 1, LWA_ALPHA).ok();

            self.register_hotkeys(hwnd);
            self.collect_windows();
            self.tile_all();

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).into() {
                if msg.message == WM_HOTKEY {
                    self.handle_hotkey(msg.wParam.0 as u32);
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    unsafe fn register_hotkeys(&self, hwnd: HWND) {
        let mod_to_val = |m: &str| -> HOT_KEY_MODIFIERS {
            match m.to_lowercase().as_str() {
                "alt" => MOD_ALT,
                "ctrl" | "control" => MOD_CONTROL,
                "shift" => MOD_SHIFT,
                "win" | "super" => MOD_WIN,
                _ => MOD_ALT,
            }
        };

        let key_to_val = |k: &str| -> u32 {
            match k.to_lowercase().as_str() {
                "return" | "enter" => VK_RETURN.0 as u32,
                "q" => 0x51,
                "t" => 0x54,
                "j" => 0x4A,
                "k" => 0x4B,
                "l" => 0x4C,
                "f" => 0x46,
                "h" => 0x48,
                "1" => VK_1.0 as u32,
                "2" => VK_2.0 as u32,
                "3" => VK_3.0 as u32,
                "4" => VK_4.0 as u32,
                "5" => VK_5.0 as u32,
                "6" => VK_6.0 as u32,
                "7" => VK_7.0 as u32,
                "8" => VK_8.0 as u32,
                "9" => VK_9.0 as u32,
                _ => 0,
            }
        };

        let kb = &self.config.keybinds;
        let modif = mod_to_val(&kb.modifier);

        let binds: Vec<(&str, u32)> = vec![
            (&kb.tile_toggle, 1),
            (&kb.kill, 2),
            (&kb.focus_next, 3),
            (&kb.focus_prev, 4),
            (&kb.swap_next, 5),
            (&kb.fullscreen, 6),
            (&kb.launch_terminal, 7),
        ];

        for (key, id) in binds {
            let vk = key_to_val(key);
            if vk != 0 {
                RegisterHotKey(hwnd, id as i32, modif, vk);
            }
        }

        for ws in 1..=self.config.workspace.count {
            RegisterHotKey(hwnd, 100 + ws as i32, modif, VK_1.0 as u32 + ws - 1);
        }
    }

    unsafe fn handle_hotkey(&mut self, id: u32) {
        match id {
            1 => self.toggle_tiling(),
            2 => self.kill_focused(),
            3 => self.focus_next(),
            4 => self.focus_prev(),
            5 => self.swap_next(),
            6 => self.toggle_fullscreen(),
            7 => self.launch_terminal(),
            100..=109 => {
                let ws = id - 100;
                if ws <= self.config.workspace.count {
                    self.switch_workspace(ws);
                }
            }
            _ => {}
        }
    }

    unsafe fn collect_windows(&mut self) {
        let ws = self.workspaces.get_mut(&self.current_workspace).unwrap();
        ws.clear();

        let ptr = LPARAM(ws as *mut Vec<HWND> as isize);
        EnumWindows(Some(enum_callback), ptr).ok();

        ws.retain(|h| {
            let style = GetWindowLongW(*h, GWL_STYLE);
            let ex_style = GetWindowLongW(*h, GWL_EXSTYLE);
            let visible = (style & WS_VISIBLE.0 as i32) != 0;
            let is_tool = (ex_style & WS_EX_TOOLWINDOW.0 as i32) != 0;
            let has_caption = (style & WS_CAPTION.0 as i32) != 0;
            let is_child = (style & WS_CHILD.0 as i32) != 0;
            visible && !is_tool && !is_child && has_caption
        });
    }

    unsafe fn tile_all(&self) {
        if !self.tiling_enabled {
            return;
        }

        let ws = match self.workspaces.get(&self.current_workspace) {
            Some(w) if !w.is_empty() => w,
            _ => return,
        };

        let monitor = self.get_monitor_rect();
        let rects = match self.layout {
            Layout::MasterStack => layout::compute_master_stack(ws, &monitor, self.config.gaps, self.config.inner_gap),
            Layout::Grid => layout::compute_grid(ws, &monitor, self.config.gaps, self.config.inner_gap),
            Layout::Horizontal => layout::compute_horizontal(ws, &monitor, self.config.gaps, self.config.inner_gap),
            Layout::Vertical => layout::compute_vertical(ws, &monitor, self.config.gaps, self.config.inner_gap),
        };

        for (i, hwnd) in ws.iter().enumerate() {
            if let Some(rect) = rects.get(i) {
                SetWindowPos(
                    *hwnd,
                    HWND_TOPMOST,
                    rect.x, rect.y, rect.w, rect.h,
                    SWP_NOACTIVATE,
                ).ok();
            }
        }
    }

    unsafe fn get_monitor_rect(&self) -> Rect {
        let ref_hwnd = self.focused.unwrap_or(HWND(std::ptr::null_mut()));
        let monitor = MonitorFromWindow(ref_hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..std::mem::zeroed()
        };
        GetMonitorInfoW(monitor, &mut info);
        let r = info.rcMonitor;
        Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }
    }

    fn toggle_tiling(&mut self) {
        self.tiling_enabled = !self.tiling_enabled;
    }

    unsafe fn kill_focused(&self) {
        if let Some(hwnd) = self.focused {
            PostMessageW(hwnd, WM_CLOSE, None, None).ok();
        }
    }

    unsafe fn focus_next(&mut self) {
        let ws = match self.workspaces.get(&self.current_workspace) {
            Some(w) if w.len() > 1 => w,
            _ => return,
        };

        let current_idx = ws.iter().position(|h| Some(*h) == self.focused).unwrap_or(0);
        let next_idx = (current_idx + 1) % ws.len();
        self.focus_window(ws[next_idx]);
    }

    unsafe fn focus_prev(&mut self) {
        let ws = match self.workspaces.get(&self.current_workspace) {
            Some(w) if w.len() > 1 => w,
            _ => return,
        };

        let current_idx = ws.iter().position(|h| Some(*h) == self.focused).unwrap_or(0);
        let prev_idx = if current_idx == 0 { ws.len() - 1 } else { current_idx - 1 };
        self.focus_window(ws[prev_idx]);
    }

    unsafe fn focus_window(&mut self, hwnd: HWND) {
        SetForegroundWindow(hwnd).ok();
        self.focused = Some(hwnd);
    }

    unsafe fn swap_next(&mut self) {
        let ws = match self.workspaces.get_mut(&self.current_workspace) {
            Some(w) if w.len() > 1 => w,
            _ => return,
        };

        let current_idx = ws.iter().position(|h| Some(*h) == self.focused).unwrap_or(0);
        let next_idx = (current_idx + 1) % ws.len();
        ws.swap(current_idx, next_idx);
        self.tile_all();
    }

    unsafe fn toggle_fullscreen(&self) {
        if let Some(hwnd) = self.focused {
            let style = GetWindowLongW(hwnd, GWL_STYLE);
            let is_fullscreen = (style & WS_POPUP.0 as i32) != 0;

            if is_fullscreen {
                let new_style = (style | WS_CAPTION.0 as i32 | WS_THICKFRAME.0 as i32) as u32;
                SetWindowLongW(hwnd, GWL_STYLE, new_style as i32);
            } else {
                let new_style = (style & !(WS_CAPTION.0 as i32 | WS_THICKFRAME.0 as i32)) as u32;
                SetWindowLongW(hwnd, GWL_STYLE, new_style as i32);
            }

            let monitor = self.get_monitor_rect();
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                monitor.x, monitor.y, monitor.w, monitor.h,
                SWP_FRAMECHANGED,
            ).ok();
        }
    }

    unsafe fn launch_terminal(&self) {
        use windows::Win32::System::Threading::{CreateProcessW, STARTUPINFOW};

        let _ = CreateProcessW(
            w!("pwsh.exe"),
            PWSTR::null(),
            None,
            None,
            false,
            Default::default(),
            None,
            w!("C:\\Users\\canne"),
            &STARTUPINFOW::default(),
            std::ptr::null_mut(),
        );
    }

    fn switch_workspace(&mut self, target: u32) {
        if target == self.current_workspace || target > self.config.workspace.count {
            return;
        }

        unsafe {
            if let Some(ws) = self.workspaces.get(&self.current_workspace) {
                for hwnd in ws {
                    ShowWindow(*hwnd, SW_HIDE);
                }
            }
        }

        self.current_workspace = target;

        unsafe {
            if let Some(ws) = self.workspaces.get(&self.current_workspace) {
                for hwnd in ws {
                    ShowWindow(*hwnd, SW_SHOW);
                }
            }
            self.tile_all();
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ws = &mut *(lparam.0 as *mut Vec<HWND>);
    ws.push(hwnd);
    TRUE
}
