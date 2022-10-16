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
                let token = env::var("CRATES_IO_TOKEN").expect("no `CRATES_IO_TOKEN` provided");
                shell(&format!("git tag {tag}"))?;
                shell(&format!("cargo publish -p {name} --token {}", token))?;
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
    fn recurse(dir: PathBuf, out: &mut Vec<Package>) -> Result<()> {
        let mut toml = dir.clone();
        toml.push("Cargo.toml");
        if toml.exists() {
            let manifest = fs::read_to_string(&toml)?;
            // Filter out virtual manifests and those with `publish = ...` set.
            if manifest.contains("[package]") && get_field(&manifest, "publish").is_err() {
                let name = get_field(&manifest, "name")?.to_string();
                let version = get_field(&manifest, "version")?.to_string();

                out.push(Package { name, version });
            }
        }

        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                recurse(entry.path(), out)?;
            }
        }
        Ok(())
    }

    let mut out = Vec::new();
    recurse(env::current_dir()?, &mut out)?;
    Ok(out)
}

fn get_field<'a>(text: &'a str, name: &str) -> Result<&'a str> {
    for line in text.lines() {
        let words = line.split_ascii_whitespace().collect::<Vec<_>>();
        match words.as_slice() {
            [n, "=", v, ..] if n.trim() == name => {
                assert!(v.starts_with('"') && v.ends_with('"'));
                return Ok(&v[1..v.len() - 1]);
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
