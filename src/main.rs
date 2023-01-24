use std::{
    env, fs,
    path::PathBuf,
    process::{self, Command, ExitStatus, Stdio},
    time::Instant,
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
    let token = env::var("CRATES_IO_TOKEN");
    let check_only = env::var_os("CICD_CHECK_ONLY").is_some();
    let cwd = env::current_dir()?;
    let cargo_toml = cwd.join("Cargo.toml");
    assert!(
        cargo_toml.exists(),
        "Cargo.toml not found, cwd: {}",
        cwd.display()
    );

    if check_only {
        let _s = Section::new("CHECK");
        shell("cargo check --workspace")?;
    } else {
        let _s = Section::new("BUILD");
        shell("cargo test --workspace --no-run")?;
    }

    {
        let _s = Section::new("BUILD_DOCS");
        shell("cargo doc --workspace")?;
    }

    if !check_only {
        let _s = Section::new("TEST");
        shell("cargo test --workspace")?;
    }

    let current_branch = shell_output("git branch --show-current")?;
    if &current_branch == "main" {
        let Ok(token) = token else {
            println!("no `CRATES_IO_TOKEN` set, skipping autopublish step");
            return Ok(());
        };

        let _s = Section::new("PUBLISH");
        let tags = shell_output("git tag --list")?;

        let packages = find_packages()?;
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
                eprintln!("publishing {name} {version}");
                let tag = format!("{prefix}v{version}");
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

fn find_packages() -> Result<Vec<Package>> {
    fn recurse(
        dir: PathBuf,
        out: &mut Vec<Package>,
        workspace_version: Option<&str>,
    ) -> Result<()> {
        let mut toml = dir.clone();
        toml.push("Cargo.toml");
        if toml.exists() {
            let manifest = fs::read_to_string(&toml)?;
            // Filter out virtual manifests and those with `publish = false` set.
            if manifest.contains("[package]")
                && !matches!(get_field(&manifest, "publish"), Ok(Value::Bool(false)))
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

        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                recurse(entry.path(), out, workspace_version)?;
            }
        }
        Ok(())
    }

    let workspace_version = workspace_version().ok();
    let mut out = Vec::new();
    recurse(env::current_dir()?, &mut out, workspace_version.as_deref())?;
    Ok(out)
}

fn workspace_version() -> Result<String> {
    let mut path = env::current_dir()?;
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
    let status = command(cmd).status()?;
    check_status(status)
}

fn shell_output(cmd: &str) -> Result<String> {
    let output = command(cmd).stderr(Stdio::inherit()).output()?;
    check_status(output.status)?;
    let res = String::from_utf8(output.stdout)?;
    let res = res.trim().to_string();
    println!("{}", res);
    Ok(res)
}

fn command(cmd: &str) -> Command {
    eprintln!("> {}", cmd);
    let words = cmd.split_ascii_whitespace().collect::<Vec<_>>();
    let (cmd, args) = words.split_first().unwrap();
    let mut res = Command::new(cmd);
    res.args(args);
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
        println!("{}: {:.2?}", self.name, self.start.elapsed());
        println!("::endgroup::");
    }
}
