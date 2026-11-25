# Contributing to storedoc-rust

Thank you for contributing to `storedoc-rust`!

## Dependency & Code Rules

1. **Minimal & Audited Dependencies:** Avoid adding heavy or unmaintained crates. Prefer `rustls` over native OpenSSL so Tauri cross-compilation stays clean.
2. **No `unsafe` Code:** Keep the crate 100% safe Rust.
3. **Security First:**
   - Never log or serialize `secret_key`, `token`, or raw `Authorization` / `X-Api-Key` headers.
   - Use constant-time comparison for signatures.
   - Enforce HTTPS on remote URLs and prevent path traversal on object paths.
4. **Quality Gates:** Before opening a PR, run:
   ```bash
   cargo fmt --check
   cargo clippy --all-targets -- -D warnings
   cargo test
   ```
