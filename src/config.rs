use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize, serde::Serialize)]
pub struct Config {
    #[serde(default = "default_gaps")]
    pub gaps: i32,

    #[serde(default = "default_border")]
    pub border: i32,

    #[serde(default = "default_inner_gap")]
    pub inner_gap: i32,

    #[serde(default = "default_animation_ms")]
    pub animation_ms: u32,

    #[serde(default)]
    pub keybinds: Keybinds,

    #[serde(default)]
    pub workspace: WorkspaceConfig,
}

#[derive(Debug, Deserialize, serde::Serialize)]
pub struct Keybinds {
    #[serde(default = "default_mod")]
    pub modifier: String,

    #[serde(default = "default_tile_toggle")]
    pub tile_toggle: String,

    #[serde(default = "default_kill")]
    pub kill: String,

    #[serde(default = "default_focus_next")]
    pub focus_next: String,

    #[serde(default = "default_focus_prev")]
    pub focus_prev: String,

    #[serde(default = "default_swap_next")]
    pub swap_next: String,

    #[serde(default = "default_fullscreen")]
    pub fullscreen: String,

    #[serde(default = "default_launch_terminal")]
    pub launch_terminal: String,
}

#[derive(Debug, Deserialize, serde::Serialize)]
pub struct WorkspaceConfig {
    #[serde(default = "default_count")]
    pub count: u32,
}

fn default_gaps() -> i32 { 10 }
fn default_border() -> i32 { 2 }
fn default_inner_gap() -> i32 { 8 }
fn default_animation_ms() -> u32 { 150 }
fn default_mod() -> String { "alt".into() }
fn default_tile_toggle() -> String { "t".into() }
fn default_kill() -> String { "q".into() }
fn default_focus_next() -> String { "j".into() }
fn default_focus_prev() -> String { "k".into() }
fn default_swap_next() -> String { "l".into() }
fn default_fullscreen() -> String { "f".into() }
fn default_launch_terminal() -> String { "return".into() }
fn default_count() -> u32 { 9 }

impl Default for Keybinds {
    fn default() -> Self {
        Self {
            modifier: default_mod(),
            tile_toggle: default_tile_toggle(),
            kill: default_kill(),
            focus_next: default_focus_next(),
            focus_prev: default_focus_prev(),
            swap_next: default_swap_next(),
            fullscreen: default_fullscreen(),
            launch_terminal: default_launch_terminal(),
        }
    }
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self { count: default_count() }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = Self::config_path();
        match fs::read_to_string(&path) {
            Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
                eprintln!("mochiwm: config parse error: {e}, using defaults");
                Self::defaults()
            }),
            Err(_) => {
                eprintln!("mochiwm: no config at {}, using defaults", path.display());
                let defaults = Self::defaults();
                defaults.save();
                defaults
            }
        }
    }

    pub fn save(&self) {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        let content = toml::to_string_pretty(self).unwrap();
        fs::write(path, content).ok();
    }

    pub fn defaults() -> Self {
        Self {
            gaps: default_gaps(),
            border: default_border(),
            inner_gap: default_inner_gap(),
            animation_ms: default_animation_ms(),
            keybinds: Keybinds::default(),
            workspace: WorkspaceConfig::default(),
        }
    }

    fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("mochiwm")
            .join("config.toml")
    }
}
