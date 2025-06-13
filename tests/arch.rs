#[cfg(target_os = "android")]
mod arch_run {
    use localdesktop::proot::process::ArchProcess;

    #[test]
    fn compositor_should_has_an_output_global() {
        let child = ArchProcess::exec("weston-info /tmp/wayland-pb");
        let output = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("wl_output"));
    }
}
