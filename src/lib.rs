#![cfg(target_os = "android")]

pub mod android_main;
pub mod app {
    pub mod build;
    pub mod run;
    pub mod backend {
        pub mod wayland;
        pub mod webview;
    }
}
pub mod proot {
    pub mod launch;
    pub mod process;
    pub mod setup;
}
pub mod utils {
    pub mod application_context;
    pub mod config;
    pub mod fullscreen_immersive;
    pub mod logging;
    pub mod ndk;
    pub mod socket;
    pub mod webview;
}
