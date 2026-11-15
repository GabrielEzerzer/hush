//! `hush run`: start a command with the secrets added to its environment. On Unix hush replaces itself with it; on
//! Windows it ignores the Ctrl+C the console also sends the child and waits, so the prompt returns after the child.

use std::ffi::OsString;
use std::process::Command;

use anyhow::{Context, Result};

use crate::vault::Secrets;

pub fn run(command: &[OsString], secrets: &Secrets) -> Result<i32> {
    let (prog, args) = command.split_first().context("no command given")?;
    let resolved = which::which(prog).with_context(|| format!("command not found: {}", prog.to_string_lossy()))?;
    let mut cmd = Command::new(&resolved);
    cmd.args(args).env_remove("HUSH_KEY").envs(secrets);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        Err(err).with_context(|| format!("starting {}", resolved.display()))
    }
    #[cfg(not(unix))]
    {
        ctrlc::set_handler(|| {}).context("installing the Ctrl+C handler")?;
        let status = cmd
            .status()
            .with_context(|| format!("starting {}", resolved.display()))?;
        Ok(status.code().unwrap_or(1))
    }
}
