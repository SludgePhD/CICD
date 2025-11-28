#[cfg(test)]
#[macro_use]
mod tests;
mod markdown;
mod toml;
mod utils;

use std::{
    env::{self, VarError},
    fmt::{self, Write as _},
    fs,
    io::{self, stdout, Write as _},
    path::PathBuf,
    process::{self, Command, ExitStatus, Stdio},
    str,
    time::{Duration, Instant},
};

use markdown::Markdown;
use toml::{Toml, Value};

type Error = Box<dyn std::error::Error>;
type Result<T> = std::result::Result<T, Error>;

macro_rules! bail {
    ($($args:tt)+) => {
        return Err(format!($($args)+).into())
    };
}

fn main() {
    if let Err(err) = try_main() {
        eprintln!("{}", err);
        process::exit(1);
    }
}

fn try_main() -> Result<()> {
    let cwd = env::current_dir()?;
    let args = env::args().skip(1).collect::<Vec<_>>().join(" ");
    let crates_io_token = match env::var("CRATES_IO_TOKEN") {
        Ok(s) if s.is_empty() => None,
        Ok(s) => Some(s),
        Err(env::VarError::NotPresent) => None,
        Err(e @ env::VarError::NotUnicode(_)) => return Err(e.into()),
    };
    let github_token = match env::var("GITHUB_TOKEN") {
        Ok(s) if s.is_empty() => None,
        Ok(s) => Some(s),
        Err(env::VarError::NotPresent) => None,
        Err(e @ env::VarError::NotUnicode(_)) => return Err(e.into()),
    };
    let check_only = env::var_os("CICD_CHECK_ONLY").is_some();
    let skip_docs = env::var_os("CICD_SKIP_DOCS").is_some();
    let sudo = env::var_os("CICD_SUDO").is_some();
    let commit = env::var("GITHUB_SHA")?;
    let cargo_doc_flags = match env::var("CICD_CARGO_DOC_FLAGS") {
        Ok(s) => s,
        Err(VarError::NotPresent) => args.clone(),
        Err(e @ VarError::NotUnicode(_)) => return Err(e.into()),
    };

    let params = Params {
        cwd,
        args,
        crates_io_token,
        github_token,
        commit,
        cargo_doc_flags,
        check_only,
        skip_docs,
        sudo,
        mock_output: None,
    };
    Pipeline::new(params)?.run()
}

struct Params {
    cwd: PathBuf,
    args: String,
    crates_io_token: Option<String>,
    github_token: Option<String>,
    commit: String,
    cargo_doc_flags: String,
    check_only: bool,
    skip_docs: bool,
    sudo: bool,
    mock_output: Option<Vec<(&'static str, String)>>,
}

struct Pipeline {
    params: Params,
    packages: Vec<Package>,
}

impl Pipeline {
    fn new(params: Params) -> Result<Self> {
        let _s = Section::new("INIT");

        let workspace = Workspace::get(params.cwd.clone())?;
        let mut packages = workspace.find_packages()?;

        extract_release_notes(&mut packages, &workspace)?;

        Ok(Self { params, packages })
    }

    fn run(mut self) -> Result<()> {
        self.step_info()?;
        self.step_test()?;
        self.step_gitcheck()?;
        self.step_manifest_check()?;
        self.step_publish()?;
        Ok(())
    }

    fn step_info(&mut self) -> Result<()> {
        if self.params.is_mock_test() {
            // This is useless to mock and clutters up the test data.
            return Ok(());
        }

        let _s = Section::new("INFO");
        println!(concat!(
            env!("CARGO_PKG_NAME"),
            " version ",
            env!("CARGO_PKG_VERSION")
        ));
        print!("PATH=");
        if let Some(path) = env::var_os("PATH") {
            stdout().write_all(path.as_encoded_bytes())?;
        }
        println!();
        shell("rustup toolchain list")?;
        shell("rustc -Vv")?;
        shell("git --version")?;
        Ok(())
    }

    fn step_test(&mut self) -> Result<()> {
        let args = &self.params.args;
        let cargo_toml = self.params.cwd.join("Cargo.toml");
        assert!(
            cargo_toml.exists(),
            "Cargo.toml not found, cwd: {}",
            self.params.cwd.display()
        );

        if self.params.check_only {
            let _s = Section::new("CHECK");
            shell(&format!("cargo check --workspace {args}"))?;
        } else {
            let _s = Section::new("BUILD");
            shell(&format!("cargo test --workspace --no-run {args}"))?;
        }

        if !self.params.skip_docs {
            let _s = Section::new("BUILD_DOCS");
            shell(&format!(
                "cargo doc --workspace {}",
                self.params.cargo_doc_flags
            ))?;
        }

        if !self.params.check_only {
            let _s = Section::new("TEST");
            shell_ex(
                &format!("cargo test --workspace {args}"),
                "",
                self.params.sudo,
            )?;
        }

        Ok(())
    }

    /// Checks that the repository is clean.
    ///
    /// There must be no untracked files created by running the test suite, no commits created, and
    /// no files staged.
    fn step_gitcheck(&mut self) -> Result<()> {
        // Deny untracked and changed files, and staged changes.
        let files = self.params.shell_output("git status --porcelain")?;
        if !files.trim().is_empty() {
            return Err(format!("untracked/modified files present: {files}").into());
        }

        // Ensure that HEAD is still at the expected git commit.
        let commit = self.params.shell_output("git rev-parse HEAD")?;
        let commit = commit.trim();
        if self.params.commit != commit {
            return Err(format!(
                "repo is at unexpected commit {commit} (expected {})",
                self.params.commit
            )
            .into());
        }

        Ok(())
    }

    fn step_manifest_check(&self) -> Result<()> {
        for Package { name, manifest, .. } in &self.packages {
            let toml = Toml(manifest);
            if !toml.get_field("description").is_ok()
                && !toml.get_field("description.workspace").is_ok()
            {
                bail!("package `{name}` is missing a description field");
            }

            if !toml.get_field("license").is_ok() && !toml.get_field("license.workspace").is_ok() {
                bail!("package `{name}` is missing a license field");
            }
        }

        Ok(())
    }

    fn step_publish(&mut self) -> Result<()> {
        let current_branch = self.params.shell_output("git branch --show-current")?;
        let _s = Section::new("PUBLISH");

        let tags_string = self.params.shell_output("git tag --list")?;
        let tags = tags_string.split_whitespace().collect::<Vec<_>>();
        println!("existing git tags: {tags:?}");

        if self.packages.is_empty() {
            bail!(
                "no publishable packages found in '{}'",
                self.params.cwd.display()
            );
        }
        println!("publishable packages in workspace: {:?}", self.packages);

        let same_version = self
            .packages
            .iter()
            .all(|pkg| pkg.version == self.packages[0].version);
        let has_package_specific_changelog =
            self.packages.iter().any(|pkg| pkg.changelog_path.is_some());
        let separate_tags = has_package_specific_changelog
            || !same_version
            || tags
                .iter()
                .any(|tag| tag.ends_with(&self.packages[0].version));

        let to_publish = self
            .packages
            .iter()
            .filter(|Package { name, version, .. }| {
                !tags.contains(&&*format!("v{version}"))
                    && !tags.contains(&&*format!("{name}-v{version}"))
            })
            .collect::<Vec<_>>();
        if to_publish.is_empty() {
            println!("no packages need publishing, done");
            return Ok(());
        }

        println!(
            "{} package{} need{} publishing: {:?}",
            to_publish.len(),
            if to_publish.len() != 1 { "s" } else { "" },
            if to_publish.len() == 1 { "s" } else { "" },
            to_publish
        );

        let Some(token) = self.params.crates_io_token.clone() else {
            println!("no `CRATES_IO_TOKEN` set, skipping autopublish step");
            return Ok(());
        };
        if &current_branch != "main" {
            println!("not on `main` branch, skipping autopublish step");
            return Ok(());
        }

        for Package { name, version, .. } in &to_publish {
            // If there is neither a `$package-v$version` tag, nor a `v$version` tag, the package
            // should be published.
            // If all publishable packages are at the same version, and no tag that ends in that
            // version exists, we'll use a single collective `v$version` tag for all packages.

            // NB: we use `--no-verify` because we've already tested the package earlier.
            println!("publishing {name}@{version}");
            shell(&format!(
                "cargo publish --no-verify -p {name} --token {token}"
            ))?;
        }

        if separate_tags {
            for package in &to_publish {
                let Package { name, version, .. } = package;

                let tag = format!("{name}-v{version}");
                shell(&format!("git tag {tag}"))?;
            }
        } else {
            let version = &to_publish[0].version;
            shell(&format!("git tag v{version}"))?;
        }

        shell("git push --tags")?;

        if separate_tags {
            for package in &to_publish {
                let Package {
                    name,
                    version,
                    release_notes,
                    ..
                } = package;

                let tag = format!("{name}-v{version}");
                if let Some(relnotes) = release_notes {
                    shell_with_stdin(&format!("gh release create {tag} --notes-file -"), relnotes)?;
                }
            }
        } else if to_publish.iter().any(|pkg| pkg.release_notes.is_some()) {
            // Shared tag -> Create merged release notes from all packages.
            // If multiple packages have the same relnotes, deduplicate them (likely from the
            // workspace-level CHANGELOG.md).
            let mut entries = Vec::new();
            let mut prev_notes = None;
            for package in &to_publish {
                if let Some(notes) = &package.release_notes {
                    if Some(&**notes) == prev_notes {
                        continue;
                    }
                    prev_notes = Some(&**notes);
                    entries.push(package);
                }
            }

            let mut relnotes = String::new();
            for package in &entries {
                if let Some(notes) = &package.release_notes {
                    if !relnotes.is_empty() {
                        relnotes += "\n";
                    }

                    if entries.len() > 1 {
                        writeln!(relnotes, "# {} {}", package.name, package.version).ok();
                        writeln!(relnotes).ok();
                    }
                    writeln!(relnotes, "{notes}").ok();
                }
            }

            if self.params.github_token.is_some() {
                let tag = format!("v{}", &to_publish[0].version);
                shell_with_stdin(
                    &format!("gh release create {tag} --notes-file -"),
                    &relnotes,
                )?;
            } else {
                eprintln!("::warning::`GITHUB_TOKEN` not set; cannot create GitHub release");
            }
        }

        Ok(())
    }
}

fn extract_release_notes(packages: &mut [Package], workspace: &Workspace) -> Result<()> {
    for package in packages {
        let Some(changelog_path) = package
            .changelog_path
            .as_deref()
            .or(workspace.changelog.as_deref())
        else {
            continue;
        };

        // There is a package-specific changelog. It has to contain a single heading for the
        // version we're about to release.
        let changelog = fs::read_to_string(changelog_path)?;

        let mut entries_matching_version = Vec::new();
        for level in 1..=3 {
            for (title, contents) in Markdown(&changelog).sections(level) {
                if title.contains(&*package.version) {
                    entries_matching_version.push((title, contents.0));
                }
            }
            if !entries_matching_version.is_empty() {
                break;
            }
        }

        let entry = match *entries_matching_version {
            [] => bail!(
                "changelog at '{}' does not contain an entry for {package}",
                changelog_path.display()
            ),
            [(_, contents)] => contents,
            [ref multiple @ ..] => {
                let mut entry_containing_name = None;
                for (title, contents) in multiple {
                    if title.to_ascii_lowercase().contains(&package.name) {
                        if entry_containing_name.is_some() {
                            bail!(
                                "changelog '{}' contains multiple entries for {package}",
                                changelog_path.display()
                            );
                        }
                        entry_containing_name = Some(*contents);
                    }
                }
                match entry_containing_name {
                    Some(contents) => contents,
                    None => bail!(
                        "changelog '{}' is missing an entry for {package}",
                        changelog_path.display()
                    ),
                }
            }
        };

        package.release_notes = Some(entry.to_string());
    }

    Ok(())
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

    fn is_mock_test(&self) -> bool {
        self.mock_output.is_some()
    }
}

#[derive(Clone)]
struct Package {
    name: String,
    version: String,
    /// If the package has its own changelog (separate from the top-level workspace changelog),
    /// this is the changelog's path.
    changelog_path: Option<PathBuf>,
    /// Set to the changelog contents for this package version when we're about to publish it.
    release_notes: Option<String>,
    /// Contents of the package's `Cargo.toml`.
    manifest: String,
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

#[derive(Debug)]
struct Workspace {
    cwd: PathBuf,
    version: Option<String>,
    changelog: Option<PathBuf>,
}

impl Workspace {
    fn get(cwd: PathBuf) -> Result<Self> {
        let version = match fs::read_to_string(cwd.join("Cargo.toml")) {
            Ok(manifest) => match Toml(&manifest).get_field("package.version") {
                Ok(version) => Some(
                    version
                        .as_str()
                        .ok_or("version is not a string")?
                        .to_string(),
                ),
                Err(_) => Toml(&manifest)
                    .section("workspace.package")
                    .and_then(|toml| {
                        toml.get_field("version")
                            .ok()
                            .and_then(|fld| fld.as_str().map(ToString::to_string))
                    }),
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };

        let changelog = cwd.join("CHANGELOG.md");
        let changelog = if changelog.exists() {
            Some(changelog)
        } else {
            None
        };

        Ok(Self {
            cwd,
            version,
            changelog,
        })
    }

    /// Enumerates all publishable packages in the workspace.
    ///
    /// A package is considered publishable if it does not set `publish = false` and it contains a
    /// `package.version` key.
    fn find_packages(&self) -> Result<Vec<Package>> {
        fn recurse(dir: PathBuf, out: &mut Vec<Package>, workspace: &Workspace) -> Result<()> {
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
                        Err(e) => match &workspace.version {
                            Some(version) => version.clone(),
                            None => return Err(e),
                        },
                    };

                    let mut changelog = dir.clone();
                    changelog.push("CHANGELOG.md");
                    let changelog = if changelog.exists()
                        && Some(&*changelog) != workspace.changelog.as_deref()
                    {
                        Some(changelog)
                    } else {
                        None
                    };

                    out.push(Package {
                        name,
                        version,
                        changelog_path: changelog,
                        release_notes: None,
                        manifest,
                    });
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
                        recurse(entry.path(), out, workspace)?;
                    }
                }
            }
            Ok(())
        }

        if !self.cwd.join("Cargo.toml").exists() {
            return Err("`Cargo.toml` does not exist in the project directory".into());
        }

        let mut out = Vec::new();
        recurse(self.cwd.clone(), &mut out, self)?;

        let pkgs = sort_packages(&mut out);

        Ok(pkgs)
    }
}

/// A package can only be published (even with `--no-verify`) if its dependencies are already
/// available on crates.io.
///
/// Cargo will wait until crates.io makes the package available, but we have to publish them in the
/// right order. That means topologically sorting an approximation of the dependency graph.
fn sort_packages(pkgs: &mut [Package]) -> Vec<Package> {
    if pkgs.is_empty() {
        return Vec::new();
    }

    // Start with a deterministic ordering.
    pkgs.sort_by_key(|pkg| pkg.name.clone());

    let mut depends_on = vec![vec![]; pkgs.len()];
    let mut dependants = vec![0; pkgs.len()];
    for (i, pkg) in pkgs.iter().enumerate() {
        let toml = Toml(&pkg.manifest);
        for (name, contents) in toml.sections() {
            // FIXME: allow specifications like [target.'cfg(...)'.dependencies]
            // FIXME: allow specifications like [dependencies.dep]\nversion = ...
            if !name.ends_with("dependencies") {
                continue;
            }

            for line in contents.0.lines() {
                // Dependency specifications like:
                // dep = { version = ... }
                // dep = "1.2.3"
                // dep.workspace = true
                // dep.version = "1.2.3"
                let Some((dep, _)) = line.split_once('=') else {
                    continue;
                };
                let dep = dep.split('.').next().unwrap().trim();
                if let Some((pos, dep)) = pkgs.iter().enumerate().find(|(_, pkg)| pkg.name == dep) {
                    if !depends_on[i].contains(&pos) {
                        println!("{pkg} depends on {dep}");
                        depends_on[i].push(pos);
                        dependants[pos] += 1;
                    }
                }
            }
        }
    }

    let mut eligible_nodes = dependants
        .iter()
        .enumerate()
        .filter_map(|(i, dependants)| if *dependants == 0 { Some(i) } else { None })
        .collect::<Vec<_>>();
    assert!(!eligible_nodes.is_empty(), "dependency cycle detected");

    let mut list = Vec::new();
    while let Some(i) = eligible_nodes.pop() {
        list.push(pkgs[i].clone());
        for &dep in &depends_on[i] {
            dependants[dep] -= 1;
            if dependants[dep] == 0 {
                eligible_nodes.push(dep);
            }
        }
    }

    // A -> B will place A in front of B in the ordering, but we need the opposite.
    list.reverse();

    assert!(dependants.iter().all(|i| *i == 0));
    assert_eq!(list.len(), pkgs.len());

    list
}

fn shell(cmd: &str) -> Result<()> {
    shell_with_stdin(cmd, "")
}

fn shell_with_stdin(cmd: &str, stdin: &str) -> Result<()> {
    shell_ex(cmd, stdin, false)
}

fn shell_ex(cmd: &str, stdin: &str, sudo: bool) -> Result<()> {
    assert!(
        !cmd.contains('"'),
        "quoting and escaping command-line arguments is not supported"
    );

    print!(
        "> {sudo}{cmd}",
        sudo = sudo.then_some("sudo ").unwrap_or(""),
        cmd = cmd.trim(),
    );
    if stdin.is_empty() {
        println!();
    } else {
        println!(" <<<EOF");
        if stdin.ends_with('\n') {
            print!("{stdin}");
        } else {
            println!("{stdin}");
        }
        println!("EOF");
    }

    if cfg!(test) {
        Ok(())
    } else {
        let mut child = command_ex(cmd, sudo)
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to execute '{cmd}': {e}"))?;
        let mut child_stdin = child.stdin.take().unwrap();
        child_stdin.write_all(stdin.as_bytes())?;
        child_stdin.flush()?;
        drop(child_stdin);

        let status = child.wait()?;
        check_status(status)
    }
}

fn command(cmd: &str) -> Command {
    command_ex(cmd, false)
}

fn command_ex(cmd: &str, sudo: bool) -> Command {
    let words = cmd
        .split_ascii_whitespace()
        .filter(|arg| !arg.trim().is_empty())
        .collect::<Vec<_>>();
    let (program, args) = words.split_first().unwrap();

    let mut command = if sudo {
        // GHA runner VMs have a very strict sudo configuration that won't search the invoking
        // user's PATH and makes preserving it difficult.
        // We have to find the full path to the program ourselves.
        let output = Command::new("which")
            .arg(program)
            .output()
            .expect("failed to run `which`");
        check_status(output.status).unwrap();

        let mut command = Command::new("sudo");
        command.args(["-n", "-E", "--preserve-env=PATH"]);
        command.arg(str::from_utf8(&output.stdout).unwrap().trim_ascii());
        command.args(args);
        command
    } else {
        let mut command = Command::new(program);
        command.args(args);
        command
    };

    setup_environment(program, &mut command);

    command
}

/// Prepares environment variables for invocation of `program`.
fn setup_environment(program: &str, cmd: &mut Command) {
    // Remove the crates.io token so that tests and build scripts can't read it. It is explicitly
    // passed to Cargo via `--token` when needed.
    cmd.env_remove("CRATES_IO_TOKEN");

    // The same goes for the `GITHUB_TOKEN`.
    cmd.env_remove("GITHUB_TOKEN");

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
        "gh" => {
            if let Some(token) = env::var_os("GITHUB_TOKEN") {
                cmd.env("GH_TOKEN", token);
            }
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
