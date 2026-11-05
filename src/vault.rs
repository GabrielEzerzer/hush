//! The vault: one folder outside every repo, holding one encrypted file per project and environment.
//!
//! ```text
//! <home>/vault.toml                  format version and the key's public fingerprint
//! <home>/projects/<project>/<env>.hush
//! <home>/projects/<project>/.lock    held while a write is in progress
//! ```
//!
//! The key lives in the OS keychain, or in `HUSH_KEY` when that is set. When `HUSH_KEY` is set,
//! the keychain is never read or written, which is how tests and CI stay off the real key.

use std::collections::BTreeMap;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use zeroize::Zeroizing;

use crate::crypto::{self, MasterKey};
use crate::names::check_path_name;

pub type Secrets = BTreeMap<String, String>;

const FORMAT: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Meta {
    version: u32,
    key_id: String,
}

pub fn home() -> Result<PathBuf> {
    match std::env::var_os("HUSH_HOME") {
        Some(p) if !p.is_empty() => Ok(PathBuf::from(p)),
        _ => Ok(dirs::data_dir()
            .context("cannot find the user data folder; set HUSH_HOME")?
            .join("hush")),
    }
}

fn read_meta(home: &Path) -> Result<Option<Meta>> {
    let path = home.join("vault.toml");
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let meta: Meta = toml::from_str(&text).with_context(|| format!("{} is not valid", path.display()))?;
    ensure!(meta.version == FORMAT, "the vault uses format {}, which this hush does not know; update hush", meta.version);
    Ok(Some(meta))
}

fn require_meta(home: &Path) -> Result<Meta> {
    read_meta(home)?.with_context(|| format!("no hush vault at {}; run `hush setup` first", home.display()))
}

pub fn require_vault(home: &Path) -> Result<()> {
    require_meta(home).map(|_| ())
}

fn env_key() -> Result<Option<MasterKey>> {
    match std::env::var("HUSH_KEY") {
        Ok(s) => {
            let s = Zeroizing::new(s);
            if s.trim().is_empty() {
                return Ok(None);
            }
            Ok(Some(crypto::decode_key(&s).context("HUSH_KEY is not a valid hush key")?))
        }
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => bail!("HUSH_KEY is not valid text"),
    }
}

fn create_private_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().recursive(true).mode(0o700).create(path)
    }
    #[cfg(not(unix))]
    {
        // On Windows the folder inherits the per-user ACL of %APPDATA%.
        fs::create_dir_all(path)
    }
    .with_context(|| format!("creating {}", path.display()))
}

pub struct SetupOutcome {
    pub created_vault: bool,
    pub new_key: bool,
    pub key_from_env: bool,
}

/// Creates the vault, or verifies an existing one. Never overwrites a key already in the keychain.
pub fn setup(home: &Path, recovered: Option<MasterKey>) -> Result<SetupOutcome> {
    let meta = read_meta(home)?;
    let mut new_key = false;
    let key_from_env;
    let key = if let Some(k) = env_key()? {
        ensure!(recovered.is_none(), "HUSH_KEY is set; unset it to restore a key into the OS keychain");
        key_from_env = true;
        k
    } else {
        key_from_env = false;
        match (keychain::load()?, recovered) {
            (Some(existing), Some(r)) => {
                ensure!(existing[..] == r[..], "the OS keychain already holds a different hush key; hush will not overwrite it");
                existing
            }
            (Some(existing), None) => existing,
            (None, Some(r)) => {
                if let Some(m) = &meta {
                    ensure!(crypto::key_id(&r) == m.key_id, "that recovery key does not match the vault at {}", home.display());
                }
                keychain::store(&r)?;
                r
            }
            (None, None) => {
                ensure!(
                    meta.is_none(),
                    "the vault at {} exists but its key is not in the OS keychain; run `hush setup --recover` with your recovery key",
                    home.display()
                );
                let k = crypto::new_key();
                keychain::store(&k)?;
                new_key = true;
                k
            }
        }
    };

    if let Some(m) = meta {
        ensure!(
            crypto::key_id(&key) == m.key_id,
            "the hush key does not match the vault at {} (key {}, vault {})",
            home.display(),
            crypto::key_id(&key),
            m.key_id
        );
        return Ok(SetupOutcome { created_vault: false, new_key, key_from_env });
    }
    create_private_dir(&home.join("projects"))?;
    let meta = toml::to_string(&Meta { version: FORMAT, key_id: crypto::key_id(&key) })?;
    fs::write(home.join("vault.toml"), meta).context("writing vault.toml")?;
    Ok(SetupOutcome { created_vault: true, new_key, key_from_env })
}

fn project_dir(home: &Path, project: &str) -> Result<PathBuf> {
    check_path_name("project", project)?;
    Ok(home.join("projects").join(project))
}

pub fn project_exists(home: &Path, project: &str) -> Result<bool> {
    Ok(project_dir(home, project)?.is_dir())
}

/// Returns whether the project was new.
pub fn create_project(home: &Path, project: &str) -> Result<bool> {
    require_vault(home)?;
    let dir = project_dir(home, project)?;
    if dir.is_dir() {
        return Ok(false);
    }
    create_private_dir(&dir)?;
    Ok(true)
}

pub fn delete_project(home: &Path, project: &str) -> Result<()> {
    let dir = project_dir(home, project)?;
    ensure!(dir.is_dir(), "no project '{project}' in the vault");
    // shortcut: does not take the project lock, so a write racing the delete can fail; deletes are rare and manual.
    fs::remove_dir_all(&dir).with_context(|| format!("deleting {}", dir.display()))
}

/// Every project with the environments that hold secrets. Names are folder names, not secret.
pub fn projects(home: &Path) -> Result<Vec<(String, Vec<String>)>> {
    require_vault(home)?;
    let mut out = Vec::new();
    for entry in fs::read_dir(home.join("projects")).context("reading the projects folder")? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !entry.file_type()?.is_dir() || check_path_name("project", &name).is_err() {
            continue;
        }
        let mut envs = Vec::new();
        for f in fs::read_dir(entry.path())? {
            let f = f?.file_name().to_string_lossy().into_owned();
            if let Some(env) = f.strip_suffix(".hush")
                && check_path_name("environment", env).is_ok()
            {
                envs.push(env.to_owned());
            }
        }
        envs.sort();
        out.push((name, envs));
    }
    out.sort();
    Ok(out)
}

pub struct Vault {
    home: PathBuf,
    key: MasterKey,
}

impl Vault {
    pub fn open(home: PathBuf) -> Result<Vault> {
        let meta = require_meta(&home)?;
        let key = match env_key()? {
            Some(k) => k,
            None => keychain::load()?.context(
                "no hush key in the OS keychain; run `hush setup --recover` with your recovery key, or set HUSH_KEY",
            )?,
        };
        ensure!(
            crypto::key_id(&key) == meta.key_id,
            "the hush key does not match the vault at {} (key {}, vault {})",
            home.display(),
            crypto::key_id(&key),
            meta.key_id
        );
        Ok(Vault { home, key })
    }

    pub fn key(&self) -> &MasterKey {
        &self.key
    }

    fn paths(&self, project: &str, env: &str) -> Result<(PathBuf, PathBuf)> {
        check_path_name("environment", env)?;
        let dir = project_dir(&self.home, project)?;
        ensure!(
            dir.is_dir(),
            "no project '{project}' in the vault; create it with `hush init` in its repo or `hush project create {project}`"
        );
        let file = dir.join(format!("{env}.hush"));
        Ok((dir, file))
    }

    fn aad(project: &str, env: &str) -> String {
        format!("hush/v1/{project}/{env}")
    }

    /// The environment's secrets, or None if nothing was ever stored there.
    pub fn read(&self, project: &str, env: &str) -> Result<Option<Secrets>> {
        let (_, file) = self.paths(project, env)?;
        let data = match fs::read(&file) {
            Ok(d) => d,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("reading {}", file.display())),
        };
        let plain = crypto::open(&self.key, &Self::aad(project, env), &data)
            .with_context(|| format!("reading {}", file.display()))?;
        // shortcut: values are zeroized only in the decrypted buffer, not in the map's Strings; upgrade if a threat model includes memory dumps.
        let secrets = serde_json::from_slice(&plain).with_context(|| format!("{} holds invalid data", file.display()))?;
        Ok(Some(secrets))
    }

    /// Read, change and rewrite one environment under the project lock. The new file replaces the
    /// old one by rename, so a crash leaves either the old or the new version, never a partial one.
    /// If `change` fails, nothing is written.
    pub fn update<T>(&self, project: &str, env: &str, change: impl FnOnce(&mut Secrets) -> Result<T>) -> Result<T> {
        let (dir, file) = self.paths(project, env)?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(".lock"))
            .context("opening the project lock")?;
        lock.lock().context("waiting for the project lock")?;

        let mut secrets = self.read(project, env)?.unwrap_or_default();
        let out = change(&mut secrets)?;
        let plain = Zeroizing::new(serde_json::to_vec(&secrets)?);
        let sealed = crypto::seal(&self.key, &Self::aad(project, env), &plain)?;

        let mut tmp = NamedTempFile::new_in(&dir).context("creating a temporary file in the vault")?;
        tmp.write_all(&sealed)?;
        tmp.as_file().sync_all()?;
        tmp.persist(&file).map_err(|e| e.error).with_context(|| format!("replacing {}", file.display()))?;
        drop(lock);
        Ok(out)
    }
}

#[cfg(any(windows, target_os = "macos"))]
mod keychain {
    use super::*;

    const SERVICE: &str = "hush";
    const ACCOUNT: &str = "vault-key";

    fn entry() -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, ACCOUNT).context("opening the OS keychain")
    }

    pub fn load() -> Result<Option<MasterKey>> {
        match entry()?.get_password() {
            Ok(s) => {
                let s = Zeroizing::new(s);
                Ok(Some(crypto::decode_key(&s).context("the hush key in the OS keychain is corrupt")?))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e).context("reading the hush key from the OS keychain"),
        }
    }

    pub fn store(key: &MasterKey) -> Result<()> {
        entry()?
            .set_password(&crypto::encode_key(key))
            .context("saving the hush key to the OS keychain")?;
        // Read it back: a keychain that accepts the key but cannot return it would lose every secret.
        let back = load()?.context("the OS keychain did not keep the hush key")?;
        ensure!(back[..] == key[..], "the OS keychain returned a different key than the one saved");
        Ok(())
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod keychain {
    use super::*;

    pub fn load() -> Result<Option<MasterKey>> {
        Ok(None)
    }

    pub fn store(_: &MasterKey) -> Result<()> {
        bail!("hush has no OS keychain support on this platform; set HUSH_KEY to a vault key (64 hex digits) instead")
    }
}
