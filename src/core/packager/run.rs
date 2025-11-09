pub fn run(env: &BuildEnv) -> Result<()> {
    let out = env.executable();
    if let Some(device) = env.target().device() {
        device.run(env, &out)?;
    } else {
        anyhow::bail!("no device specified");
    }
    Ok(())
}
