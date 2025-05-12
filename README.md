A simple CI/CD tool for Rust crates.

Based on <https://github.com/rust-analyzer/smol_str/blob/master/.github/ci.rs>.

## Operation and Features

`sludge-cicd` will:

- Build and run tests in the workspace.
  - With `RUSTFLAGS=-Dwarnings` by default.
- Build documentation.
  - With `RUSTDOCFLAGS=-Dwarnings` by default.
- If the current branch is named `"main"` and a `CRATES_IO_TOKEN` is configured:
  - Check `git tag --list` and `Cargo.toml` to figure out which packages in the workspace need to be published.
    - A tag like `v0.1.0` indicates to the tool that all crates in the workspace have been published at version 0.1.0.
    - A tag like `package-v0.1.0` indicates that `package` has been published at version 0.1.0.
    - Workspace inheritance is supported, so package versions can be stored in the workspace manifest too.
  - Build a topologically sorted list of packages to get a valid publish order.
  - Publish all packages identified previously.
  - Create git tags for the release and push them.
    - If all packages in the workspace are at the same version, and there is no git tag that ends in that version, a single `vX.Y.Z` tag will be created.
    - Otherwise, a `package-vX.Y.Z` tag will be created.

## Usage

`sludge-cicd <args...>`

The `<args...>` arguments are passed to any `cargo check`, `cargo build` and `cargo test` invocations.

Variable | Meaning
---------|--------
`CRATES_IO_TOKEN` | Token for auto-publishing new versions to crates.io. If absent, auto-publishing is disabled.
`CICD_CHECK_ONLY` | If set to any value, only `cargo check` is run for CI instead of running tests.
`CICD_SKIP_DOCS`  | If set to any value, `cargo doc` will not be run to check documentation.
