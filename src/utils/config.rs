use super::logging::PolarBearExpectation;
use serde::{Deserialize, Serialize};
use std::fs;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(not(test))]
pub const ARCH_FS_ROOT: &str = "/data/data/app.polarbear/files/arch";
#[cfg(test)]
pub const ARCH_FS_ROOT: &str = "/data/local/tmp/arch";

pub const XDG_RUNTIME_DIR: &str = "/tmp"; // Main compositor (Weston/KDE), running in **emulated's process** (PRoot), will connect to the socket here

pub const ARCH_FS_ARCHIVE: &str = "https://github.com/termux/proot-distro/releases/download/v4.22.1/archlinux-aarch64-pd-v4.22.1.tar.xz";

pub const WAYLAND_SOCKET_NAME: &str = "wayland-0";

pub const MAX_PANEL_LOG_ENTRIES: usize = 100;

pub const PACMAN_CHECKING_COMMAND: &str =
    "pacman -Q xorg-xwayland && pacman -Qg xfce4 && pacman -Q onboard";

pub const PACMAN_INSTALL_PACKAGES: &str = "xorg-xwayland xfce4 onboard";

pub const CONFIG_FILE: &str = "/etc/localdesktop/localdesktop.toml";

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    pub user: UserConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserConfig {
    pub username: String,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            username: "root".to_string(),
        }
    }
}

pub fn parse_config() -> Config {
    let config_path = format!("{}{}", ARCH_FS_ROOT, CONFIG_FILE);

    fs::read_to_string(config_path)
        .ok()
        .and_then(|content| toml::from_str::<Config>(&content).ok())
        .unwrap_or_default()
}

pub fn save_config(config: Config) {
    // Create config directory if it doesn't exist
    let config_dir = format!("{}/etc/localdesktop", ARCH_FS_ROOT);
    fs::create_dir_all(&config_dir).pb_expect("Failed to create config directory");

    // Create and write config file
    let config_path = format!("{}{}", ARCH_FS_ROOT, CONFIG_FILE);
    let config_str = toml::to_string(&config).pb_expect("Failed to serialize config");
    fs::write(&config_path, config_str).pb_expect("Failed to write config file");
}
