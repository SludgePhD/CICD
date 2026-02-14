# Changelog

## v0.1.32

- Treat packages as non-published if they use `version = …` in a dependency section, but not in `[package]`.

## v0.1.31

- Support specifying release artifacts with `ATTACHMENTS_<pkgname>`.
- Check a bunch of things on every tool run, rather than just when publishing.
- Check that all publishable packages have the metadata required by crates.io.

## v0.1.30

- Support additional Cargo.toml syntax for inheriting package versions from the workspace manifest.

## v0.1.29

- Arguments passed to `sludge-cicd` are now also passed to the `cargo doc` invocation, not just to
  `cargo test`.
- If this is undesired, `CICD_CARGO_DOC_FLAGS` can be set to override the arguments.

## v0.1.28

- Fix `sudo` logic on GitHub Actions runners.

## v0.1.27

- Correctly set environment variables when `sudo` is used.

## v0.1.26

- Allow running tests with `sudo` by setting `CICD_SUDO`.
- Include value of `PATH` in info section.

## v0.1.25

- Better error when spawning command fails.

## v0.1.24

- Print version information.
  - Includes the `git` version, the `rustc -Vv` output, the list of installed Rust toolchains,
    and the version of this CI/CD tool.

## v0.1.23

- No changes; this version is just for testing automatic GitHub releases.

## v0.1.22

- Try to fix warning emission.

## v0.1.21

- Fix how `GITHUB_TOKEN` is handled and emit a workflow warning when it's unset.

## v0.1.20

- Add support for enforcing `CHANGELOG.md` completeness, and automatically create GitHub releases from that.
