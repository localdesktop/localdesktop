use crate::cargo::CrateType;
use crate::download::DownloadManager;
use crate::task::TaskRunner;
use crate::{BuildEnv, Format, Opt, Platform};
use anyhow::{ensure, Context, Result};
use appbundle::AppBundle;
use appimage::AppImage;
use msix::Msix;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::Path;

pub fn build(env: &BuildEnv) -> Result<()> {
    let platform_dir = env.platform_dir();
    std::fs::create_dir_all(&platform_dir)?;

    let mut runner = TaskRunner::new(3, env.verbose());

    runner.start_task("Fetch precompiled artifacts");
    let manager = DownloadManager::new(env)?;
    if !env.offline() {
        manager.prefetch()?;
        runner.end_verbose_task();
    }

    runner.start_task(format!("Build rust `{}`", env.name));
    let bin_target = env.target().platform() != Platform::Android;
    let has_lib = env.root_dir().join("src").join("lib.rs").exists();
    if bin_target || has_lib {
        if env.target().platform() == Platform::Android && env.config().android().gradle {
            super::gradle::prepare(env)?;
        }
        for target in env.target().compile_targets() {
            let arch_dir = platform_dir.join(target.arch().to_string());
            let mut cargo = env.cargo_build(target, &arch_dir.join("cargo"))?;
            if !bin_target {
                cargo.arg("--lib");
            }
            cargo.exec()?;
        }
        runner.end_verbose_task();
    }

    runner.start_task(format!("Create {}", env.target().format()));
    match env.target().platform() {
        Platform::Android => {
            let out = platform_dir.join(format!("{}.{}", env.name(), env.target().format()));
            ensure!(has_lib, "Android APKs/AABs require a library");

            let mut libraries = vec![];

            for target in env.target().compile_targets() {
                let arch_dir = platform_dir.join(target.arch().to_string());
                let cargo_dir = arch_dir.join("cargo");
                let lib = env.cargo_artefact(&cargo_dir, target, CrateType::Cdylib)?;

                let ndk = env.android_ndk();

                let deps_dir = {
                    let arch_dir = if target.is_host()? {
                        cargo_dir.to_path_buf()
                    } else {
                        cargo_dir.join(target.rust_triple()?)
                    };
                    let opt_dir = arch_dir.join(target.opt().to_string());
                    opt_dir.join("deps")
                };

                let mut search_paths = env
                    .cargo()
                    .lib_search_paths(&cargo_dir, target)
                    .with_context(|| {
                        format!(
                            "Finding libraries in `{}` for {:?}",
                            cargo_dir.display(),
                            target
                        )
                    })?;
                search_paths.push(deps_dir);
                let search_paths = search_paths.iter().map(AsRef::as_ref).collect::<Vec<_>>();

                let ndk_sysroot_libs = ndk.join("usr/lib").join(target.ndk_triple());
                let provided_libs_paths = [
                    ndk_sysroot_libs.as_path(),
                    &*ndk_sysroot_libs.join(
                        // Use libraries (symbols) from the lowest NDK that is supported by the application,
                        // to prevent inadvertently making newer APIs available:
                        // https://developer.android.com/ndk/guides/sdk-versions
                        env.config()
                            .android()
                            .manifest
                            .sdk
                            .min_sdk_version
                            .unwrap()
                            .to_string(),
                    ),
                ];

                let mut explicit_libs = vec![lib];

                // Collect the libraries the user wants to include
                for runtime_lib_path in env.config().runtime_libs(env.target().platform()) {
                    let abi_dir = env
                        .cargo()
                        .package_root()
                        .join(runtime_lib_path)
                        .join(target.android_abi().as_str());
                    let entries = std::fs::read_dir(&abi_dir).with_context(|| {
                        format!(
                            "Runtime libraries for current ABI not found at `{}`",
                            abi_dir.display()
                        )
                    })?;
                    for entry in entries {
                        let entry = entry?;
                        let path = entry.path();
                        if !path.is_dir() && path.extension() == Some(OsStr::new("so")) {
                            explicit_libs.push(path);
                        }
                    }
                }

                // Collect the names of libraries provided by the user, and assume these
                // are available for other dependencies to link to, too.
                let mut included_libs = explicit_libs
                    .iter()
                    .map(|p| p.file_name().unwrap().to_owned())
                    .collect::<HashSet<_>>();

                // Collect the names of all libraries that are available on Android
                for provided_libs_path in provided_libs_paths {
                    included_libs.extend(super::llvm::find_libs_in_dir(provided_libs_path)?);
                }

                // libc++_shared is bundled with the NDK but not available on-device
                included_libs.remove(OsStr::new("libc++_shared.so"));

                let mut needs_cpp_shared = false;

                for lib in explicit_libs {
                    libraries.push((target.android_abi(), lib.clone()));

                    let (extra_libs, cpp_shared) = super::llvm::list_needed_libs_recursively(
                                &lib,
                                &search_paths,
                                &included_libs,
                            )
                            .with_context(|| {
                                format!(
                                    "Failed to collect all required libraries for `{}` with `{:?}` available libraries and `{:?}` shippable libraries",
                                    lib.display(),
                                    provided_libs_paths,
                                    search_paths
                                )
                            })?;
                    needs_cpp_shared |= cpp_shared;
                    for lib in extra_libs {
                        libraries.push((target.android_abi(), lib));
                    }
                }
                if needs_cpp_shared {
                    let cpp_shared = ndk_sysroot_libs.join("libc++_shared.so");
                    libraries.push((target.android_abi(), cpp_shared));
                }
            }

            super::gradle::build(env, libraries, &out)?;
            runner.end_verbose_task();
            return Ok(());
        }
    }
    runner.end_task();

    Ok(())
}
