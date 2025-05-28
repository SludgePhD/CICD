A simple CI/CD tool for Rust crates.

Based on <https://github.com/rust-analyzer/smol_str/blob/master/.github/ci.rs>.

Note that this is intended for personal use. The documentation below is because I forget.

## Operation and Features

`sludge-cicd` will:

- Build and run tests in the workspace.
  - With `RUSTFLAGS=-Dwarnings` by default.
- Build documentation.
  - With `RUSTDOCFLAGS=-Dwarnings` by default.
- If the current branch is named `"main"` and a `CRATES_IO_TOKEN` is configured:
  - Validate `CHANGELOG.md` completeness:
    - A `CHANGELOG.md` next to a non-workspace `Cargo.toml` has to contain a heading for the version we're about to publish.
    - A workspace-level `CHANGELOG.md` has to either contain an entry for every package we're about to publish, or an entry for the version all packages share.
  - Check `git tag --list` and `Cargo.toml` to figure out which packages in the workspace need to be published.
    - A tag like `v0.1.0` indicates to the tool that all crates in the workspace have been published at version 0.1.0.
    - A tag like `package-v0.1.0` indicates that `package` has been published at version 0.1.0.
    - Workspace inheritance is supported, so package versions can be stored in the workspace manifest too.
  - Build a topologically sorted list of packages to get a valid publish order.
  - Publish all packages identified previously.
  - Create git tags for the release and push them.
    - If all packages in the workspace are at the same version, and there is no git tag that ends in that version, and there is only a single top-level `CHANGELOG.md` (or none) a single `vX.Y.Z` tag will be created.
    - Otherwise, a `package-vX.Y.Z` tag will be created.
  - Create GitHub releases for all tags.
    - The release description will contain release notes extracted from the `CHANGELOG.md`, if any.

## Usage

`sludge-cicd <args...>`

The `<args...>` arguments are passed to any `cargo check`, `cargo build` and `cargo test` invocations.

Variable | Meaning
---------|--------
`GITHUB_TOKEN`    | GitHub Actions token. Needs to have `contents: write` permission if auto-publishing is used (for pushing tags and creating releases). Hidden from all subprocesses invoked, except those that require it.
`CRATES_IO_TOKEN` | Token for auto-publishing new versions to crates.io. If absent, auto-publishing is disabled. The token is hidden from any subprocesses invoked.
`CICD_CHECK_ONLY` | If set to any value, only `cargo check` is run for CI instead of running tests.
`CICD_SKIP_DOCS`  | If set to any value, `cargo doc` will not be run to check documentation.
`CICD_SUDO`       | If set to any value, tests will be executed (but not built) using `sudo`. OS must be configured to allow passwordless `sudo`.
