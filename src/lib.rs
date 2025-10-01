pub mod core {
    pub mod config;
    pub mod logging;
}

#[cfg(target_os = "android")]
pub mod android {

    pub mod main;
    pub mod app {
        pub mod build;
        pub mod run;
    }
    pub mod backend {
        pub mod wayland;
        pub mod webview;
    }
    pub mod proot {
        pub mod launch;
        pub mod process;
        pub mod setup;
        pub mod storage;
        #[cfg(debug_assertions)]
        pub mod storage_example;
    }
    pub mod utils {
        pub mod application_context;
        pub mod fullscreen_immersive;
        pub mod ndk;
        pub mod storage_permissions;
        pub mod webview;
    }
}
