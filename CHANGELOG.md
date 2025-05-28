# Changelog

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
