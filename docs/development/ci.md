# CI and Verification

GitHub Actions workflow [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml) runs on every pull request and push to `main`.

## Local Checks

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
python3 scripts/check-doc-links.py
```

Report the exact commit SHA and commands executed; never claim a skipped check passed.
