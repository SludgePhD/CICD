A simple CI/CD tool for Rust crates.

Based on <https://github.com/rust-analyzer/smol_str/blob/master/.github/ci.rs>.

## Usage

`sludge-cicd <args...>`

The `<args...>` arguments are passed to any `cargo check`, `cargo build` and `cargo test` invocations.

Variable | Meaning
---------|--------
`CRATES_IO_TOKEN` | Token for auto-publishing new versions to crates.io. If absent, auto-publishing is disabled.
`CICD_CHECK_ONLY` | If set to any value, only `cargo check` is run for CI instead of running tests.
`CICD_SKIP_DOCS`  | If set to any value, `cargo doc` will not be run to check documentation.
