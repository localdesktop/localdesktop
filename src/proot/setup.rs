use super::process::ArchProcess;
use crate::{
    app::build::{PolarBearBackend, WaylandBackend, WebviewBackend},
    utils::{
        application_context::get_application_context,
        config::{self},
        logging::PolarBearExpectation,
    },
    wayland::compositor::Compositor,
};
use smithay::utils::Clock;
use std::{
    fs::{self, File},
    io::{Read, Write},
    sync::{
        mpsc::{self, Sender},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
};
use tar::Archive;
use winit::platform::android::activity::AndroidApp;
use xz2::read::XzDecoder;

#[derive(Debug)]
pub enum SetupMessage {
    Progress(String),
    Error(String),
}

pub struct SetupOptions {
    pub install_packages: String,
    pub checking_command: String,
    pub android_app: AndroidApp,
    pub mpsc_sender: Sender<SetupMessage>,
}

/// Setup is a process that should be done **only once** when the user installed the app.
/// The setup process consists of several stages.
/// Each stage is a function that takes the `SetupOptions` and returns a `StageOutput`.
type SetupStage = Box<dyn Fn(&SetupOptions) -> StageOutput + Send>;

/// Each stage should indicate whether the associated task is done previously or not.
/// Thus, it should return a finished status if the task is done, so that the setup process can move on to the next stage.
/// Otherwise, it should return a `JoinHandle`, so that the setup process can wait for the task to finish, but not block the main thread so that the setup progress can be reported to the user.
type StageOutput = Option<JoinHandle<()>>;

fn setup_fake_sysdata_stage(options: &SetupOptions) -> StageOutput {
    let fs_root = std::path::Path::new(config::ARCH_FS_ROOT);
    let mpsc_sender = options.mpsc_sender.clone();

    if !fs_root.join("proc/.version").exists() {
        return Some(thread::spawn(move || {
            mpsc_sender
                .send(SetupMessage::Progress(
                    "Setting up fake system data...".to_string(),
                ))
                .unwrap_or(());

            // Create necessary directories - don't fail if they already exist
            let _ = fs::create_dir_all(fs_root.join("proc"));
            let _ = fs::create_dir_all(fs_root.join("sys"));
            let _ = fs::create_dir_all(fs_root.join("sys/.empty"));

            // Set permissions - only try to set permissions if we're on Unix and have the capability
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                // Try to set permissions, but don't fail if we can't
                let _ =
                    fs::set_permissions(fs_root.join("proc"), fs::Permissions::from_mode(0o700));
                let _ = fs::set_permissions(fs_root.join("sys"), fs::Permissions::from_mode(0o700));
                let _ = fs::set_permissions(
                    fs_root.join("sys/.empty"),
                    fs::Permissions::from_mode(0o700),
                );
            }

            // Create fake proc files
            let proc_files = [
                    ("proc/.loadavg", "0.12 0.07 0.02 2/165 765\n"),
                    ("proc/.stat", "cpu  1957 0 2877 93280 262 342 254 87 0 0\ncpu0 31 0 226 12027 82 10 4 9 0 0\n"),
                    ("proc/.uptime", "124.08 932.80\n"),
                    ("proc/.version", "Linux version 6.2.1 (proot@termux) (gcc (GCC) 12.2.1 20230201, GNU ld (GNU Binutils) 2.40) #1 SMP PREEMPT_DYNAMIC Wed, 01 Mar 2023 00:00:00 +0000\n"),
                    ("proc/.vmstat", "nr_free_pages 1743136\nnr_zone_inactive_anon 179281\nnr_zone_active_anon 7183\n"),
                    ("proc/.sysctl_entry_cap_last_cap", "40\n"),
                    ("proc/.sysctl_inotify_max_user_watches", "4096\n"),
                ];

            for (path, content) in proc_files {
                fs::write(fs_root.join(path), content)
                    .pb_expect(&format!("Permission denied while writing to {}", path));
            }
        }));
    }
    None
}

fn setup_arch_fs(options: &SetupOptions) -> StageOutput {
    let context = get_application_context().pb_expect("Failed to get application context");
    let fs_root = std::path::Path::new(config::ARCH_FS_ROOT);

    if !fs_root.exists()
        || fs::read_dir(fs_root)
            .pb_expect("Failed to read fs_root directory")
            .next()
            .is_none()
    {
        let mpsc_sender = options.mpsc_sender.clone();
        return Some(thread::spawn(move || {
            mpsc_sender
                .send(SetupMessage::Progress(
                    "Downloading Arch Linux FS...".to_string(),
                ))
                .pb_expect("Failed to send log message");

            let temp_file = context.data_dir.join("archlinux-fs.tar.xz");

            let response = reqwest::blocking::get(config::ARCH_FS_ARCHIVE)
                .pb_expect("Failed to download Arch Linux FS");

            let total_size = response.content_length().unwrap_or(0);

            let mut file =
                File::create(&temp_file).pb_expect("Failed to create temp file for Arch Linux FS");

            let mut downloaded = 0u64;
            let mut buffer = [0u8; 8192];
            let mut reader = response;
            loop {
                let n = reader
                    .read(&mut buffer)
                    .pb_expect("Failed to read from response");
                if n == 0 {
                    break;
                }
                file.write_all(&buffer[..n])
                    .pb_expect("Failed to write to file");
                downloaded += n as u64;
                if total_size > 0 {
                    let percent = (downloaded * 100 / total_size).min(100);
                    let downloaded_mb = downloaded as f64 / 1024.0 / 1024.0;
                    let total_mb = total_size as f64 / 1024.0 / 1024.0;
                    mpsc_sender
                        .send(SetupMessage::Progress(format!(
                            "Downloading Arch Linux FS... {}% ({:.2} MB / {:.2} MB)",
                            percent, downloaded_mb, total_mb
                        )))
                        .unwrap_or(());
                }
            }

            mpsc_sender
                .send(SetupMessage::Progress(
                    "Extracting Arch Linux FS...".to_string(),
                ))
                .pb_expect("Failed to send log message");

            // Ensure the extracted directory is clean
            let extracted_dir = &context.data_dir.join("archlinux-aarch64");
            fs::remove_dir_all(extracted_dir).unwrap_or(());

            // Extract tar file directly to the final destination
            let tar_file = std::fs::File::open(&temp_file)
                .pb_expect("Failed to open downloaded Arch Linux FS file");
            let tar = XzDecoder::new(tar_file);
            let mut archive = Archive::new(tar);
            archive
                .unpack(context.data_dir.clone())
                .pb_expect("Failed to extract Arch Linux FS .tar.xz file");

            // Move the extracted files to the final destination
            fs::rename(extracted_dir, fs_root)
                .pb_expect("Failed to rename extracted files to final destination");

            // Clean up the temporary file
            fs::remove_file(&temp_file).pb_expect("Failed to remove temporary file");
        }));
    } else {
        return None;
    }
}

fn install_dependencies(options: &SetupOptions) -> StageOutput {
    let SetupOptions {
        install_packages,
        checking_command,
        mpsc_sender,
        android_app: _,
    } = options;

    let checking_command = checking_command.clone();
    let installed = move || {
        ArchProcess::exec(&checking_command)
            .wait()
            .pb_expect("Failed to check whether the installation target is installed")
            .success()
    };

    if installed() {
        return None;
    }

    let install_packages = install_packages.clone();
    let mpsc_sender = mpsc_sender.clone();
    return Some(thread::spawn(move || {
        loop {
            ArchProcess::exec("rm -f /var/lib/pacman/db.lck"); // Install dependencies
            ArchProcess::exec(&format!(
                "stdbuf -oL pacman -Syu {} --noconfirm --noprogressbar",
                install_packages
            ))
            .with_log(|it| {
                mpsc_sender
                    .send(SetupMessage::Progress(it))
                    .pb_expect("Failed to send log message");
            });
            if installed() {
                break;
            }
        }
    }));
}

fn setup_firefox_config(_: &SetupOptions) -> StageOutput {
    // Create the Firefox root directory if it doesn't exist
    let firefox_root = format!("{}/usr/lib/firefox", crate::utils::config::ARCH_FS_ROOT);
    fs::create_dir_all(&firefox_root).pb_expect("Failed to create Firefox root directory");

    // Create the defaults/pref directory
    let pref_dir = format!("{}/defaults/pref", firefox_root);
    fs::create_dir_all(&pref_dir).pb_expect("Failed to create Firefox pref directory");

    // Create autoconfig.js in defaults/pref
    let autoconfig_js = r#"pref("general.config.filename", "localdesktop.cfg");
pref("general.config.obscure_value", 0);
"#;

    fs::write(format!("{}/autoconfig.js", pref_dir), autoconfig_js)
        .pb_expect("Failed to write Firefox autoconfig.js");

    // Create localdesktop.cfg in the Firefox root directory
    let firefox_cfg = r#"// Auto updated by Local Desktop on each startup, do not edit manually
defaultPref("media.cubeb.sandbox", false);
defaultPref("security.sandbox.content.level", 0);
"#; // It is required that the first line of this file is a comment, even if you have nothing to comment. Docs: https://support.mozilla.org/en-US/kb/customizing-firefox-using-autoconfig

    fs::write(format!("{}/localdesktop.cfg", firefox_root), firefox_cfg)
        .pb_expect("Failed to write Firefox configuration");

    None
}

pub fn setup(android_app: AndroidApp) -> PolarBearBackend {
    let (sender, receiver) = mpsc::channel();
    let progress = Arc::new(Mutex::new(0));

    let options = SetupOptions {
        install_packages: config::PACMAN_INSTALL_PACKAGES.to_string(),
        checking_command: config::PACMAN_CHECKING_COMMAND.to_string(),
        android_app,
        mpsc_sender: sender.clone(),
    };

    let stages: Vec<SetupStage> = vec![
        Box::new(setup_arch_fs),            // Step 1. Setup Arch FS
        Box::new(setup_fake_sysdata_stage), // Step 2. Setup fake system data
        Box::new(install_dependencies),     // Step 3. Install dependencies
        Box::new(setup_firefox_config),     // Step 4. Setup Firefox config
    ];

    let handle_stage_error = |e: Box<dyn std::any::Any + Send>, sender: &Sender<SetupMessage>| {
        let error_msg = if let Some(e) = e.downcast_ref::<String>() {
            format!("Stage execution failed: {}", e)
        } else if let Some(e) = e.downcast_ref::<&str>() {
            format!("Stage execution failed: {}", e)
        } else {
            "Stage execution failed: Unknown error".to_string()
        };
        sender.send(SetupMessage::Error(error_msg)).unwrap_or(());
    };

    let fully_installed = 'outer: loop {
        for (i, stage) in stages.iter().enumerate() {
            if let Some(handle) = stage(&options) {
                let progress_clone = progress.clone();
                let sender_clone = sender.clone();
                thread::spawn(move || {
                    let progress = progress_clone;
                    let progress_value = ((i) as u16 * 100 / stages.len() as u16) as u16;
                    *progress.lock().unwrap() = progress_value;

                    // Wait for the current stage to finish
                    if let Err(e) = handle.join() {
                        handle_stage_error(e, &sender_clone);
                        return;
                    }

                    // Process the remaining stages in the same loop
                    for (j, next_stage) in stages.iter().enumerate().skip(i + 1) {
                        let progress_value = ((j) as u16 * 100 / stages.len() as u16) as u16;
                        *progress.lock().unwrap() = progress_value;
                        if let Some(next_handle) = next_stage(&options) {
                            if let Err(e) = next_handle.join() {
                                handle_stage_error(e, &sender_clone);
                                return;
                            }

                            // Increment progress and send it
                            let next_progress_value =
                                ((j + 1) as u16 * 100 / stages.len() as u16) as u16;
                            *progress.lock().unwrap() = next_progress_value;
                        }
                    }

                    // All stages are done, we need to replace the WebviewBackend with the WaylandBackend
                    // Or, easier, just restart the whole app
                    *progress.lock().unwrap() = 100;
                    sender_clone
                        .send(SetupMessage::Progress(
                            "Installation finished, please restart the app".to_string(),
                        ))
                        .pb_expect("Failed to send installation finished message");
                });

                // Setup is still running in the background, but we need to return control
                // so that the main thread can continue to report progress to the user
                break 'outer false;
            }
        }

        // All stages were done previously, no need to wait for anything
        break 'outer true;
    };

    if fully_installed {
        PolarBearBackend::Wayland(WaylandBackend {
            compositor: Compositor::build().pb_expect("Failed to build compositor"),
            graphic_renderer: None,
            clock: Clock::new(),
            key_counter: 0,
            scale_factor: 1.0,
        })
    } else {
        PolarBearBackend::WebView(WebviewBackend::build(receiver, progress))
    }
}
