//! `admit schedule install|uninstall`: the launchd job that runs `sync --if-due`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::ctx::Ctx;
use crate::sync;

pub fn run(ctx: &Ctx, config: &Path, install: bool) -> Result<()> {
    // Uninstall only removes the plist. `current_exe` runs later, and only
    // for install.
    run_with(ctx, config, install, std::env::current_exe)
}

/// `exe` resolves the binary launchd will run (`current_exe`); it is called
/// only when installing, which a test checks by passing one that panics.
fn run_with(ctx: &Ctx, config: &Path, install: bool, exe: impl FnOnce() -> std::io::Result<PathBuf>) -> Result<()> {
    let home = std::env::var_os("HOME").map(PathBuf::from).context("HOME is not set")?;
    let agents = home.join("Library/LaunchAgents");
    let path = agents.join(format!("{}.plist", sync::LABEL));
    let uid = String::from_utf8(std::process::Command::new("id").arg("-u").output()?.stdout)?.trim().to_string();
    let domain = format!("gui/{uid}");
    // Unload any previous version first; failure just means it wasn't loaded.
    let _ = std::process::Command::new("launchctl")
        .args(["bootout", &domain, &path.display().to_string()])
        .stderr(std::process::Stdio::null())
        .status();
    if !install {
        let _ = std::fs::remove_file(&path);
        println!("removed {}", path.display());
        return Ok(());
    }
    let cfg = ctx.config.sync.clone().context("admit.toml has no [sync] section")?;
    let base = std::fs::canonicalize(&ctx.base)?;
    let reports = base.join(&cfg.reports);
    std::fs::create_dir_all(&reports)?;
    let exe = exe()?;
    let body = sync::plist(&exe, &std::fs::canonicalize(config)?, &base, &reports.join("launchd.log"), &cfg)?;
    std::fs::create_dir_all(&agents)?;
    std::fs::write(&path, body)?;
    let st =
        std::process::Command::new("launchctl").args(["bootstrap", &domain, &path.display().to_string()]).status()?;
    anyhow::ensure!(st.success(), "launchctl bootstrap failed for {}", path.display());
    println!("installed {} (runs `{} sync`; reports in {})", path.display(), exe.display(), reports.display());
    Ok(())
}

#[cfg(test)]
#[path = "tests/schedule_tests.rs"]
mod tests;
