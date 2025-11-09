use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Opt {
    Debug,
    Release,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum Platform {
    Android,
    Ios,
    Linux,
    Macos,
    Windows,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum Format {
    Aab,
    Apk,
    Appbundle,
    Appdir,
    Appimage,
    Dmg,
    Exe,
    Ipa,
    Msix,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum Store {
    Apple,
    Microsoft,
    Play,
    Sideload,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompileTarget {
    platform: Platform,
    arch: Arch,
    opt: Opt,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum Arch {
    //Arm,
    Arm64,
    X64,
    //X86,
}

#[derive(Clone, Debug)]
pub struct Device {
    backend: Backend,
    id: String,
}

#[derive(Clone, Debug)]
pub struct BuildTarget {
    opt: Opt,
    platform: Platform,
    archs: Vec<Arch>,
    format: Format,
    device: Option<Device>,
    store: Option<Store>,
    provisioning_profile: Option<Vec<u8>>,
    api_key: Option<PathBuf>,
}

pub struct BuildEnv {
    name: String,
    build_target: BuildTarget,
    build_dir: PathBuf,
    cache_dir: PathBuf,
    icon: Option<PathBuf>,
    cargo: Cargo,
    config: Config,
    verbose: bool,
    offline: bool,
}

impl BuildEnv {
    pub fn debug() -> Self {
        Self {
            name: (),
            build_target: (),
            build_dir: (),
            cache_dir: (),
            icon: (),
            cargo: (),
            config: (),
            verbose: (),
            offline: (),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn target(&self) -> &BuildTarget {
        &self.build_target
    }

    pub fn verbose(&self) -> bool {
        self.verbose
    }

    pub fn offline(&self) -> bool {
        self.offline
    }

    pub fn root_dir(&self) -> &Path {
        self.cargo.package_root()
    }

    pub fn build_dir(&self) -> &Path {
        &self.build_dir
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    pub fn opt_dir(&self) -> PathBuf {
        self.build_dir().join(self.target().opt().to_string())
    }

    pub fn platform_dir(&self) -> PathBuf {
        self.opt_dir().join(self.target().platform().to_string())
    }

    pub fn arch_dir(&self, arch: Arch) -> PathBuf {
        self.platform_dir().join(arch.to_string())
    }

    pub fn output(&self) -> PathBuf {
        let output_dir = if self.target().format().supports_multiarch() {
            self.platform_dir()
        } else {
            let target = self.target().compile_targets().next().unwrap();
            self.arch_dir(target.arch())
        };
        let output_name = format!("{}.{}", self.name(), self.target().format().extension());
        output_dir.join(output_name)
    }

    pub fn executable(&self) -> PathBuf {
        let out = self.output();
        match (self.target().format(), self.target().platform()) {
            (Format::Appdir, _) => out.join("AppRun"),
            (Format::Appbundle, Platform::Macos) => {
                out.join("Contents").join("MacOS").join(self.name())
            }
            _ => out,
        }
    }

    pub fn icon(&self) -> Option<&Path> {
        self.icon.as_deref()
    }

    pub fn cargo(&self) -> &Cargo {
        &self.cargo
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn target_sdk_version(&self) -> u32 {
        self.config()
            .android()
            .manifest
            .sdk
            .target_sdk_version
            .unwrap()
    }

    pub fn android_jar(&self) -> PathBuf {
        self.cache_dir()
            .join("Android.sdk")
            .join("platforms")
            .join(format!("android-{}", self.target_sdk_version()))
            .join("android.jar")
    }

    pub fn windows_sdk(&self) -> PathBuf {
        self.cache_dir().join("Windows.sdk")
    }

    pub fn macos_sdk(&self) -> PathBuf {
        self.cache_dir().join("MacOSX.sdk")
    }

    pub fn android_sdk(&self) -> PathBuf {
        self.cache_dir().join("Android.sdk")
    }

    pub fn android_ndk(&self) -> PathBuf {
        self.cache_dir().join("Android.ndk")
    }

    pub fn ios_sdk(&self) -> PathBuf {
        self.cache_dir().join("iPhoneOS.sdk")
    }

    pub fn developer_disk_image(&self, major: u32, minor: u32) -> PathBuf {
        self.cache_dir()
            .join("iPhoneOS.platform")
            .join("DeviceSupport")
            .join(format!("{}.{}", major, minor))
            .join("DeveloperDiskImage.dmg")
    }

    pub fn lldb_server(&self, target: CompileTarget) -> Result<PathBuf> {
        match target.platform() {
            Platform::Android => {
                let ndk = self.android_ndk();
                let lib_dir = ndk.join("usr").join("lib").join(target.ndk_triple());
                Ok(lib_dir.join("lldb-server"))
            }
            Platform::Ios => {
                todo!()
            }
            _ => Ok(which::which("lldb-server")?),
        }
    }

    pub fn cargo_build(&self, target: CompileTarget, target_dir: &Path) -> Result<CargoBuild> {
        let mut cargo = self.cargo.build(target, target_dir)?;
        if target.platform() == Platform::Linux {
            cargo.add_link_arg("-Wl,-rpath");
            cargo.add_link_arg("-Wl,$ORIGIN/lib");
        }
        if target.platform() == Platform::Android {
            let ndk = self.android_ndk();
            let target_sdk_version = self
                .config()
                .android()
                .manifest
                .sdk
                .target_sdk_version
                .unwrap();
            cargo.use_android_ndk(&ndk, target_sdk_version)?;
        }
        if target.platform() == Platform::Windows {
            let sdk = self.windows_sdk();
            if sdk.exists() {
                cargo.use_windows_sdk(&sdk)?;
            }
        }
        if target.platform() == Platform::Macos {
            let sdk = self.macos_sdk();
            if sdk.exists() {
                let minimum_version = self
                    .config()
                    .macos()
                    .info
                    .ls_minimum_system_version
                    .as_ref()
                    .unwrap();
                cargo.use_macos_sdk(&sdk, minimum_version)?;
            } else {
                cargo.add_link_arg("-rpath");
                cargo.add_link_arg("@executable_path/../Frameworks");
            }
        }
        if target.platform() == Platform::Ios {
            let sdk = self.ios_sdk();
            if sdk.exists() {
                let minimum_version = self
                    .config()
                    .ios()
                    .info
                    .minimum_os_version
                    .as_ref()
                    .unwrap();
                cargo.use_ios_sdk(&sdk, minimum_version)?;
            }
        }
        Ok(cargo)
    }

    pub fn cargo_test(&self, target: CompileTarget, target_dir: &Path) -> Result<CargoBuild> {
        let mut cargo = self.cargo.test(target, target_dir)?;
        if target.platform() == Platform::Linux {
            cargo.add_link_arg("-Wl,-rpath");
            cargo.add_link_arg("-Wl,$ORIGIN/lib");
        }
        if target.platform() == Platform::Android {
            let ndk = self.android_ndk();
            let target_sdk_version = self
                .config()
                .android()
                .manifest
                .sdk
                .target_sdk_version
                .unwrap();
            cargo.use_android_ndk(&ndk, target_sdk_version)?;
        }
        if target.platform() == Platform::Windows {
            let sdk = self.windows_sdk();
            if sdk.exists() {
                cargo.use_windows_sdk(&sdk)?;
            }
        }
        if target.platform() == Platform::Macos {
            let sdk = self.macos_sdk();
            if sdk.exists() {
                let minimum_version = self
                    .config()
                    .macos()
                    .info
                    .ls_minimum_system_version
                    .as_ref()
                    .unwrap();
                cargo.use_macos_sdk(&sdk, minimum_version)?;
            } else {
                cargo.add_link_arg("-rpath");
                cargo.add_link_arg("@executable_path/../Frameworks");
            }
        }
        if target.platform() == Platform::Ios {
            let sdk = self.ios_sdk();
            if sdk.exists() {
                let minimum_version = self
                    .config()
                    .ios()
                    .info
                    .minimum_os_version
                    .as_ref()
                    .unwrap();
                cargo.use_ios_sdk(&sdk, minimum_version)?;
            }
        }
        cargo.arg("--no-run");
        Ok(cargo)
    }

    pub fn cargo_artefact(
        &self,
        target_dir: &Path,
        target: CompileTarget,
        crate_type: CrateType,
    ) -> Result<PathBuf> {
        self.cargo.artifact(target_dir, target, None, crate_type)
    }
}
