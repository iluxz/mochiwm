use crate::config::Config;
use crate::layout::{self, Layout, Rect, WindowState};
use std::collections::HashMap;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const IGNORE_CLASSES: &[&str] = &[
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "MochiWMClass",
    "WorkerW",
    "Progman",
    "SysDrag",
    "tooltips_class32",
    "NotifyIconOverflowWindow",
];

pub struct WindowManager {
    config: Config,
    workspaces: HashMap<u32, Vec<WindowState>>,
    current_workspace: u32,
    layout: Layout,
    tiling_enabled: bool,
    focused: Option<HWND>,
}

static mut GLOBAL_WM: Option<*mut WindowManager> = None;

impl WindowManager {
    pub fn new(config: Config) -> Self {
        let mut config = config;
        if config.workspace.count == 0 {
            config.workspace.count = 1;
        }
        if config.workspace.count > 9 {
            config.workspace.count = 9;
        }

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

    pub fn set_global(wm: &mut Self) {
        unsafe {
            GLOBAL_WM = Some(wm as *mut WindowManager);
        }
    }

    pub fn run(&mut self) {
        unsafe {
            let instance = match GetModuleHandleW(None) {
                Ok(h) => h,
                Err(_) => return,
            };
            let class_name = w!("MochiWMClass");

            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                lpszClassName: class_name,
                ..std::mem::zeroed()
            };

            if RegisterClassExW(&wc) == 0 {
                return;
            }

            let hwnd = match CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW,
                class_name,
                w!("MochiWM"),
                WS_POPUP,
                0, 0, 0, 0,
                None, None, instance, None,
            ) {
                Ok(h) => h,
                Err(_) => return,
            };

            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 1, LWA_ALPHA);

            self.register_hotkeys(hwnd);
            self.collect_windows();
            self.tile_all();

            SetWinEventHook(
                EVENT_OBJECT_CREATE,
                EVENT_OBJECT_DESTROY,
                None,
                Some(win_event_callback),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            );

            let mut msg = MSG::default();
            loop {
                let result = GetMessageW(&mut msg, None, 0, 0);
                if result.0 == 0 || result.0 == -1 {
                    break;
                }
                if msg.message == WM_HOTKEY {
                    self.handle_hotkey(msg.wParam.0 as u32);
                }
                let _ = TranslateMessage(&msg);
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
                _ => 0,
            }
        };

        let kb = &self.config.keybinds;
        let modif = mod_to_val(&kb.modifier);

        let mut used_vkeys: std::collections::HashSet<u32> = std::collections::HashSet::new();

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
            if vk != 0 && !used_vkeys.contains(&vk) {
                let _ = RegisterHotKey(hwnd, id as i32, modif, vk);
                used_vkeys.insert(vk);
            }
        }

        for ws in 1..=self.config.workspace.count {
            let vk = VK_1.0 as u32 + ws - 1;
            if !used_vkeys.contains(&vk) {
                let _ = RegisterHotKey(hwnd, 100 + ws as i32, modif, vk);
                used_vkeys.insert(vk);
            }
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
                if ws >= 1 && ws <= self.config.workspace.count {
                    self.switch_workspace(ws);
                }
            }
            _ => {}
        }
    }

    unsafe fn should_tile(hwnd: HWND) -> bool {
        if !IsWindowVisible(hwnd).as_bool() {
            return false;
        }

        let style = GetWindowLongW(hwnd, GWL_STYLE);
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);

        let is_child = (style & WS_CHILD.0 as i32) != 0;
        let is_tool = (ex_style & WS_EX_TOOLWINDOW.0 as i32) != 0;
        let has_caption = (style & WS_CAPTION.0 as i32) != 0;
        let is_appwindow = (ex_style & WS_EX_APPWINDOW.0 as i32) != 0;

        if is_child || is_tool {
            return false;
        }

        if !has_caption && !is_appwindow {
            return false;
        }

        let mut class_name = [0u16; 256];
        GetClassNameW(hwnd, &mut class_name);
        let class = String::from_utf16_lossy(
            &class_name[..class_name.iter().position(|&c| c == 0).unwrap_or(256)]
        );

        for ignore in IGNORE_CLASSES {
            if class.eq_ignore_ascii_case(ignore) {
                return false;
            }
        }

        let owner = GetWindow(hwnd, GW_OWNER).unwrap_or(HWND(std::ptr::null_mut()));
        if !owner.0.is_null() {
            return false;
        }

        true
    }

    unsafe fn collect_windows(&mut self) {
        let ws = self.workspaces.get_mut(&self.current_workspace).unwrap();
        ws.clear();

        let mut collected: Vec<HWND> = Vec::new();
        {
            let ptr = &mut collected as *mut Vec<HWND>;
            let _ = EnumWindows(Some(enum_callback), LPARAM(ptr as isize));
        }

        for hwnd in collected {
            if IsWindow(hwnd).as_bool() && Self::should_tile(hwnd) {
                ws.push(WindowState { hwnd, prev_rect: None });
            }
        }
    }

    unsafe fn remove_dead_windows(&mut self) {
        for ws in self.workspaces.values_mut() {
            ws.retain(|w| IsWindow(w.hwnd).as_bool());
        }
        if let Some(focused) = self.focused {
            if !IsWindow(focused).as_bool() {
                self.focused = None;
            }
        }
    }

    fn hwnd_in_workspaces(&self, hwnd: HWND) -> bool {
        for ws in self.workspaces.values() {
            if ws.iter().any(|w| w.hwnd == hwnd) {
                return true;
            }
        }
        false
    }

    unsafe fn add_window(&mut self, hwnd: HWND) {
        if !Self::should_tile(hwnd) {
            return;
        }
        if self.hwnd_in_workspaces(hwnd) {
            return;
        }

        if let Some(ws) = self.workspaces.get_mut(&self.current_workspace) {
            ws.push(WindowState { hwnd, prev_rect: None });
        }
        self.tile_all();
    }

    unsafe fn remove_window(&mut self, hwnd: HWND) {
        let mut removed = false;
        for ws in self.workspaces.values_mut() {
            let len_before = ws.len();
            ws.retain(|w| w.hwnd != hwnd);
            if ws.len() < len_before {
                removed = true;
            }
        }
        if removed {
            if self.focused == Some(hwnd) {
                self.focused = None;
            }
            self.tile_all();
        }
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

        for (i, state) in ws.iter().enumerate() {
            if let Some(rect) = rects.get(i) {
                let w = rect.w.max(1);
                let h = rect.h.max(1);
                let _ = SetWindowPos(
                    state.hwnd,
                    HWND_TOP,
                    rect.x, rect.y, w, h,
                    SWP_NOACTIVATE,
                );
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
        let _ = GetMonitorInfoW(monitor, &mut info);
        let r = info.rcMonitor;
        Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }
    }

    fn toggle_tiling(&mut self) {
        self.tiling_enabled = !self.tiling_enabled;
        if self.tiling_enabled {
            unsafe { self.tile_all(); }
        }
    }

    unsafe fn kill_focused(&self) {
        if let Some(hwnd) = self.focused {
            let _ = PostMessageW(hwnd, WM_CLOSE, None, None);
        }
    }

    unsafe fn focus_next(&mut self) {
        self.remove_dead_windows();
        let ws = match self.workspaces.get(&self.current_workspace) {
            Some(w) if w.len() > 1 => w,
            _ => return,
        };

        let current_idx = ws.iter().position(|w| Some(w.hwnd) == self.focused).unwrap_or(0);
        let next_idx = (current_idx + 1) % ws.len();
        let next = ws[next_idx].hwnd;
        self.focus_window(next);
    }

    unsafe fn focus_prev(&mut self) {
        self.remove_dead_windows();
        let ws = match self.workspaces.get(&self.current_workspace) {
            Some(w) if w.len() > 1 => w,
            _ => return,
        };

        let current_idx = ws.iter().position(|w| Some(w.hwnd) == self.focused).unwrap_or(0);
        let prev_idx = if current_idx == 0 { ws.len() - 1 } else { current_idx - 1 };
        let prev = ws[prev_idx].hwnd;
        self.focus_window(prev);
    }

    unsafe fn focus_window(&mut self, hwnd: HWND) {
        let _ = SetForegroundWindow(hwnd);
        self.focused = Some(hwnd);
    }

    unsafe fn swap_next(&mut self) {
        self.remove_dead_windows();
        let ws = match self.workspaces.get_mut(&self.current_workspace) {
            Some(w) if w.len() > 1 => w,
            _ => return,
        };

        let current_idx = ws.iter().position(|w| Some(w.hwnd) == self.focused).unwrap_or(0);
        let next_idx = (current_idx + 1) % ws.len();
        ws.swap(current_idx, next_idx);
        if self.tiling_enabled {
            self.tile_all();
        }
    }

    unsafe fn toggle_fullscreen(&mut self) {
        if let Some(hwnd) = self.focused {
            let style = GetWindowLongW(hwnd, GWL_STYLE);
            let has_caption = (style & WS_CAPTION.0 as i32) != 0;

            if has_caption {
                let mut window_rect = RECT::default();
                let _ = GetWindowRect(hwnd, &mut window_rect);

                let saved = Rect {
                    x: window_rect.left,
                    y: window_rect.top,
                    w: window_rect.right - window_rect.left,
                    h: window_rect.bottom - window_rect.top,
                };

                if let Some(ws) = self.workspaces.get_mut(&self.current_workspace) {
                    if let Some(state) = ws.iter_mut().find(|w| w.hwnd == hwnd) {
                        state.prev_rect = Some(saved);
                    }
                }

                let new_style = style & !(WS_CAPTION.0 as i32 | WS_THICKFRAME.0 as i32);
                SetWindowLongW(hwnd, GWL_STYLE, new_style);

                let monitor = self.get_monitor_rect();
                let _ = SetWindowPos(
                    hwnd,
                    HWND_TOP,
                    monitor.x, monitor.y, monitor.w, monitor.h,
                    SWP_FRAMECHANGED,
                );
            } else {
                let new_style = style | WS_CAPTION.0 as i32 | WS_THICKFRAME.0 as i32;
                SetWindowLongW(hwnd, GWL_STYLE, new_style);

                let restored = self.workspaces.get(&self.current_workspace)
                    .and_then(|ws| ws.iter().find(|w| w.hwnd == hwnd))
                    .and_then(|w| w.prev_rect);

                if let Some(prev) = restored {
                    let _ = SetWindowPos(
                        hwnd,
                        HWND_TOP,
                        prev.x, prev.y, prev.w, prev.h,
                        SWP_FRAMECHANGED,
                    );
                } else {
                    let _ = SetWindowPos(
                        hwnd,
                        HWND_TOP,
                        0, 0, 0, 0,
                        SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE,
                    );
                }

                if let Some(ws) = self.workspaces.get_mut(&self.current_workspace) {
                    if let Some(state) = ws.iter_mut().find(|w| w.hwnd == hwnd) {
                        state.prev_rect = None;
                    }
                }
            }
        }
    }

    unsafe fn launch_terminal(&self) {
        use windows::Win32::System::Threading::CreateProcessW;
        use windows::Win32::System::Threading::STARTUPINFOW;

        let app = w!("pwsh.exe");
        let mut cmd = [0u16; 2];

        let _ = CreateProcessW(
            app,
            PWSTR(cmd.as_mut_ptr()),
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
                for state in ws {
                    let _ = ShowWindow(state.hwnd, SW_HIDE);
                }
            }
        }

        self.current_workspace = target;

        unsafe {
            if let Some(ws) = self.workspaces.get(&self.current_workspace) {
                for state in ws {
                    let _ = ShowWindow(state.hwnd, SW_SHOW);
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
    if WindowManager::should_tile(hwnd) {
        let ws = &mut *(lparam.0 as *mut Vec<HWND>);
        ws.push(hwnd);
    }
    TRUE
}

unsafe extern "system" fn win_event_callback(
    _hwin_event_hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _id_event_thread: u32,
    _dwms_event_time: u32,
) {
    if hwnd.0.is_null() {
        return;
    }

    if let Some(ptr) = GLOBAL_WM {
        match event {
            EVENT_OBJECT_CREATE => {
                if WindowManager::should_tile(hwnd) && !(*ptr).hwnd_in_workspaces(hwnd) {
                    (*ptr).add_window(hwnd);
                }
            }
            EVENT_OBJECT_DESTROY => {
                (*ptr).remove_window(hwnd);
            }
            _ => {}
        }
    }
}
