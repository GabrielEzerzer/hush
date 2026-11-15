//! Validation for every name that reaches the filesystem or a child's environment.

use anyhow::{Result, bail, ensure};

const WINDOWS_RESERVED_DEVICE_NAMES: &[&str] = &["con", "prn", "aux", "nul"];

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
    let reserved = WINDOWS_RESERVED_DEVICE_NAMES.contains(&s)
        || ((s.starts_with("com") || s.starts_with("lpt")) && s.len() == 4 && s.as_bytes()[3].is_ascii_digit());
    ensure!(
        !reserved,
        "invalid {kind} name '{s}': Windows reserves it as a device name"
    );
    Ok(())
}

pub fn check_secret_name(s: &str) -> Result<()> {
    let first = s.bytes().next().is_some_and(|b| b.is_ascii_alphabetic() || b == b'_');
    let rest = s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    ensure!(
        first && rest && s.len() <= 256,
        "invalid secret name '{s}': use letters, digits and '_', not starting with a digit"
    );
    Ok(())
}

pub fn check_case_clash<'a>(existing: impl IntoIterator<Item = &'a str>, name: &str, place: &str) -> Result<()> {
    if let Some(other) = existing
        .into_iter()
        .find(|k| *k != name && k.eq_ignore_ascii_case(name))
    {
        bail!(
            "{name} differs from {other} in {place} only by case; Windows treats them as one environment variable, so use {other} or remove it first. Nothing was stored."
        );
    }
    Ok(())
}

pub fn check_secret_value(name: &str, v: &str) -> Result<()> {
    ensure!(
        !v.contains('\0'),
        "the value for {name} contains a NUL byte, which no environment variable can hold"
    );
    Ok(())
}

pub fn is_prod(env: &str) -> bool {
    env.starts_with("prod")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_names() {
        for ok in ["web-app", "dev", "prod", "a", "x_1", "9lives"] {
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
    fn names_differing_only_in_case_clash() {
        let existing = ["db_url", "OTHER"];
        let err = check_case_clash(existing, "DB_URL", "demo/dev")
            .unwrap_err()
            .to_string();
        assert!(err.contains("differs from db_url"), "{err}");
        check_case_clash(existing, "db_url", "demo/dev").unwrap();
        check_case_clash(existing, "DB_URL2", "demo/dev").unwrap();
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

    use proptest::prelude::*;
    use proptest::test_runner::RngSeed;

    fn seeded() -> ProptestConfig {
        ProptestConfig {
            cases: 256,
            rng_seed: RngSeed::Fixed(2),
            failure_persistence: None,
            ..ProptestConfig::default()
        }
    }

    proptest! {
        #![proptest_config(seeded())]

        #[test]
        fn path_names_are_accepted_exactly_when_they_fit_the_rule(
            head in "[a-z0-9]",
            tail in "[a-z0-9_-]{0,61}",
            bad in "[^a-z0-9_-]",
            at in any::<proptest::sample::Index>(),
        ) {
            let good = format!("{head}{tail}-x");
            prop_assert!(check_path_name("project", &good).is_ok(), "{:?} refused", good);
            let mut broken: Vec<char> = good.chars().collect();
            broken.insert(at.index(broken.len() + 1), bad.chars().next().unwrap());
            let broken: String = broken.into_iter().collect();
            prop_assert!(check_path_name("project", &broken).is_err(), "{:?} accepted", broken);
            let too_long = good.clone() + &"a".repeat(65 - good.len());
            prop_assert!(check_path_name("project", &too_long).is_err());
        }

        #[test]
        fn secret_names_are_accepted_exactly_when_they_fit_the_rule(
            head in "[A-Za-z_]",
            tail in "[A-Za-z0-9_]{0,40}",
            bad in "[^A-Za-z0-9_]",
            at in any::<proptest::sample::Index>(),
        ) {
            let good = format!("{head}{tail}");
            prop_assert!(check_secret_name(&good).is_ok());
            let mut broken: Vec<char> = good.chars().collect();
            broken.insert(at.index(broken.len() + 1), bad.chars().next().unwrap());
            let broken: String = broken.into_iter().collect();
            prop_assert!(check_secret_name(&broken).is_err(), "{:?} accepted", broken);
            let leading_digit = "9".to_owned() + &good;
            prop_assert!(check_secret_name(&leading_digit).is_err());
        }

        #[test]
        fn a_name_clashes_with_its_other_casings_but_never_with_itself(
            name in "[A-Za-z_][A-Za-z0-9_]{0,20}",
            flips in proptest::collection::vec(any::<bool>(), 21),
            others in proptest::collection::vec("[A-Za-z_][A-Za-z0-9_]{0,20}", 0..5),
        ) {
            let recased: String = name
                .chars()
                .zip(&flips)
                .map(|(c, flip)| if *flip { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() })
                .collect();
            let unrelated: Vec<&str> = others.iter().map(String::as_str).filter(|o| !o.eq_ignore_ascii_case(&name)).collect();
            let mut existing = unrelated.clone();
            existing.push(&name);
            prop_assert!(check_case_clash(existing.iter().copied(), &name, "p/e").is_ok());
            prop_assert!(check_case_clash(unrelated.iter().copied(), &recased, "p/e").is_ok());
            prop_assert_eq!(check_case_clash(existing.iter().copied(), &recased, "p/e").is_err(), recased != name);
            existing.reverse();
            prop_assert_eq!(check_case_clash(existing.iter().copied(), &recased, "p/e").is_err(), recased != name);
        }
    }
}
