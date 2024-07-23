//! Test support.

use std::{cell::RefCell, path::PathBuf};

use expect_test::{expect, Expect};

use crate::{find_packages, Params};

#[allow(unused_macros)]
macro_rules! print {
    () => {};
}
#[allow(unused_macros)]
macro_rules! eprint {
    () => {};
}
macro_rules! println {
    ($($t:tt)*) => {{
        #[cfg(not(test))]
        ::std::println!($($t)*);
        crate::tests::append_stdout_ln(::std::format!($($t)*));
    }};
}
macro_rules! eprintln {
    ($($t:tt)*) => {{
        #[cfg(not(test))]
        ::std::eprintln!($($t)*);
        crate::tests::append_stderr_ln(::std::format!($($t)*));
    }};
}

thread_local! {
    static OUTPUT: RefCell<String> = RefCell::new(String::new());
}

pub fn append_stdout(s: String) {
    OUTPUT.with_borrow_mut(|b| b.push_str(&s));
}

pub fn append_stdout_ln(mut s: String) {
    s.push('\n');
    append_stdout(s);
}

pub fn append_stderr(s: String) {
    OUTPUT.with_borrow_mut(|b| b.push_str(&s));
}

pub fn append_stderr_ln(mut s: String) {
    s.push('\n');
    append_stderr(s);
}

fn test_dir(subdir: &str) -> PathBuf {
    format!(
        "{}/sludge-cicd-test-projects/{}",
        env!("CARGO_MANIFEST_DIR"),
        subdir
    )
    .into()
}

impl Params {
    fn test(subdir: &str) -> Self {
        Self {
            cwd: test_dir(subdir),
            args: Vec::new(),
            crates_io_token: Some("dummy-token".into()),
            check_only: false,
            skip_docs: false,
            mock_output: Some(vec![
                ("git branch --show-current", "main".into()),
                ("git tag --list", "".into()),
            ]),
        }
    }
}

fn check_output(params: Params, expect: Expect) {
    OUTPUT.replace(String::new());

    crate::run_cicd(params).unwrap();

    let output = OUTPUT.take();
    expect.assert_eq(&output);
}

fn check_find_packages(subdir: &str, expect: Expect) {
    let packages = find_packages(test_dir(subdir)).unwrap();
    expect.assert_debug_eq(&packages);
}

fn check_find_packages_errors(subdir: &str) {
    find_packages(test_dir(subdir)).unwrap_err();
}

/// Makes sure that we won't find our own test packages during dogfeeding.
#[test]
fn does_not_find_test_packages() {
    let packages = find_packages(test_dir("..")).unwrap();
    assert_eq!(packages.len(), 1, "{packages:#?}");
    assert_eq!(packages[0].name, "sludge-cicd");
}

#[test]
fn single_package() {
    check_find_packages(
        "single-package",
        expect![[r#"
            [
                Package {
                    name: "single-package",
                    version: "2.2.2",
                },
            ]
        "#]],
    );

    check_output(
        Params::test("single-package"),
        expect![[r#"
            ::group::BUILD
            > cargo test --workspace --no-run
            BUILD: 0.00ns
            ::endgroup::
            ::group::BUILD_DOCS
            > cargo doc --workspace
            BUILD_DOCS: 0.00ns
            ::endgroup::
            ::group::TEST
            > cargo test --workspace
            TEST: 0.00ns
            ::endgroup::
            > git branch --show-current
            ::group::PUBLISH
            > git tag --list
            publishable packages in workspace: [Package { name: "single-package", version: "2.2.2" }]
            publishing single-package 2.2.2 (with git tag v2.2.2)
            > git tag v2.2.2
            > cargo publish --no-verify -p single-package --token dummy-token
            > git push --tags
            PUBLISH: 0.00ns
            ::endgroup::
        "#]],
    );
}

#[test]
fn workspace_inheritance() {
    check_find_packages(
        "workspace-inheritance",
        expect![[r#"
            [
                Package {
                    name: "version-normal",
                    version: "4.5.6",
                },
                Package {
                    name: "version-workspace",
                    version: "555.222.333",
                },
            ]
        "#]],
    );

    check_output(
        Params::test("workspace-inheritance"),
        expect![[r#"
            ::group::BUILD
            > cargo test --workspace --no-run
            BUILD: 0.00ns
            ::endgroup::
            ::group::BUILD_DOCS
            > cargo doc --workspace
            BUILD_DOCS: 0.00ns
            ::endgroup::
            ::group::TEST
            > cargo test --workspace
            TEST: 0.00ns
            ::endgroup::
            > git branch --show-current
            ::group::PUBLISH
            > git tag --list
            publishable packages in workspace: [Package { name: "version-normal", version: "4.5.6" }, Package { name: "version-workspace", version: "555.222.333" }]
            publishing version-normal 4.5.6 (with git tag version-normal-v4.5.6)
            > git tag version-normal-v4.5.6
            > cargo publish --no-verify -p version-normal --token dummy-token
            > git push --tags
            publishing version-workspace 555.222.333 (with git tag version-workspace-v555.222.333)
            > git tag version-workspace-v555.222.333
            > cargo publish --no-verify -p version-workspace --token dummy-token
            > git push --tags
            PUBLISH: 0.00ns
            ::endgroup::
        "#]],
    );
}

#[test]
fn project_dir_does_not_exist() {
    check_find_packages_errors("does-not-exist");
}

#[test]
fn project_dir_has_no_manifest() {
    check_find_packages_errors("empty");
}
