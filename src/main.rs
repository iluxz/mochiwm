mod config;
mod layout;
mod wm;

use config::Config;
use wm::WindowManager;

fn main() {
    let config = Config::load();
    let mut wm = WindowManager::new(config);
    wm.run();
}
