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

    def test_leaf_modules_import_nothing_from_the_crate(self):
        self.check("hush-leaf-modules-import-nothing-from-the-crate", {
            "src/config.rs": "use crate::vault::Secrets;\n",
            "src/crypto.rs": "fn f() { crate::names::is_prod(\"x\"); }\n",
        }, {
            "src/config.rs": "pub(crate) fn f() {}\n",
            "src/main.rs": "use crate::vault::Secrets;\n",
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

    def test_run_imports_only_vault(self):
        self.check("hush-run-imports-only-vault", {
            "src/run.rs": "use crate::crypto;\n",
        }, {
            "src/run.rs": "use crate::vault::Secrets;\n",
        })

    def test_only_crypto_touches_the_cipher(self):
        self.check("hush-only-crypto-touches-the-cipher", {
            "src/main.rs": "use sha2::Sha256;\n",
            "tests/cli.rs": "fn f() { let _ = chacha20poly1305::XChaCha20Poly1305::new_from_slice(&[0; 32]); }\n",
        }, {
            "src/crypto.rs": "use sha2::{Digest, Sha256};\nuse chacha20poly1305::aead::OsRng;\n",
        })

    def test_only_vault_touches_the_keychain(self):
        self.check("hush-only-vault-touches-the-keychain", {
            "src/main.rs": "fn f() { let _ = keyring::Entry::new(\"a\", \"b\"); }\n",
        }, {
            "src/vault.rs": "fn f() { let _ = keyring::Entry::new(\"a\", \"b\"); let _ = dirs::data_dir(); }\n",
        })

    def test_only_main_talks_to_the_user(self):
        self.check("hush-only-main-talks-to-the-user", {
            "src/vault.rs": "fn f() { println!(\"x\"); }\n",
            "src/run.rs": "use anstream::eprintln;\n",
        }, {
            "src/main.rs": "fn f() { eprintln!(\"x\"); }\n",
            "src/vault.rs": "fn f() -> String { format!(\"x\") }\n",
        })

    def test_clap_stays_in_the_cli(self):
        self.check("hush-clap-stays-in-the-cli", {
            "src/vault.rs": "use clap::Parser;\n",
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
