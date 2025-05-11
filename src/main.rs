#[cfg(test)]
#[macro_use]
mod tests;
mod toml;

use std::{
    env, fmt, fs,
    path::PathBuf,
    process::{self, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

use toml::{Toml, Value};

type Error = Box<dyn std::error::Error>;
type Result<T> = std::result::Result<T, Error>;

fn main() {
    if let Err(err) = try_main() {
        eprintln!("{}", err);
        process::exit(1);
    }
}

fn try_main() -> Result<()> {
    let cwd = env::current_dir()?;
    let args = env::args().skip(1).collect::<Vec<_>>();
    let crates_io_token = match env::var("CRATES_IO_TOKEN") {
        Ok(s) if s.is_empty() => None,
        Ok(s) => Some(s),
        Err(env::VarError::NotPresent) => None,
        Err(e @ env::VarError::NotUnicode(_)) => return Err(e.into()),
    };
    let check_only = env::var_os("CICD_CHECK_ONLY").is_some();
    let skip_docs = env::var_os("CICD_SKIP_DOCS").is_some();

    Params {
        cwd,
        args,
        crates_io_token,
        check_only,
        skip_docs,
        mock_output: None,
    }
    .run_cicd_pipeline()
}

struct Params {
    cwd: PathBuf,
    args: Vec<String>,
    crates_io_token: Option<String>,
    check_only: bool,
    skip_docs: bool,
    mock_output: Option<Vec<(&'static str, String)>>,
}

impl Params {
    fn shell_output(&mut self, cmd: &str) -> Result<String> {
        match &mut self.mock_output {
            Some(output) => {
                if output.is_empty() {
                    panic!("missing entry for '{cmd}' in mock output list");
                }
                let (expected_cmd, output) = output.remove(0);
                assert_eq!(expected_cmd, cmd);
                Ok(output)
            }
            None => {
                let output = command(cmd).stderr(Stdio::inherit()).output()?;
                check_status(output.status)?;
                let res = String::from_utf8(output.stdout)?;
                let res = res.trim().to_string();
                Ok(res)
            }
        }
    }

    fn run_cicd_pipeline(mut self) -> Result<()> {
        let args = self.args.join(" ");
        let cargo_toml = self.cwd.join("Cargo.toml");
        assert!(
            cargo_toml.exists(),
            "Cargo.toml not found, cwd: {}",
            self.cwd.display()
        );

        if self.check_only {
            let _s = Section::new("CHECK");
            shell(&format!("cargo check --workspace {args}"))?;
        } else {
            let _s = Section::new("BUILD");
            shell(&format!("cargo test --workspace --no-run {args}"))?;
        }

        if !self.skip_docs {
            let _s = Section::new("BUILD_DOCS");
            shell("cargo doc --workspace")?;
        }

        if !self.check_only {
            let _s = Section::new("TEST");
            shell(&format!("cargo test --workspace {args}"))?;
        }

        let current_branch = self.shell_output("git branch --show-current")?;
        if &current_branch == "main" {
            let _s = Section::new("PUBLISH");

            let Some(token) = self.crates_io_token.clone() else {
                eprintln!("no `CRATES_IO_TOKEN` set, skipping autopublish step");
                return Ok(());
            };

            let tags_string = self.shell_output("git tag --list")?;
            let tags = tags_string.split_whitespace().collect::<Vec<_>>();
            println!("existing git tags: {tags:?}");

            let packages = find_packages(self.cwd)?;
            assert!(!packages.is_empty());
            println!("publishable packages in workspace: {:?}", packages);

            let same_version = packages
                .iter()
                .all(|pkg| pkg.version == packages[0].version);
            let separate_tags =
                !same_version || tags.iter().any(|tag| tag.ends_with(&packages[0].version));

            let to_publish = packages
                .iter()
                .filter(|Package { name, version }| {
                    !tags.contains(&&*format!("v{version}"))
                        && !tags.contains(&&*format!("{name}-v{version}"))
                })
                .collect::<Vec<_>>();

            eprintln!(
                "{} package{} need{} publishing: {:?}",
                to_publish.len(),
                if to_publish.len() > 2 { "s" } else { "" },
                if to_publish.len() == 1 { "s" } else { "" },
                to_publish
            );

            for Package { name, version } in &to_publish {
                // If there is neither a `$package-v$version` tag, nor a `v$version` tag, the package
                // should be published.
                // If all publishable packages are at the same version, and no tag that ends in that
                // version exists, we'll use a single collective `v$version` tag for all packages.

                // NB: we use `--no-verify` because we might build the workspace crates out of
                // order, so a dependency might not be on crates.io when its dependents are
                // verified. This isn't easily fixable without pulling in dependencies and getting
                // the package graph somehow.
                eprintln!("publishing {name}@{version}");
                shell(&format!(
                    "cargo publish --no-verify -p {name} --token {token}"
                ))?;
            }

            if !to_publish.is_empty() {
                if separate_tags {
                    for Package { name, version } in &to_publish {
                        let tag = format!("{name}-v{version}");
                        shell(&format!("git tag {tag}"))?;
                    }
                } else {
                    let version = &to_publish[0].version;
                    shell(&format!("git tag v{version}"))?;
                }

                shell("git push --tags")?;
            }
        }
        Ok(())
    }
}

struct Package {
    name: String,
    version: String,
}

impl fmt::Display for Package {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.version)
    }
}
impl fmt::Debug for Package {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.version)
    }
}

fn find_packages(cwd: PathBuf) -> Result<Vec<Package>> {
    fn recurse(
        dir: PathBuf,
        out: &mut Vec<Package>,
        workspace_version: Option<&str>,
    ) -> Result<()> {
        let mut toml = dir.clone();
        toml.push("Cargo.toml");
        if toml.exists() {
            let manifest = fs::read_to_string(&toml)?;
            let toml = Toml(&manifest);
            // Filter out virtual manifests, those with `publish = false` set, and those that lack a
            // `version` field.
            if manifest.contains("[package]")
                && !matches!(toml.get_field("publish"), Ok(Value::Bool(false)))
                && (toml.get_field("version").is_ok()
                    || toml.get_field("version.workspace").is_ok())
            {
                let name = toml
                    .get_field("name")?
                    .as_str()
                    .ok_or("package name is not a string")?
                    .to_string();
                let version = match toml.get_field("version") {
                    Ok(version) => version
                        .as_str()
                        .ok_or("version is not a string")?
                        .to_string(),
                    Err(e) => match workspace_version {
                        Some(version) => version.to_string(),
                        None => return Err(e),
                    },
                };

                out.push(Package { name, version });
            }
        }

        let is_test_dir = dir
            .to_str()
            .expect("non-UTF-8 path")
            .ends_with("sludge-cicd-test-projects");

        if !is_test_dir {
            for entry in fs::read_dir(&dir)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    recurse(entry.path(), out, workspace_version)?;
                }
            }
        }
        Ok(())
    }

    if !cwd.join("Cargo.toml").exists() {
        return Err("`Cargo.toml` does not exist in the project directory".into());
    }

    let workspace_version = workspace_version(cwd.clone()).ok();
    let mut out = Vec::new();
    recurse(cwd, &mut out, workspace_version.as_deref())?;

    // This list depends on iteration order, so sort it for portability in the tests.
    out.sort_by_key(|pkg| pkg.name.clone());

    Ok(out)
}

fn workspace_version(cwd: PathBuf) -> Result<String> {
    let mut path = cwd;
    path.push("Cargo.toml");
    let manifest = fs::read_to_string(path)?;
    if manifest.contains("[workspace]") {
        let version = Toml(&manifest)
            .get_field("package.version")?
            .as_str()
            .ok_or("version is not a string")?;
        Ok(version.to_string())
    } else {
        Err("no workspace".into())
    }
}

fn shell(cmd: &str) -> Result<()> {
    eprintln!("> {}", cmd.trim());
    assert!(
        !cmd.contains('"'),
        "quoting and escaping command-line arguments is not supported"
    );
    if cfg!(test) {
        Ok(())
    } else {
        let status = command(cmd).status()?;
        check_status(status)
    }
}

fn command(cmd: &str) -> Command {
    let words = cmd
        .split_ascii_whitespace()
        .filter(|arg| !arg.trim().is_empty())
        .collect::<Vec<_>>();
    let (program, args) = words.split_first().unwrap();
    let mut command = Command::new(program);
    command.args(args);
    setup_environment(program, &mut command);
    command
}

fn setup_environment(program: &str, cmd: &mut Command) {
    // Remove the crates.io token so that tests and build scripts can't read it. It is explicitly
    // passed to Cargo via `--token` when needed.
    cmd.env_remove("CRATES_IO_TOKEN");

    // Only `git` needs the `GITHUB_TOKEN` for pushing tags (which needs to be configured with
    // `contents: write` permission).
    if program != "git" {
        cmd.env_remove("GITHUB_TOKEN");
    }

    match program {
        "cargo" => {
            let rustflags = env::var_os("RUSTFLAGS").unwrap_or("-D warnings".into());
            let rustdocflags = env::var_os("RUSTDOCFLAGS").unwrap_or("-D warnings".into());
            let rust_backtrace = env::var_os("RUST_BACKTRACE").unwrap_or("short".into());
            cmd.env("CI", "1")
                .env("CARGO_NET_RETRY", "10") // CI environment networking may be unreliable
                .env("CARGO_INCREMENTAL", "0") // Incremental builds are slower and not needed here
                .env("RUSTFLAGS", rustflags)
                .env("RUSTDOCFLAGS", rustdocflags)
                .env("RUST_BACKTRACE", rust_backtrace);
        }
        _ => (),
    }
}

fn check_status(status: ExitStatus) -> Result<()> {
    if !status.success() {
        Err(format!("$status: {}", status))?;
    }
    Ok(())
}

struct Section {
    name: &'static str,
    start: Instant,
}

impl Section {
    fn new(name: &'static str) -> Section {
        println!("::group::{}", name);
        let start = Instant::now();
        Section { name, start }
    }
}

impl Drop for Section {
    fn drop(&mut self) {
        let elapsed = if cfg!(test) {
            Duration::ZERO
        } else {
            self.start.elapsed()
        };
        println!("{}: {:.2?}", self.name, elapsed);
        println!("::endgroup::");
    }
}
