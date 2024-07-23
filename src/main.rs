#[cfg(test)]
#[macro_use]
mod tests;

use std::{
    env, fs,
    path::PathBuf,
    process::{self, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

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

    run_cicd(Params {
        cwd,
        args,
        crates_io_token,
        check_only,
        skip_docs,
        mock_output: None,
    })
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
        eprintln!("> {}", cmd);

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
                println!("{}", res);
                Ok(res)
            }
        }
    }
}

fn run_cicd(mut params: Params) -> Result<()> {
    let args = params.args.join(" ");
    let cargo_toml = params.cwd.join("Cargo.toml");
    assert!(
        cargo_toml.exists(),
        "Cargo.toml not found, cwd: {}",
        params.cwd.display()
    );

    if params.check_only {
        let _s = Section::new("CHECK");
        shell(&format!("cargo check --workspace {args}"))?;
    } else {
        let _s = Section::new("BUILD");
        shell(&format!("cargo test --workspace --no-run {args}"))?;
    }

    if !params.skip_docs {
        let _s = Section::new("BUILD_DOCS");
        shell("cargo doc --workspace")?;
    }

    if !params.check_only {
        let _s = Section::new("TEST");
        shell(&format!("cargo test --workspace {args}"))?;
    }

    let current_branch = params.shell_output("git branch --show-current")?;
    if &current_branch == "main" {
        let Some(token) = params.crates_io_token.clone() else {
            println!("no `CRATES_IO_TOKEN` set, skipping autopublish step");
            return Ok(());
        };

        let _s = Section::new("PUBLISH");
        let tags = params.shell_output("git tag --list")?;

        let packages = find_packages(params.cwd)?;
        assert!(!packages.is_empty());
        eprintln!("publishable packages in workspace: {:?}", packages);

        // Did any previous release include multiple packages?
        let was_multi_package = packages.iter().any(|pkg| tags.contains(&pkg.name));
        let is_multi_package = packages.len() != 1;

        let needs_publish = |pkgname: &str, version: &str| {
            if was_multi_package {
                !tags.contains(&format!("{pkgname}-v{version}"))
            } else {
                !tags.contains(&format!("v{version}"))
            }
        };

        for Package { name, version } in packages {
            let prefix = if is_multi_package {
                format!("{}-", name)
            } else {
                String::new()
            };

            if needs_publish(&name, &version) {
                // NB: we use `--no-verify` because we might build the workspace crates out of
                // order, so a dependency might not be on crates.io when its dependents are
                // verified. This isn't easily fixable without pulling it dependencies and getting
                // the package graph somehow.
                let tag = format!("{prefix}v{version}");
                eprintln!("publishing {name} {version} (with git tag {tag})");
                shell(&format!("git tag {tag}"))?;
                shell(&format!(
                    "cargo publish --no-verify -p {name} --token {token}"
                ))?;
                shell("git push --tags")?;
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
struct Package {
    name: String,
    version: String,
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
            // Filter out virtual manifests, those with `publish = false` set, and those that lack a
            // `version` field.
            if manifest.contains("[package]")
                && !matches!(get_field(&manifest, "publish"), Ok(Value::Bool(false)))
                && (get_field(&manifest, "version").is_ok()
                    || get_field(&manifest, "version.workspace").is_ok())
            {
                let name = get_field(&manifest, "name")?
                    .as_str()
                    .ok_or("package name is not a string")?
                    .to_string();
                let version = match get_field(&manifest, "version") {
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
        let version = get_field(&manifest, "package.version")?
            .as_str()
            .ok_or("version is not a string")?;
        Ok(version.to_string())
    } else {
        Err("no workspace".into())
    }
}

enum Value<'a> {
    Str(&'a str),
    Bool(bool),
}

impl<'a> Value<'a> {
    fn as_str(&self) -> Option<&'a str> {
        if let Self::Str(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

fn get_field<'a>(text: &'a str, name: &str) -> Result<Value<'a>> {
    for line in text.lines() {
        let words = line.split_ascii_whitespace().collect::<Vec<_>>();
        match words.as_slice() {
            [n, "=", v, ..] if n.trim() == name => {
                let v = v.trim();
                if v.starts_with('"') {
                    assert!(
                        v.ends_with('"'),
                        "unclosed string, or trailing comment in '{line}'"
                    );
                    return Ok(Value::Str(&v[1..v.len() - 1]));
                } else if v.split(|v: char| !v.is_alphanumeric()).next().unwrap() == "true" {
                    return Ok(Value::Bool(true));
                } else if v.split(|v: char| !v.is_alphanumeric()).next().unwrap() == "false" {
                    return Ok(Value::Bool(false));
                }
            }
            _ => (),
        }
    }
    Err(format!("can't find `{}` in\n----\n{}\n----\n", name, text))?
}

fn shell(cmd: &str) -> Result<()> {
    eprintln!("> {}", cmd.trim());
    if cfg!(test) {
        return Ok(());
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
    let (cmd, args) = words.split_first().unwrap();
    let mut res = Command::new(cmd);
    res.env("CI", "1").env_remove("CRATES_IO_TOKEN").args(args);
    res
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
