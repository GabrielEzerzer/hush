//! `hush run`: start a command with the secrets added to its environment.

use std::ffi::OsString;
use std::process::Command;

use anyhow::{Context, Result};

use crate::vault::Secrets;

/// Runs the command and returns its exit code. On Unix hush replaces itself with the command.
pub fn run(command: &[OsString], secrets: &Secrets) -> Result<i32> {
    let (prog, args) = command.split_first().context("no command given")?;
    // Resolve through PATH (and PATHEXT on Windows) so `npm` finds npm.cmd.
    let resolved = which::which(prog).with_context(|| format!("command not found: {}", prog.to_string_lossy()))?;
    let mut cmd = Command::new(&resolved);
    cmd.args(args).envs(secrets);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        Err(err).with_context(|| format!("starting {}", resolved.display()))
    }
    #[cfg(not(unix))]
    {
        // The console delivers Ctrl+C to the child too. hush ignores it and waits, so the
        // shell prompt returns only after the child has shut down.
        ctrlc::set_handler(|| {}).context("installing the Ctrl+C handler")?;
        let status = cmd
            .status()
            .with_context(|| format!("starting {}", resolved.display()))?;
        Ok(status.code().unwrap_or(1))
    }
}
