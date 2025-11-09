//! LLVM utilities

use anyhow::{bail, ensure, Context, Result};
use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Returns the set of additional libraries that need to be bundled with
/// the given library, scanned recursively.
///
/// Any libraries in `provided_libs` will be treated as available, without
/// being emitted. Any other library not in `search_paths` or `provided_libs`
/// will result in an error.
pub fn list_needed_libs_recursively(
    lib: &Path,
    search_paths: &[&Path],
    provided_libs: &HashSet<OsString>,
) -> Result<(HashSet<PathBuf>, bool)> {
    let mut to_copy = HashSet::new();
    let mut needs_cpp_shared = false;

    let mut artifacts = vec![lib.to_path_buf()];
    while let Some(artifact) = artifacts.pop() {
        for need in list_needed_libs(&artifact).with_context(|| {
            format!(
                "Unable to read needed libraries from `{}`",
                artifact.display()
            )
        })? {
            if need == "libc++_shared.so" {
                // c++_shared is available in the NDK but not on-device. Communicate that
                //  we need to copy it, once
                needs_cpp_shared = true;
            } else if !provided_libs.contains(OsStr::new(&need)) {
                if let Some(path) = find_library_path(search_paths, &need).with_context(|| {
                    format!(
                        "Could not iterate one or more search directories in `{:?}` while searching for library `{}`",
                        search_paths, need
                    )
                })? {
                    to_copy.insert(path.clone());
                    artifacts.push(path);
                } else {
                    bail!("Shared library `{}` not found", need);
                }
            }
        }
    }

    Ok((to_copy, needs_cpp_shared))
}

/// List all required shared libraries as per the dynamic section
fn list_needed_libs(library_path: &Path) -> Result<HashSet<String>> {
    let mut readelf = Command::new("llvm-readobj");
    let readelf = readelf.arg("--needed-libs").arg(library_path);
    let output = readelf
        .output()
        .with_context(|| format!("Failed to run `{:?}`", readelf))?;
    ensure!(
        output.status.success(),
        "Failed to run `{:?}`: {}",
        readelf,
        output.status
    );
    let output = std::str::from_utf8(&output.stdout).unwrap();
    let (_, output) = output.split_once("NeededLibraries [\n").unwrap();
    let output = output.strip_suffix("]\n").unwrap();
    let mut needed = HashSet::new();

    for line in output.lines() {
        let lib = line.trim_start();
        needed.insert(lib.to_string());
    }
    Ok(needed)
}

/// List names of shared libraries inside directory
pub fn find_libs_in_dir(path: &Path) -> Result<HashSet<OsString>> {
    let mut libs = HashSet::new();
    let entries = std::fs::read_dir(path)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() && path.extension() == Some(OsStr::new("so")) {
            libs.insert(entry.file_name().to_owned());
        }
    }
    Ok(libs)
}

/// Resolves native library using search paths
fn find_library_path(paths: &[&Path], library: &str) -> Result<Option<PathBuf>> {
    for path in paths {
        let lib_path = path.join(library);
        if lib_path.exists() {
            return Ok(Some(dunce::canonicalize(lib_path)?));
        }
    }
    Ok(None)
}
