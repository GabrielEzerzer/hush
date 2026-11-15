# Tests for hush's layer rules in rules/hush-layers.yml: each rule fires on a bad sample and not on a good one.
import unittest

from test_rules import scan_matches, setUpModule


def fired(files):
    return sorted((m["file"].replace("\\", "/").removeprefix("./"), m["ruleId"]) for m in scan_matches(files)
                  if m["ruleId"].startswith("hush-"))


class Layers(unittest.TestCase):
    def check(self, rule, bad, good):
        for name, text in bad.items():
            with self.subTest(rule=rule, bad=name):
                self.assertEqual(fired({name: text}), [(name, rule)])
        for name, text in good.items():
            with self.subTest(rule=rule, good=name):
                self.assertEqual(fired({name: text}), [])

    def test_pure_modules_do_no_io(self):
        rule = "hush-pure-modules-do-no-io"
        self.check(rule, {
            "src/crypto.rs": "use std::fs;\n",
            "src/names.rs": "fn f() -> bool { std::env::var_os(\"X\").is_some() }\n",
            "src/style.rs": "use std::{fmt, io};\n",
        }, {
            "src/crypto.rs": "use std::collections::BTreeMap;\n",
            "src/vault.rs": "use std::fs;\nfn f() { let _ = std::env::var(\"X\"); }\n",
        })
        self.check(rule, {"src/names.rs": "fn f() { let _ = tempfile::tempdir(); }\n"}, {})
        for name, text in [("src/names.rs", "use tempfile::*;\n"), ("src/style.rs", "use which as w;\n"),
                           ("src/crypto.rs", "extern crate dotenvy;\n"), ("src/crypto.rs", "use std::*;\n"),
                           ("src/names.rs", "use std as s;\n"), ("src/style.rs", "use std::{self as s};\n")]:
            self.check(rule, {name: text}, {})
        self.check(rule, {}, {"src/crypto.rs": "use std::collections::*;\nuse std::fmt::{self, Write as _};\n"})
        for name, text in [("src/crypto.rs", "use std::{collections::{BTreeMap}, fs};\n"),
                           ("src/names.rs", "use std::{collections::BTreeMap, fmt::{self}, io::Write};\n"),
                           ("src/style.rs", "use std::{fmt::{Write}, *};\n"), ("src/crypto.rs", "use ::std::{{os}};\n"),
                           ("src/names.rs", "use std::{fmt::{self}, self as s};\n"),
                           ("src/style.rs", "use std::{fmt::{Write}, io::*};\n")]:
            self.check(rule, {name: text}, {})
        self.check(rule, {}, {"src/crypto.rs": "use std::{collections::*, fmt};\nuse std::{fmt::{self, Write as io}};\n"})

    def test_leaf_modules_import_nothing_from_the_crate(self):
        self.check("hush-leaf-modules-import-nothing-from-the-crate", {
            "src/config.rs": "use crate::vault::Secrets;\n",
            "src/crypto.rs": "fn f() { crate::names::is_prod(\"x\"); }\n",
        }, {
            "src/config.rs": "pub(crate) fn f() {}\n",
            "src/main.rs": "use crate::vault::Secrets;\n",
        })
        self.check("hush-leaf-modules-import-nothing-from-the-crate", {
            "src/crypto.rs": "use super::names;\n",
            "src/config.rs": "fn f() { super::vault::home(); }\n",
            "src/names.rs": "mod tests { use super::super::vault; }\n",
        }, {
            "src/crypto.rs": "mod tests { use super::*; }\n",
            "src/config.rs": "pub(super) fn f() {}\n",
            "src/names.rs": "extern crate core;\n",
        })

    def test_vault_imports_only_crypto_and_names(self):
        self.check("hush-vault-imports-only-crypto-and-names", {
            "src/vault.rs": "use crate::run::run;\n",
        }, {
            "src/vault.rs": "use crate::crypto::{self, MasterKey};\nuse crate::names::check_path_name;\n",
        })
        self.check("hush-vault-imports-only-crypto-and-names", {
            "src/vault.rs": "use crate::{crypto, config};\n",
        }, {"src/vault.rs": "use crate::{crypto, names};\n"})
        for text in ["use super::run;\n", "fn f() { super::config::load(); }\n", "use crate::*;\n",
                     "fn f() -> crate::Cli { todo!() }\n", "use crate::{crypto, config as c};\n",
                     "use crate::{names, run::*};\n", "mod keychain { use super::super::run; }\n",
                     "use crate as c;\n", "use crate::{self as c};\n"]:
            self.check("hush-vault-imports-only-crypto-and-names", {"src/vault.rs": text}, {})
        for text in ["use self::super::run;\n", "fn f() { self::super::run::run(); }\n"]:
            self.check("hush-vault-imports-only-crypto-and-names", {"src/vault.rs": text}, {})
        for text in ["use self::super::crypto;\n", "mod keychain { use self::super::names; use self::super::Vault; }\n"]:
            self.check("hush-vault-imports-only-crypto-and-names", {}, {"src/vault.rs": text})
        for text in ["use super::crypto::{self, MasterKey};\n", "mod tests { use super::*; }\n",
                     "mod keychain { use super::*; fn f() -> crate::crypto::MasterKey { todo!() } }\n"]:
            self.check("hush-vault-imports-only-crypto-and-names", {}, {"src/vault.rs": text})

    def test_run_imports_only_vault(self):
        self.check("hush-run-imports-only-vault", {
            "src/run.rs": "use crate::crypto;\n",
        }, {
            "src/run.rs": "use crate::vault::Secrets;\n",
        })
        for text in ["use super::crypto;\n", "fn f() { super::crypto::new_key(); }\n", "use crate::*;\n",
                     "use super as s;\n"]:
            self.check("hush-run-imports-only-vault", {"src/run.rs": text}, {})
        for text in ["use super::vault::Secrets;\n", "use crate::{self, vault as v};\n"]:
            self.check("hush-run-imports-only-vault", {}, {"src/run.rs": text})

    def test_only_crypto_touches_the_cipher(self):
        self.check("hush-only-crypto-touches-the-cipher", {
            "src/main.rs": "use sha2::Sha256;\n",
            "tests/cli.rs": "fn f() { let _ = chacha20poly1305::XChaCha20Poly1305::new_from_slice(&[0; 32]); }\n",
        }, {
            "src/crypto.rs": "use sha2::{Digest, Sha256};\nuse chacha20poly1305::aead::OsRng;\n",
        })
        for text in ["use sha2::*;\n", "use chacha20poly1305 as c;\n", "extern crate sha2 as s;\n", "use {rand};\n"]:
            self.check("hush-only-crypto-touches-the-cipher", {"src/main.rs": text}, {})

    def test_only_vault_touches_the_keychain(self):
        self.check("hush-only-vault-touches-the-keychain", {
            "src/main.rs": "fn f() { let _ = keyring::Entry::new(\"a\", \"b\"); }\n",
        }, {
            "src/vault.rs": "fn f() { let _ = keyring::Entry::new(\"a\", \"b\"); let _ = dirs::data_dir(); }\n",
        })
        for text in ["use keyring::*;\n", "use keyring as k;\n", "use dirs::*;\n"]:
            self.check("hush-only-vault-touches-the-keychain", {"src/main.rs": text}, {})

    def test_only_main_talks_to_the_user(self):
        self.check("hush-only-main-talks-to-the-user", {
            "src/vault.rs": "fn f() { println!(\"x\"); }\n",
            "src/run.rs": "use anstream::eprintln;\n",
        }, {
            "src/main.rs": "fn f() { eprintln!(\"x\"); }\n",
            "src/vault.rs": "fn f() -> String { format!(\"x\") }\n",
        })
        self.check("hush-only-main-talks-to-the-user", {
            "src/vault.rs": "use std::io::Write;\nfn f() { std::io::stderr().write_all(b\"x\").unwrap(); }\n",
            "src/run.rs": "use std::io::{self, Write};\nfn f() { writeln!(io::stdout(), \"x\").unwrap(); }\n",
            "src/config.rs": "use std::io::{stdin as input, Read};\n",
        }, {
            "src/run.rs": "use std::process::{Command, Stdio};\n"
                          "fn f(c: &mut Command) { c.stdin(Stdio::null()).stdout(Stdio::inherit()).stderr(Stdio::piped()); }\n",
            "src/config.rs": "fn f(out: std::process::Output) -> anyhow::Result<()> "
                             "{ anyhow::bail!(\"{}\", String::from_utf8_lossy(&out.stderr)) }\n",
        })
        self.check("hush-only-main-talks-to-the-user", {"src/run.rs": "use anstream::*;\n"}, {})
        for text in ["use std::io::*;\nfn f() { stdout().write_all(b\"x\").unwrap(); }\n",
                     "use std::io::{self, *};\nfn f() { let _ = stderr(); }\n",
                     "fn f() { std::println!(\"x\"); }\n", "fn f() { ::std::eprintln!(\"x\"); }\n"]:
            self.check("hush-only-main-talks-to-the-user", {"src/vault.rs": text}, {})
        self.check("hush-only-main-talks-to-the-user", {}, {
            "src/config.rs": "fn f(out: &std::process::Output) -> PathBuf "
                             "{ PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()) }\n",
            "src/vault.rs": "fn f(out: std::process::Output) { anyhow::bail!(\"{:?}\", out . stdin); }\n",
        })

    def test_clap_stays_in_the_cli(self):
        self.check("hush-clap-stays-in-the-cli", {
            "src/vault.rs": "use clap::Parser;\n",
            "src/run.rs": "use clap::*;\n",
        }, {
            "src/main.rs": "use clap::Parser;\n",
            "src/style.rs": "pub fn clap() -> clap::builder::Styles { clap::builder::Styles::styled() }\n",
        })

    def test_harness_evals_stay_independent(self):
        self.check("hush-harness-evals-stay-independent", {
            "harness/evals/test/acceptance_cases.py": "from behaviors import positional_value\n",
            "harness/acceptance.py": "import console_delete\n",
        }, {
            "harness/evals/dev/behaviors.py": "from acceptance import require, scenario\nimport json\n",
        })


if __name__ == "__main__":
    unittest.main()
