#[cfg(not(target_os = "android"))]
#[test]
fn run_integration_tests_on_android() {
    use clap::Parser;
    use std::{env, fmt::format, fs::create_dir_all, io, mem, process::Command, ptr};
    use xbuild::{Arch, BuildArgs, BuildEnv, CompileTarget, Opt, Platform};

    let args = BuildArgs::parse_from(["xbuild"].iter());
    let env = BuildEnv::new(args).expect("Failed to create BuildEnv");
    let platform_dir = env.platform_dir();
    create_dir_all(&platform_dir).expect("Failed to create platform dir");

    println!("Build executable tests");

    let mut devices = vec![];
    let adb_devices_output = Command::new("adb")
        .arg("devices")
        .output()
        .expect("Failed to run `adb devices`");
    let mut lines = std::str::from_utf8(&adb_devices_output.stdout)
        .expect("Failed to parse `adb devices` output")
        .lines();
    lines.next();
    for line in lines {
        if let Some(id) = line.split_whitespace().next() {
            devices.push(id);
        }
    }

    assert_ne!(
        devices.len(),
        0,
        "No devices connected. Please connect a physical device or start an emulator"
    );

    for device in devices {
        let target_dir = platform_dir.join(Arch::Arm64.to_string()).join("cargo");
        let cargo = env
            .cargo_build(
                CompileTarget::new(Platform::Android, Arch::Arm64, Opt::Debug),
                &target_dir,
            )
            .expect("Failed to create cargo build command");

        // Transmute the cargo reference to a raw pointer
        let cargo_ptr: *const u8 = unsafe { mem::transmute(&cargo) };

        // Calculate the offset of the cmd field
        // let cmd_offset = mem::offset_of!(CargoBuild, cmd);
        let cmd_offset = 0; // Remove this when CargoBuild is public

        // Get a raw pointer to the cmd field
        let cmd_ptr = unsafe { cargo_ptr.add(cmd_offset) as *mut Command };

        // Dereference the raw pointer to get the value
        let old_command = unsafe { &*cmd_ptr };
        println!("-- {:?}", old_command);

        // Build a new command
        let mut new_command = Command::new(old_command.get_program());
        new_command.arg("test").arg("--no-run").arg("-q");
        new_command.args(old_command.get_args().skip(1));
        new_command.envs(
            old_command
                .get_envs()
                .filter_map(|(k, v)| v.map(|v| (k, v))),
        );
        new_command.current_dir(
            old_command
                .get_current_dir()
                .expect("Failed to get current dir"),
        );
        println!("++ {:?}", new_command);

        // Replace the old command with the new command
        unsafe { ptr::write(cmd_ptr, new_command) };

        cargo.exec().expect("Failed to execute cargo command");

        let executable_tests_output = target_dir.join("aarch64-linux-android/debug/deps");
        // Look for the executable file, whose name starts with `localdesktop-` and has no extension
        let executable_test_binary = executable_tests_output
            .read_dir()
            .expect(&format!(
                "Failed to read directory {}",
                executable_tests_output.display()
            ))
            .map(|entry| entry.expect("Failed to read entry").file_name())
            .find(|file_name| {
                let name = file_name.to_string_lossy();
                name.starts_with("localdesktop-") && !name.contains(".")
            });
        let executable_test_binary: String = executable_test_binary
            .expect(&format!(
                "No executable test binary found in {:?}",
                executable_tests_output
            ))
            .to_string_lossy()
            .into_owned();

        println!(
            "Run executable tests {} on device {}",
            executable_test_binary, device
        );

        // Check if assets have been pushed to the device
        let mut list_assets = Command::new("adb")
            .arg("shell")
            .arg("ls")
            .arg("/data/local/tmp")
            .output()
            .expect("Failed to run `adb shell ls`");
        let list_assets_output = std::str::from_utf8(&list_assets.stdout)
            .expect("Failed to parse `adb shell ls` output");
        let assets_pushed = list_assets_output.contains("arch");

        if !assets_pushed {
            let cwd = env::current_dir().expect("Failed to get current dir");

            // Use TMPDIR or /tmp directly for the tar.xz file
            let tmpdir = std::env::var("TMPDIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"));

            // Use the new URL from config
            let tar_xz_url = localdesktop::utils::config::ARCH_FS_ARCHIVE;
            let tar_xz_filename = tar_xz_url.split('/').last().unwrap();
            let tar_xz_path = tmpdir.join(tar_xz_filename);

            if !tar_xz_path.exists() {
                println!("Downloading {}", tar_xz_url);
                let mut resp = reqwest::blocking::get(tar_xz_url).expect("Failed to download file");
                let mut out =
                    std::fs::File::create(&tar_xz_path).expect("Failed to create tar.xz file");
                let mut downloaded = 0u64;
                let total_size = resp.content_length().unwrap_or(0);
                let mut buffer = [0u8; 8192];
                use std::io::{Read, Write};
                loop {
                    let n = resp
                        .read(&mut buffer)
                        .expect("Failed to read from response");
                    if n == 0 {
                        break;
                    }
                    out.write_all(&buffer[..n])
                        .expect("Failed to write to file");
                    downloaded += n as u64;
                    if total_size > 0 {
                        let percent = (downloaded * 100 / total_size).min(100);
                        print!("\rDownloading... {}%", percent);
                        std::io::stdout().flush().unwrap();
                    }
                }
                println!("\nDownload complete.");
            }

            // Use XzDecoder to decompress the tar.xz file to a tar file in the assets dir
            let tar_path = cwd.join("assets").join("arch.tar");
            let tar_xz_file =
                std::fs::File::open(&tar_xz_path).expect("Failed to open tar.xz file");
            let mut tar_xz_decoder = xz2::read::XzDecoder::new(tar_xz_file);
            let mut tar_file = std::fs::File::create(&tar_path).expect("Failed to create tar file");
            std::io::copy(&mut tar_xz_decoder, &mut tar_file)
                .expect("Failed to copy tar.xz to tar");

            // push mock assets to device
            let assets = [
                "assets/libs/arm64-v8a/libproot.so",
                "assets/libs/arm64-v8a/libproot_loader.so",
                "assets/arch.tar",
            ];
            for asset in assets {
                let file_name = asset.split('/').last().unwrap();
                let mut adb_push = Command::new("adb");
                adb_push.arg("-s").arg(device);
                adb_push.arg("push").arg(cwd.join(asset));
                adb_push.arg("/data/local/tmp/");
                let status = adb_push
                    .status()
                    .expect("Failed to execute adb push")
                    .code();
                assert_eq!(status, Some(0));
                if asset.ends_with(".so") {
                    // chmod +x
                    let mut adb_shell = Command::new("adb");
                    adb_shell.arg("-s").arg(device);
                    adb_shell.arg("shell");
                    adb_shell.arg("chmod");
                    adb_shell.arg("+x");
                    adb_shell.arg(format!("/data/local/tmp/{}", file_name));
                    let status = adb_shell
                        .status()
                        .expect("Failed to execute adb shell")
                        .code();
                    assert_eq!(status, Some(0));
                }

                if asset.ends_with(".tar") {
                    use std::ffi::CString;
                    use tar::Archive;
                    use xz2::read::XzDecoder;

                    // Open the .tar.xz file as an asset from disk
                    let tar_file =
                        std::fs::File::open(cwd.join(asset)).expect("Failed to open .tar.xz file");

                    // Clean up any previous extracted directory
                    let extracted_dir = std::path::Path::new("/data/local/tmp/archlinux-aarch64");
                    let _ = std::fs::remove_dir_all(&extracted_dir);

                    // Extract the tar file directly to the target directory
                    let tar = XzDecoder::new(tar_file);
                    let mut archive = Archive::new(tar);
                    archive
                        .unpack("/data/local/tmp")
                        .expect("Failed to extract Arch Linux FS .tar.xz file");

                    // Move the extracted files to the desired final destination
                    let final_dir = std::path::Path::new("/data/local/tmp/arch");
                    std::fs::rename(extracted_dir, final_dir)
                        .expect("Failed to rename extracted files to final destination");
                }
            }

            // Remove temporary arch.tar file
            std::fs::remove_file(&tar_path).expect("Failed to remove tar file");
        }

        // adb push <test_binary> /data/local/tmp/
        let mut adb_push = Command::new("adb");
        adb_push.arg("-s").arg(device);
        adb_push
            .arg("push")
            .arg(&executable_tests_output.join(&executable_test_binary));
        adb_push.arg("/data/local/tmp/");
        let status = adb_push
            .status()
            .expect("Failed to execute adb push")
            .code();
        assert_eq!(status, Some(0));

        // adb shell /data/local/tmp/<test_binary>
        let mut adb_shell = Command::new("adb");
        adb_push.arg("-s").arg(device);
        adb_shell.arg("shell");
        adb_shell.arg(format!("/data/local/tmp/{}", &executable_test_binary));
        let status = adb_shell
            .status()
            .expect("Failed to execute adb shell")
            .code();
        assert_eq!(status, Some(0));
        println!("Done");
    }
}
