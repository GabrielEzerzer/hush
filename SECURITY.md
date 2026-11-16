# Security policy

hush is a personal project. It has not been audited and has not had a professional security review. Don't rely on it for high-stakes secrets. The README's section "What hush does not protect against" lists the known limits.

## Reporting a vulnerability

Please report vulnerabilities privately through GitHub's private vulnerability reporting: open the repository's **Security** tab and choose **Report a vulnerability**. Don't open a public issue for a security problem.

Include the version or commit, your platform, and the steps to reproduce. Don't include real secrets in the report.

This is maintained by one person in their spare time, so there is no guaranteed response time. Fixes go into the latest release only.

## Design notes

The security properties the code relies on, for reviewers. The README covers what hush does and does not protect against.

- **File format.** Each `<env>.hush` file is `HUSH1`, a 24-byte nonce drawn fresh from the OS random generator for every write, then the XChaCha20-Poly1305 ciphertext and tag. The additional data is `hush/v1/<project>/<env>`, so a file copied over another project's or environment's file fails to open.
- **Vault layout.** `vault.toml` holds the format version and the key's public fingerprint. Secrets live in `projects/<project>/<env>.hush`, and `projects/<project>/.lock` is held while a write is in progress.
- **Key fingerprint.** The fingerprint is the first 8 bytes of SHA-256 over `hush key id v1` and the key, in hex. Every command that reads or writes secrets checks it first, as do `setup` and `recovery-key`, so a wrong key fails with a clear message before any secrets file is touched. `init` and the `project` commands never load the key.
- **Recovery form.** The key prints as 64 hex digits in eight dash-separated groups of eight. Dashes and whitespace are ignored when it is read back.
- **Key in memory.** The key, decrypted file contents and values read from prompts, stdin or `.env` files sit in buffers that are zeroised when dropped. The values in the decrypted secrets map are plain strings and are not ([#15](https://github.com/GabrielEzerzer/hush/issues/15)).
- **Empty variables.** A set but empty `HUSH_HOME` or `HUSH_KEY` is an error, never a fallback, so a variable that expanded to nothing can't point a test or CI run at the real vault or keychain.
- **Folder permissions.** On Unix the vault folders are created with mode 0700. On Windows they inherit the per-user ACL of `%APPDATA%`.
- **Setup.** Setup never overwrites a key already in the keychain, and checks a recovery key against `vault.toml` before storing it. When `vault.toml` is missing but `.hush` files exist, it refuses to mint a new key, since that would tie them to a key they were not sealed with; only a recovery key may proceed. It reads the key back from the keychain after storing it, because a keychain that accepts a key but can't return it would lose every secret.
- **Writes.** `vault.toml` is written to a temporary file and moved into place only if none exists, so a crash never leaves a partial file and a concurrent setup never overwrites one. An environment file is read, changed and rewritten under the project lock, and the new file replaces the old one by rename, so a crash leaves the old or the new version, never a partial one. If the change fails, nothing is written. `hush project delete` does not take the lock ([#14](https://github.com/GabrielEzerzer/hush/issues/14)).
- **Error messages.** Parse errors from the decrypted JSON and from `.env` files report only the file name and line, because the parsers' own messages can quote a value.
- **Import and the environment.** Before parsing, `hush import` resolves everything it needs from the environment (`HUSH_HOME`, `HUSH_KEY`, `HUSH_PROJECT`, `HUSH_ENV`) and then empties its own environment, so a `$` reference the scanner misses expands to nothing. Removing the variables is sound because hush starts no threads before that point; its only thread is the Ctrl+C handler in `hush run`. On macOS the keychain call may leave system threads behind; they are not known to read the environment.
