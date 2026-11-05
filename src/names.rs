//! Validation for every name that reaches the filesystem or a child's environment.

use anyhow::{Result, ensure};

/// Device names Windows reserves in any folder, with or without an extension.
const WINDOWS_RESERVED: &[&str] = &["con", "prn", "aux", "nul"];

/// Project and environment names become folder and file names in the vault.
pub fn check_path_name(kind: &str, s: &str) -> Result<()> {
    let charset = s
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    let first = s
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    ensure!(
        charset && first && s.len() <= 64,
        "invalid {kind} name '{s}': use lowercase letters, digits, '-' and '_', starting with a letter or digit, up to 64 characters"
    );
    let reserved = WINDOWS_RESERVED.contains(&s)
        || ((s.starts_with("com") || s.starts_with("lpt")) && s.len() == 4 && s.as_bytes()[3].is_ascii_digit());
    ensure!(
        !reserved,
        "invalid {kind} name '{s}': Windows reserves it as a device name"
    );
    Ok(())
}

/// Secret names become environment variable names.
pub fn check_secret_name(s: &str) -> Result<()> {
    let first = s.bytes().next().is_some_and(|b| b.is_ascii_alphabetic() || b == b'_');
    let rest = s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    ensure!(
        first && rest && s.len() <= 256,
        "invalid secret name '{s}': use letters, digits and '_', not starting with a digit"
    );
    Ok(())
}

/// Environment variable values cannot hold NUL on any platform.
pub fn check_secret_value(name: &str, v: &str) -> Result<()> {
    ensure!(
        !v.contains('\0'),
        "the value for {name} contains a NUL byte, which no environment variable can hold"
    );
    Ok(())
}

/// Environments that hold production secrets, any name starting with `prod`, need --allow-prod to be read.
pub fn is_prod(env: &str) -> bool {
    env.starts_with("prod")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_names() {
        for ok in ["ran-ai", "dev", "prod", "a", "x_1", "9lives"] {
            check_path_name("project", ok).unwrap();
        }
        for bad in [
            "", "..", ".", "../x", "a/b", "a\\b", "Dev", "-x", "_x", "a.b", "con", "nul", "com1", "lpt9", "c:",
        ] {
            assert!(check_path_name("project", bad).is_err(), "{bad:?} accepted");
        }
        assert!(check_path_name("project", &"a".repeat(65)).is_err());
        check_path_name("project", "console").unwrap();
    }

    #[test]
    fn secret_names() {
        for ok in ["A", "_A", "NEXT_PUBLIC_URL", "a1"] {
            check_secret_name(ok).unwrap();
        }
        for bad in ["", "1A", "A-B", "A B", "A=B", "A.B", "É"] {
            assert!(check_secret_name(bad).is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn every_prod_prefixed_environment_is_gated() {
        for env in ["prod", "production", "prod-eu", "prod2", "prod_old"] {
            assert!(is_prod(env), "{env} not gated");
        }
        for env in ["dev", "staging", "test", "preprod"] {
            assert!(!is_prod(env), "{env} gated");
        }
    }
}
