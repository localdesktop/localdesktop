use super::process::ArchProcess;
use crate::{
    android::{
        app::build::PolarBearBackend,
        backend::{
            wayland::{Compositor, WaylandBackend},
            webview::{ErrorVariant, WebviewBackend},
        },
        utils::application_context::get_application_context,
    },
    core::{
        config::{CommandConfig, ROOT_FS_ARCHIVE_PARTS, ROOT_FS_ROOT},
        logging::PolarBearExpectation,
    },
};
use backhand::{FilesystemReader, InnerNode};
use pathdiff::diff_paths;
use smithay::utils::Clock;
use std::{
    fs::{self, File},
    io::{BufReader, Read, Write},
    os::unix::fs::{symlink, PermissionsExt},
    path::Path,
    sync::{
        mpsc::{self, Sender},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
};
use winit::platform::android::activity::AndroidApp;

#[derive(Debug)]
pub enum SetupMessage {
    Progress(String),
    Error(String),
}

pub struct SetupOptions {
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

fn setup_root_fs(options: &SetupOptions) -> StageOutput {
    let context = get_application_context();
    let squashfs_path = context.data_dir.join("filesystem.squashfs");
    let fs_root = Path::new(ROOT_FS_ROOT).to_path_buf();
    let extracted_dir = context.data_dir.join("rootfs-extracted");
    let mpsc_sender = options.mpsc_sender.clone();

    // Only run if the fs_root is missing or empty
    // TODO: Setup integration test to make sure on clean install, the fs_root is either non existent or empty
    let need_setup = fs_root.read_dir().map_or(true, |mut d| d.next().is_none());
    if need_setup {
        return Some(thread::spawn(move || {
            loop {
                if !squashfs_path.exists() {
                    mpsc_sender
                        .send(SetupMessage::Progress(
                            "Downloading root filesystem...".to_string(),
                        ))
                        .pb_expect("Failed to send log message");

                    let client = reqwest::blocking::Client::new();
                    let mut total_size = 0u64;
                    let mut unknown_length = false;
                    for part_url in ROOT_FS_ARCHIVE_PARTS {
                        match client.head(*part_url).send() {
                            Ok(response) => {
                                if let Some(length) = response.content_length() {
                                    total_size += length;
                                } else {
                                    unknown_length = true;
                                }
                            }
                            Err(_) => {
                                unknown_length = true;
                            }
                        }
                    }
                    if unknown_length {
                        total_size = 0;
                    }

                    let mut file = File::create(&squashfs_path)
                        .pb_expect("Failed to create temp file for root filesystem");

                    let mut downloaded = 0u64;
                    let mut buffer = [0u8; 8192];
                    let mut last_percent = 0u8;
                    let mut last_reported_bytes = 0u64;

                    for part_url in ROOT_FS_ARCHIVE_PARTS {
                        let response = client
                            .get(*part_url)
                            .send()
                            .pb_expect("Failed to download root filesystem part");
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
                                let percent = (downloaded * 100 / total_size).min(100) as u8;
                                if percent != last_percent {
                                    let downloaded_mb = downloaded as f64 / 1024.0 / 1024.0;
                                    let total_mb = total_size as f64 / 1024.0 / 1024.0;
                                    mpsc_sender
                                        .send(SetupMessage::Progress(format!(
                                            "Downloading root filesystem... {}% ({:.2} MB / {:.2} MB)",
                                            percent, downloaded_mb, total_mb
                                        )))
                                        .unwrap_or(());
                                    last_percent = percent;
                                }
                            } else if downloaded - last_reported_bytes >= 8 * 1024 * 1024 {
                                let downloaded_mb = downloaded as f64 / 1024.0 / 1024.0;
                                mpsc_sender
                                    .send(SetupMessage::Progress(format!(
                                        "Downloading root filesystem... {:.2} MB",
                                        downloaded_mb
                                    )))
                                    .unwrap_or(());
                                last_reported_bytes = downloaded;
                            }
                        }
                    }

                    if total_size > 0 && downloaded != total_size {
                        let _ = fs::remove_file(&squashfs_path);
                        mpsc_sender
                            .send(SetupMessage::Error(format!(
                                "Downloaded root filesystem size mismatch ({} / {} bytes). Restarting download...",
                                downloaded, total_size
                            )))
                            .unwrap_or(());
                        continue;
                    }
                }

                mpsc_sender
                    .send(SetupMessage::Progress(
                        "Extracting root filesystem...".to_string(),
                    ))
                    .pb_expect("Failed to send log message");

                // Ensure the extracted directory is clean
                let _ = fs::remove_dir_all(&extracted_dir);

                let extract_result = (|| -> Result<(), String> {
                    let squashfs_file = File::open(&squashfs_path)
                        .map_err(|e| format!("Failed to open root filesystem image: {}", e))?;
                    let reader = BufReader::new(squashfs_file);
                    let filesystem = FilesystemReader::from_reader(reader)
                        .map_err(|e| format!("Failed to read filesystem image: {}", e))?;

                    for node in filesystem.files() {
                        if node.fullpath == Path::new("/") {
                            continue;
                        }

                        let relative_path = node
                            .fullpath
                            .strip_prefix(Path::new("/"))
                            .unwrap_or(node.fullpath.as_path());
                        let output_path = extracted_dir.join(relative_path);

                        match &node.inner {
                            InnerNode::Dir(_) => {
                                fs::create_dir_all(&output_path).map_err(|e| {
                                    format!(
                                        "Failed to create directory from filesystem image: {}",
                                        e
                                    )
                                })?;
                                #[cfg(unix)]
                                let _ = fs::set_permissions(
                                    &output_path,
                                    fs::Permissions::from_mode(node.header.permissions as u32),
                                );
                            }
                            InnerNode::File(file) => {
                                if let Some(parent) = output_path.parent() {
                                    fs::create_dir_all(parent).map_err(|e| {
                                        format!("Failed to create parent directories: {}", e)
                                    })?;
                                }
                                let mut output_file = File::create(&output_path).map_err(|e| {
                                    format!("Failed to create file from filesystem image: {}", e)
                                })?;
                                let mut reader = filesystem.file(file).reader();
                                std::io::copy(&mut reader, &mut output_file).map_err(|e| {
                                    format!("Failed to extract file from filesystem image: {}", e)
                                })?;
                                #[cfg(unix)]
                                let _ = fs::set_permissions(
                                    &output_path,
                                    fs::Permissions::from_mode(node.header.permissions as u32),
                                );
                            }
                            InnerNode::Symlink(link) => {
                                if let Some(parent) = output_path.parent() {
                                    fs::create_dir_all(parent).map_err(|e| {
                                        format!("Failed to create parent directories: {}", e)
                                    })?;
                                }
                                symlink(&link.link, &output_path).map_err(|e| {
                                    format!("Failed to create symlink from filesystem image: {}", e)
                                })?;
                            }
                            InnerNode::CharacterDevice(_)
                            | InnerNode::BlockDevice(_)
                            | InnerNode::NamedPipe
                            | InnerNode::Socket => {}
                        }
                    }
                    Ok(())
                })();

                match extract_result {
                    Ok(()) => break,
                    Err(err) => {
                        let _ = fs::remove_dir_all(&extracted_dir);
                        let _ = fs::remove_file(&squashfs_path);
                        mpsc_sender
                            .send(SetupMessage::Error(format!(
                                "Failed to extract root filesystem: {}. Restarting download...",
                                err
                            )))
                            .unwrap_or(());
                    }
                }
            }

            // Move the extracted files to the final destination
            fs::rename(&extracted_dir, fs_root)
                .pb_expect("Failed to rename extracted files to final destination");

            // Clean up the temporary file
            let _ = fs::remove_file(&squashfs_path);
        }));
    }
    None
}

fn simulate_linux_sysdata_stage(options: &SetupOptions) -> StageOutput {
    let fs_root = Path::new(ROOT_FS_ROOT);
    let mpsc_sender = options.mpsc_sender.clone();

    if !fs_root.join("proc/.version").exists() {
        return Some(thread::spawn(move || {
            mpsc_sender
                .send(SetupMessage::Progress(
                    "Simulating Linux system data...".to_string(),
                ))
                .pb_expect(&format!("Failed to send log message"));

            // Create necessary directories - don't fail if they already exist
            let _ = fs::create_dir_all(fs_root.join("proc"));
            let _ = fs::create_dir_all(fs_root.join("sys"));
            let _ = fs::create_dir_all(fs_root.join("sys/.empty"));

            // Set permissions - only try to set permissions if we're on Unix and have the capability
            #[cfg(unix)]
            {
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
                let _ = fs::write(fs_root.join(path), content)
                    .pb_expect(&format!("Permission denied while writing to {}", path));
            }
        }));
    }
    None
}

fn setup_firefox_config(_: &SetupOptions) -> StageOutput {
    // Create the Firefox root directory if it doesn't exist
    let firefox_root = format!("{}/usr/lib/firefox", ROOT_FS_ROOT);
    let _ = fs::create_dir_all(&firefox_root).pb_expect("Failed to create Firefox root directory");

    // Create the defaults/pref directory
    let pref_dir = format!("{}/defaults/pref", firefox_root);
    let _ = fs::create_dir_all(&pref_dir).pb_expect("Failed to create Firefox pref directory");

    // Create autoconfig.js in defaults/pref
    let autoconfig_js = r#"pref("general.config.filename", "localdesktop.cfg");
pref("general.config.obscure_value", 0);
"#;

    let _ = fs::write(format!("{}/autoconfig.js", pref_dir), autoconfig_js)
        .pb_expect("Failed to write Firefox autoconfig.js");

    // Create localdesktop.cfg in the Firefox root directory
    let firefox_cfg = r#"// Auto updated by Local Desktop on each startup, do not edit manually
defaultPref("media.cubeb.sandbox", false);
defaultPref("security.sandbox.content.level", 0);
"#; // It is required that the first line of this file is a comment, even if you have nothing to comment. Docs: https://support.mozilla.org/en-US/kb/customizing-firefox-using-autoconfig

    let _ = fs::write(format!("{}/localdesktop.cfg", firefox_root), firefox_cfg)
        .pb_expect("Failed to write Firefox configuration");

    None
}

fn fix_xkb_symlink(options: &SetupOptions) -> StageOutput {
    let fs_root = Path::new(ROOT_FS_ROOT);
    let xkb_path = fs_root.join("usr/share/X11/xkb");
    let mpsc_sender = options.mpsc_sender.clone();

    if let Ok(meta) = fs::symlink_metadata(&xkb_path) {
        if meta.file_type().is_symlink() {
            if let Ok(target) = fs::read_link(&xkb_path) {
                if target.is_absolute() {
                    log::info!(
                        "Absolute symlink target detected: {} -> {}. This is a problem because libxkbcommon is loaded in NDK, whose / is not the root FS!",
                        xkb_path.display(),
                        target.display()
                    );
                    // Compute the relative path from /usr/share/X11/xkb to /usr/share/xkeyboard-config-2
                    // Both are inside the chroot, so strip the fs_root prefix
                    let xkb_inside = Path::new("/usr/share/X11/xkb");
                    let target_inside = Path::new("/usr/share/xkeyboard-config-2");
                    let rel_target = diff_paths(target_inside, xkb_inside.parent().unwrap())
                        .unwrap_or_else(|| target_inside.to_path_buf());
                    log::info!(
                        "Fixing with new relative symlink: {} -> {}",
                        xkb_path.display(),
                        rel_target.display()
                    );
                    // Remove the old symlink
                    let _ = fs::remove_file(&xkb_path);
                    // Create the new relative symlink
                    if let Err(e) = symlink(&rel_target, &xkb_path) {
                        mpsc_sender
                            .send(SetupMessage::Error(format!(
                                "Failed to create relative symlink for xkb: {}",
                                e
                            )))
                            .unwrap_or(());
                    }
                }
            }
        }
    }
    None
}

pub fn setup(android_app: AndroidApp) -> PolarBearBackend {
    let (sender, receiver) = mpsc::channel();
    let progress = Arc::new(Mutex::new(0));

    if ArchProcess::is_supported() {
        sender
            .send(SetupMessage::Progress(
                "✅ Your device is supported!".to_string(),
            ))
            .unwrap_or(());
    } else {
        return PolarBearBackend::WebView(WebviewBackend {
            socket_port: 0,
            progress,
            error: ErrorVariant::Unsupported,
        });
    }

    let options = SetupOptions {
        android_app,
        mpsc_sender: sender.clone(),
    };

    let stages: Vec<SetupStage> = vec![
        Box::new(setup_root_fs),                // Step 1. Setup root FS (extract)
        Box::new(simulate_linux_sysdata_stage), // Step 2. Simulate Linux system data
        Box::new(setup_firefox_config),         // Step 4. Setup Firefox config
        Box::new(fix_xkb_symlink),              // Step 5. Fix xkb symlink (last)
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
