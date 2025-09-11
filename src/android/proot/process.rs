use crate::android::utils::application_context::get_application_context;
use crate::android::proot::storage::StorageSetup;
use crate::core::{config, logging::PolarBearExpectation};
use winit::platform::android::activity::AndroidApp;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::process::{Child, Command, Stdio};

pub type Log = Box<dyn Fn(String)>;

pub struct ArchProcess {
    pub command: String,
    pub user: String,
    pub process: Option<Child>,
    pub panic_on_error: bool,
    pub enable_storage_access: bool,
}

impl ArchProcess {
    pub fn is_supported() -> bool {
        let context = get_application_context();
        let proot_loader = context.native_library_dir.join("libproot_loader.so");

        let mut process = Command::new(context.native_library_dir.join("libproot.so"));
        process
            .env("PROOT_LOADER", proot_loader)
            .env("PROOT_TMP_DIR", config::ARCH_FS_ROOT)
            .arg("-r")
            .arg("/")
            .arg("-L")
            .arg("--link2symlink")
            .arg("--sysvipc")
            .arg("--kill-on-exit")
            .arg("--root-id")
            .arg("sh")
            .arg("-c")
            .arg("cat /proc/cpuinfo 2>&1");

        process
            .stdout(Stdio::piped())
            .spawn()
            .map(|res| res.stdout.is_some())
            .unwrap_or(false)
    }

    /// Run the command inside Proot  
    pub fn spawn(self) -> Self {
        self.spawn_with_android_app(None)
    }

    /// Run the command inside Proot with optional Android app for storage access
    pub fn spawn_with_android_app(mut self, android_app: Option<AndroidApp>) -> Self {
        let context = get_application_context();
        let proot_loader = context.native_library_dir.join("libproot_loader.so");

        let mut process = Command::new(context.native_library_dir.join("libproot.so"));
        process
            .env("PROOT_LOADER", proot_loader)
            .env("PROOT_TMP_DIR", config::ARCH_FS_ROOT)
            .arg("-r")
            .arg(config::ARCH_FS_ROOT)
            .arg("-L")
            .arg("--link2symlink")
            .arg("--sysvipc")
            .arg("--kill-on-exit")
            .arg("--root-id")
            .arg("--bind=/dev")
            .arg("--bind=/proc")
            .arg("--bind=/sys")
            .arg(format!("--bind={}/tmp:/dev/shm", config::ARCH_FS_ROOT))
            .arg("--bind=/dev/urandom:/dev/random")
            .arg("--bind=/proc/self/fd:/dev/fd")
            .arg("--bind=/proc/self/fd/0:/dev/stdin")
            .arg("--bind=/proc/self/fd/1:/dev/stdout")
            .arg("--bind=/proc/self/fd/2:/dev/stderr")
            .arg(format!("--bind={}/proc/.loadavg:/proc/loadavg", config::ARCH_FS_ROOT))
            .arg(format!("--bind={}/proc/.stat:/proc/stat", config::ARCH_FS_ROOT))
            .arg(format!("--bind={}/proc/.uptime:/proc/uptime", config::ARCH_FS_ROOT))
            .arg(format!("--bind={}/proc/.version:/proc/version", config::ARCH_FS_ROOT))
            .arg(format!("--bind={}/proc/.vmstat:/proc/vmstat", config::ARCH_FS_ROOT))
            .arg(format!("--bind={}/proc/.sysctl_entry_cap_last_cap:/proc/sys/kernel/cap_last_cap", config::ARCH_FS_ROOT))
            .arg(format!("--bind={}/proc/.sysctl_inotify_max_user_watches:/proc/sys/fs/inotify/max_user_watches", config::ARCH_FS_ROOT))
            .arg(format!("--bind={}/sys/.empty:/sys/fs/selinux", config::ARCH_FS_ROOT));

        // Add storage bind mounts if enabled and available
        if self.enable_storage_access {
            if let Some(app) = android_app {
                let storage_setup = StorageSetup::new(app);
                let storage_bind_mounts = storage_setup.get_storage_bind_mounts();
                
                for bind_mount in storage_bind_mounts {
                    process.arg(bind_mount);
                    log::debug!("Added storage bind mount to proot");
                }
            } else {
                log::warn!("Storage access enabled but no AndroidApp provided, skipping storage mounts");
            }
        }

        process.arg("/usr/bin/env").arg("-i");

        let home = if self.user == "root" {
            "HOME=/root".to_string()
        } else {
            format!("HOME=/home/{}", self.user)
        };
        process.arg(home);

        process
            .arg("LANG=C.UTF-8")
            .arg("PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/usr/local/games:/usr/games:/system/bin:/system/xbin")
            .arg("TMPDIR=/tmp")
            .arg(format!("USER={}", self.user))
            .arg(format!("LOGNAME={}", self.user));
        if self.user == "root" {
            process.arg("sh");
        } else {
            process
                .arg("runuser")
                .arg("-u")
                .arg(&self.user)
                .arg("--")
                .arg("sh");
        }
        let child = process
            .arg("-c")
            .arg(&self.command)
            .stdout(Stdio::piped())
            .stderr(if self.panic_on_error {
                Stdio::piped()
            } else {
                Stdio::inherit()
            })
            .spawn()
            .pb_expect("Failed to run command");

        self.process.replace(child);
        self
    }

    pub fn exec(command: &str) -> Self {
        ArchProcess {
            command: command.to_string(),
            user: "root".to_string(),
            process: None,
            panic_on_error: false,
            enable_storage_access: false,
        }
        .spawn()
    }

    pub fn exec_as(command: &str, user: &str) -> Self {
        ArchProcess {
            command: command.to_string(),
            user: user.to_string(),
            process: None,
            panic_on_error: false,
            enable_storage_access: false,
        }
        .spawn()
    }

    /// Execute command with storage access enabled
    pub fn exec_with_storage(command: &str, android_app: AndroidApp) -> Self {
        ArchProcess {
            command: command.to_string(),
            user: "root".to_string(),
            process: None,
            panic_on_error: false,
            enable_storage_access: true,
        }
        .spawn_with_android_app(Some(android_app))
    }

    /// Execute command as specific user with storage access enabled
    pub fn exec_as_with_storage(command: &str, user: &str, android_app: AndroidApp) -> Self {
        ArchProcess {
            command: command.to_string(),
            user: user.to_string(),
            process: None,
            panic_on_error: false,
            enable_storage_access: true,
        }
        .spawn_with_android_app(Some(android_app))
    }

    pub fn with_log(self, mut log: impl FnMut(String)) {
        if let Some(child) = self.process {
            let reader = BufReader::new(child.stdout.unwrap());
            for line in reader.lines() {
                let line = line.unwrap();
                log(line);
            }
        }
    }

    pub fn wait_with_output(self) -> std::io::Result<std::process::Output> {
        if let Some(child) = self.process {
            child.wait_with_output()
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Process not spawned",
            ))
        }
    }

    pub fn wait(self) -> std::io::Result<std::process::ExitStatus> {
        if let Some(mut child) = self.process {
            child.wait()
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Process not spawned",
            ))
        }
    }

    pub fn exec_with_panic_on_error(command: &str) {
        if let Some(child) = (ArchProcess {
            command: command.to_string(),
            user: "root".to_string(),
            process: None,
            panic_on_error: true,
            enable_storage_access: false,
        }
        .spawn()
        .process)
        {
            // What is the best way to get full stderr as a string?
            if let Some(stderr) = child.stderr {
                let mut error_output = String::new();
                let mut reader = BufReader::new(stderr);
                reader.read_to_string(&mut error_output).unwrap();
                if error_output.contains("fatal error: see `libproot.so --help`") {
                    panic!("PRoot error: {}", error_output);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[test]
    fn should_echoable() {
        let process = ArchProcess::exec("echo hello");
        let output = process.wait_with_output().expect("Failed to read output");
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello");
    }

    #[test]
    fn should_output_uname() {
        let process = ArchProcess::exec("uname -a");
        let output = process.wait_with_output().expect("Failed to read output");
        log::info!("Output: {}", String::from_utf8_lossy(&output.stdout));
        assert!(String::from_utf8_lossy(&output.stdout)
            .to_lowercase()
            .contains("arch"));
    }

    #[test]
    fn should_run_with_log_successfully() {
        let mut logs = VecDeque::new();
        ArchProcess {
            command: "echo hello".to_string(),
            user: "root".to_string(),
            process: None,
            panic_on_error: true,
            enable_storage_access: false,
        }
        .spawn()
        .with_log(|log| {
            logs.push_back(log.to_string());
        });
        assert!(logs.iter().any(|log| log.contains("hello")));
    }

    #[test]
    fn should_exit_with_success_code() {
        let process = ArchProcess::exec("pacman -Ss chrome");
        let status = process.wait().expect("Failed to wait for process");
        assert_eq!(status.success(), true);
    }

    #[test]
    fn should_exit_with_fail_code() {
        let process = ArchProcess::exec("pacman -Qg plasmma");
        let status = process.wait().expect("Failed to wait for process");
        assert_ne!(status.success(), true);
    }
}
