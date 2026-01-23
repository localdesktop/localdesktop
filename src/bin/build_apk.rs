#[cfg(target_os = "android")]
pub mod android {
    pub mod build_apk;
}

#[cfg(not(target_os = "android"))]
fn main() {
    println!(
        "`build_apk` is intended to run on Android hosts where the host and target architectures match."
    );
}

#[cfg(target_os = "android")]
fn main() {
    android::build_apk::run();
}
