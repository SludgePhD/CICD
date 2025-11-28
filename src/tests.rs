//! Test support.

use std::{cell::RefCell, path::PathBuf};

use expect_test::{expect, Expect};

use crate::{Params, Pipeline, Workspace};

#[allow(unused_macros)]
macro_rules! print {
    ($($t:tt)*) => {{
        #[cfg(not(test))]
        ::std::print!($($t)*);
        crate::tests::append_stdout(::std::format!($($t)*));
    }};
}
#[allow(unused_macros)]
macro_rules! eprint {
    () => {};
}
macro_rules! println {
    () => {{
        #[cfg(not(test))]
        ::std::println!();
        crate::tests::append_stdout_ln(String::new());
    }};
    ($($t:tt)+) => {{
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
        let test_commit = "1234567890abcdef".to_string();
        Self {
            cwd: test_dir(subdir),
            args: String::new(),
            crates_io_token: Some("dummy-token".into()),
            github_token: Some("github-dummy-token".into()),
            commit: test_commit.clone(),
            cargo_doc_flags: String::new(),
            check_only: false,
            skip_docs: false,
            sudo: false,
            mock_output: Some(vec![
                ("git status --porcelain", "".into()),
                ("git rev-parse HEAD", test_commit),
                ("git branch --show-current", "main".into()),
                ("git tag --list", "".into()),
            ]),
        }
    }

    fn replace_output(&mut self, cmd: &str, output: String) {
        let (_, out) = self
            .mock_output
            .as_mut()
            .unwrap()
            .iter_mut()
            .find(|(command, _)| *command == cmd)
            .expect("command not found");
        *out = output;
    }

    fn with_tags(mut self, tags: &[&str]) -> Self {
        self.replace_output("git tag --list", tags.join("\n"));
        self
    }

    fn with_sudo(mut self) -> Self {
        self.sudo = true;
        self
    }
}

fn check_output(params: Params, expect: Expect) {
    OUTPUT.replace(String::new());

    Pipeline::new(params).unwrap().run().unwrap();

    let output = OUTPUT.take();
    expect.assert_eq(&output);
}
fn check_error(params: Params, expected_error: Expect, expected_output: Expect) {
    OUTPUT.replace(String::new());

    let error = match Pipeline::new(params) {
        Ok(pipe) => pipe.run().unwrap_err(),
        Err(e) => e,
    };

    let mut string = error.to_string();
    if !string.ends_with('\n') {
        string.push('\n');
    }
    expected_error.assert_eq(&string);

    let output = OUTPUT.take();
    expected_output.assert_eq(&output);
}

fn check_find_packages(subdir: &str, expect: Expect) {
    let packages = Workspace::get(test_dir(subdir))
        .unwrap()
        .find_packages()
        .unwrap();
    expect.assert_debug_eq(&packages);
}

fn check_find_packages_errors(subdir: &str) {
    Workspace::get(test_dir(subdir))
        .unwrap()
        .find_packages()
        .unwrap_err();
}

/// Makes sure that we won't find our own test packages during dogfeeding (only `sludge-cicd`).
#[test]
fn does_not_find_test_packages() {
    let packages = Workspace::get(test_dir(".."))
        .unwrap()
        .find_packages()
        .unwrap();
    assert_eq!(packages.len(), 1, "{packages:#?}");
    assert_eq!(packages[0].name, "sludge-cicd");
}

#[test]
fn project_dir_does_not_exist() {
    check_find_packages_errors("does-not-exist");
}

#[test]
fn project_dir_has_no_manifest() {
    check_find_packages_errors("empty");
}

#[test]
fn missing_metadata() {
    check_error(
        Params::test("no-license"),
        expect![[r#"
            package `mypkg` is missing a license field
        "#]],
        expect![[r#"
            ::group::INIT
            INIT: 0.00ns
            ::endgroup::
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
        "#]],
    );
    check_error(
        Params::test("no-description"),
        expect![[r#"
            package `mypkg` is missing a description field
        "#]],
        expect![[r#"
            ::group::INIT
            INIT: 0.00ns
            ::endgroup::
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
        "#]],
    );
}

#[test]
fn single_package() {
    check_find_packages(
        "single-package",
        expect![[r#"
            [
                single-package@2.2.2,
            ]
        "#]],
    );

    check_output(
        Params::test("single-package"),
        expect![[r#"
            ::group::INIT
            INIT: 0.00ns
            ::endgroup::
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
            ::group::PUBLISH
            existing git tags: []
            publishable packages in workspace: [single-package@2.2.2]
            1 package needs publishing: [single-package@2.2.2]
            publishing single-package@2.2.2
            > cargo publish --no-verify -p single-package --token dummy-token
            > git tag v2.2.2
            > git push --tags
            PUBLISH: 0.00ns
            ::endgroup::
        "#]],
    );
    check_output(
        Params::test("single-package").with_tags(&["v2.2.1"]),
        expect![[r#"
            ::group::INIT
            INIT: 0.00ns
            ::endgroup::
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
            ::group::PUBLISH
            existing git tags: ["v2.2.1"]
            publishable packages in workspace: [single-package@2.2.2]
            1 package needs publishing: [single-package@2.2.2]
            publishing single-package@2.2.2
            > cargo publish --no-verify -p single-package --token dummy-token
            > git tag v2.2.2
            > git push --tags
            PUBLISH: 0.00ns
            ::endgroup::
        "#]],
    );
}

#[test]
fn single_package_existing_tag() {
    check_output(
        Params::test("single-package").with_tags(&["v2.2.2"]),
        expect![[r#"
            ::group::INIT
            INIT: 0.00ns
            ::endgroup::
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
            ::group::PUBLISH
            existing git tags: ["v2.2.2"]
            publishable packages in workspace: [single-package@2.2.2]
            no packages need publishing, done
            PUBLISH: 0.00ns
            ::endgroup::
        "#]],
    );
    check_output(
        Params::test("single-package").with_tags(&["single-package-v2.2.2"]),
        expect![[r#"
            ::group::INIT
            INIT: 0.00ns
            ::endgroup::
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
            ::group::PUBLISH
            existing git tags: ["single-package-v2.2.2"]
            publishable packages in workspace: [single-package@2.2.2]
            no packages need publishing, done
            PUBLISH: 0.00ns
            ::endgroup::
        "#]],
    );
}

#[test]
fn single_package_sudo() {
    check_output(
        Params::test("single-package").with_sudo(),
        expect![[r#"
            ::group::INIT
            INIT: 0.00ns
            ::endgroup::
            ::group::BUILD
            > cargo test --workspace --no-run
            BUILD: 0.00ns
            ::endgroup::
            ::group::BUILD_DOCS
            > cargo doc --workspace
            BUILD_DOCS: 0.00ns
            ::endgroup::
            ::group::TEST
            > sudo cargo test --workspace
            TEST: 0.00ns
            ::endgroup::
            ::group::PUBLISH
            existing git tags: []
            publishable packages in workspace: [single-package@2.2.2]
            1 package needs publishing: [single-package@2.2.2]
            publishing single-package@2.2.2
            > cargo publish --no-verify -p single-package --token dummy-token
            > git tag v2.2.2
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
                version-normal@4.5.6,
                version-workspace@555.222.333,
            ]
        "#]],
    );
    check_find_packages(
        "workspace-inheritance2",
        expect![[r#"
            [
                version-normal@4.5.6,
                version-workspace@555.222.333,
            ]
        "#]],
    );

    check_output(
        Params::test("workspace-inheritance"),
        expect![[r#"
            ::group::INIT
            INIT: 0.00ns
            ::endgroup::
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
            ::group::PUBLISH
            existing git tags: []
            publishable packages in workspace: [version-normal@4.5.6, version-workspace@555.222.333]
            2 packages need publishing: [version-normal@4.5.6, version-workspace@555.222.333]
            publishing version-normal@4.5.6
            > cargo publish --no-verify -p version-normal --token dummy-token
            publishing version-workspace@555.222.333
            > cargo publish --no-verify -p version-workspace --token dummy-token
            > git tag version-normal-v4.5.6
            > git tag version-workspace-v555.222.333
            > git push --tags
            PUBLISH: 0.00ns
            ::endgroup::
        "#]],
    );
    check_output(
        Params::test("workspace-inheritance2"),
        expect![[r#"
            ::group::INIT
            INIT: 0.00ns
            ::endgroup::
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
            ::group::PUBLISH
            existing git tags: []
            publishable packages in workspace: [version-normal@4.5.6, version-workspace@555.222.333]
            2 packages need publishing: [version-normal@4.5.6, version-workspace@555.222.333]
            publishing version-normal@4.5.6
            > cargo publish --no-verify -p version-normal --token dummy-token
            publishing version-workspace@555.222.333
            > cargo publish --no-verify -p version-workspace --token dummy-token
            > git tag version-normal-v4.5.6
            > git tag version-workspace-v555.222.333
            > git push --tags
            PUBLISH: 0.00ns
            ::endgroup::
        "#]],
    );
}

#[test]
fn synced_derive() {
    check_find_packages(
        "synced-derive",
        expect![[r#"
            [
                mylib-derive@0.1.2,
                mylib@0.1.2,
            ]
        "#]],
    );

    check_output(
        Params::test("synced-derive"),
        expect![[r#"
            ::group::INIT
            mylib@0.1.2 depends on mylib-derive@0.1.2
            INIT: 0.00ns
            ::endgroup::
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
            ::group::PUBLISH
            existing git tags: []
            publishable packages in workspace: [mylib-derive@0.1.2, mylib@0.1.2]
            2 packages need publishing: [mylib-derive@0.1.2, mylib@0.1.2]
            publishing mylib-derive@0.1.2
            > cargo publish --no-verify -p mylib-derive --token dummy-token
            publishing mylib@0.1.2
            > cargo publish --no-verify -p mylib --token dummy-token
            > git tag v0.1.2
            > git push --tags
            PUBLISH: 0.00ns
            ::endgroup::
        "#]],
    );
    check_output(
        Params::test("synced-derive").with_tags(&["v0.1.2"]),
        expect![[r#"
            ::group::INIT
            mylib@0.1.2 depends on mylib-derive@0.1.2
            INIT: 0.00ns
            ::endgroup::
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
            ::group::PUBLISH
            existing git tags: ["v0.1.2"]
            publishable packages in workspace: [mylib-derive@0.1.2, mylib@0.1.2]
            no packages need publishing, done
            PUBLISH: 0.00ns
            ::endgroup::
        "#]],
    );
    check_output(
        Params::test("synced-derive").with_tags(&["mylib-v0.1.2"]),
        expect![[r#"
            ::group::INIT
            mylib@0.1.2 depends on mylib-derive@0.1.2
            INIT: 0.00ns
            ::endgroup::
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
            ::group::PUBLISH
            existing git tags: ["mylib-v0.1.2"]
            publishable packages in workspace: [mylib-derive@0.1.2, mylib@0.1.2]
            1 package needs publishing: [mylib-derive@0.1.2]
            publishing mylib-derive@0.1.2
            > cargo publish --no-verify -p mylib-derive --token dummy-token
            > git tag mylib-derive-v0.1.2
            > git push --tags
            PUBLISH: 0.00ns
            ::endgroup::
        "#]],
    );
}

#[test]
fn dep_graph() {
    check_output(
        Params::test("graph"),
        expect![[r#"
            ::group::INIT
            b@0.1.0 depends on a@0.1.0
            b@0.1.0 depends on d@0.1.0
            c@0.1.0 depends on a@0.1.0
            INIT: 0.00ns
            ::endgroup::
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
            ::group::PUBLISH
            existing git tags: []
            publishable packages in workspace: [a@0.1.0, d@0.1.0, b@0.1.0, c@0.1.0]
            4 packages need publishing: [a@0.1.0, d@0.1.0, b@0.1.0, c@0.1.0]
            publishing a@0.1.0
            > cargo publish --no-verify -p a --token dummy-token
            publishing d@0.1.0
            > cargo publish --no-verify -p d --token dummy-token
            publishing b@0.1.0
            > cargo publish --no-verify -p b --token dummy-token
            publishing c@0.1.0
            > cargo publish --no-verify -p c --token dummy-token
            > git tag v0.1.0
            > git push --tags
            PUBLISH: 0.00ns
            ::endgroup::
        "#]],
    );
}

#[test]
fn nonvirtual_workspace_changelog() {
    // Non-virtual workspace with 2 packages published at 1.0.0.
    // There is one shared CHANGELOG.md, so there should be a single tag and release.
    check_output(
        Params::test("nonvirtual-workspace-changelog"),
        expect![[r#"
            ::group::INIT
            INIT: 0.00ns
            ::endgroup::
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
            ::group::PUBLISH
            existing git tags: []
            publishable packages in workspace: [subpackage@1.0.0, toplevel@1.0.0]
            2 packages need publishing: [subpackage@1.0.0, toplevel@1.0.0]
            publishing subpackage@1.0.0
            > cargo publish --no-verify -p subpackage --token dummy-token
            publishing toplevel@1.0.0
            > cargo publish --no-verify -p toplevel --token dummy-token
            > git tag v1.0.0
            > git push --tags
            > gh release create v1.0.0 --notes-file - <<<EOF
            # subpackage 1.0.0

            - Subpackage Bla

            # toplevel 1.0.0

            - Toplevel Bla

            ### blabla 1.0.0

            ```rust
            {
                code example :)
            }
            ```
            EOF
            PUBLISH: 0.00ns
            ::endgroup::
        "#]],
    );

    // If a tag indicates that subpackage 1.0.0 was already published, only `toplevel` should be
    // published and tagged, and only `toplevel` should get a release (which should only contain its
    // release notes).
    check_output(
        Params::test("nonvirtual-workspace-changelog").with_tags(&["subpackage-v1.0.0"]),
        expect![[r#"
            ::group::INIT
            INIT: 0.00ns
            ::endgroup::
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
            ::group::PUBLISH
            existing git tags: ["subpackage-v1.0.0"]
            publishable packages in workspace: [subpackage@1.0.0, toplevel@1.0.0]
            1 package needs publishing: [toplevel@1.0.0]
            publishing toplevel@1.0.0
            > cargo publish --no-verify -p toplevel --token dummy-token
            > git tag toplevel-v1.0.0
            > git push --tags
            > gh release create toplevel-v1.0.0 --notes-file - <<<EOF
            - Toplevel Bla

            ### blabla 1.0.0

            ```rust
            {
                code example :)
            }
            ```
            EOF
            PUBLISH: 0.00ns
            ::endgroup::
        "#]],
    );
}

#[test]
fn single_package_changelog() {
    check_output(
        Params::test("single-package-changelog"),
        expect![[r#"
            ::group::INIT
            INIT: 0.00ns
            ::endgroup::
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
            ::group::PUBLISH
            existing git tags: []
            publishable packages in workspace: [mypackage@0.1.0]
            1 package needs publishing: [mypackage@0.1.0]
            publishing mypackage@0.1.0
            > cargo publish --no-verify -p mypackage --token dummy-token
            > git tag v0.1.0
            > git push --tags
            > gh release create v0.1.0 --notes-file - <<<EOF
            Notes for 0.1.0
            EOF
            PUBLISH: 0.00ns
            ::endgroup::
        "#]],
    );
}

#[test]
fn multiple_changelogs() {
    check_output(
        Params::test("workspace-with-package-changelog"),
        expect![[r#"
            ::group::INIT
            INIT: 0.00ns
            ::endgroup::
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
            ::group::PUBLISH
            existing git tags: []
            publishable packages in workspace: [a@0.1.0, b@0.1.0]
            2 packages need publishing: [a@0.1.0, b@0.1.0]
            publishing a@0.1.0
            > cargo publish --no-verify -p a --token dummy-token
            publishing b@0.1.0
            > cargo publish --no-verify -p b --token dummy-token
            > git tag a-v0.1.0
            > git tag b-v0.1.0
            > git push --tags
            > gh release create a-v0.1.0 --notes-file - <<<EOF
            - entry for `a`
            EOF
            > gh release create b-v0.1.0 --notes-file - <<<EOF
            - entry for `b`
            EOF
            PUBLISH: 0.00ns
            ::endgroup::
        "#]],
    );
}

#[test]
fn changelog_shared() {
    check_output(
        Params::test("changelog-shared"),
        expect![[r#"
            ::group::INIT
            INIT: 0.00ns
            ::endgroup::
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
            ::group::PUBLISH
            existing git tags: []
            publishable packages in workspace: [derive@0.1.0, shared@0.1.0]
            2 packages need publishing: [derive@0.1.0, shared@0.1.0]
            publishing derive@0.1.0
            > cargo publish --no-verify -p derive --token dummy-token
            publishing shared@0.1.0
            > cargo publish --no-verify -p shared --token dummy-token
            > git tag v0.1.0
            > git push --tags
            > gh release create v0.1.0 --notes-file - <<<EOF
            - shared changelog for `derive` and `shared`
            EOF
            PUBLISH: 0.00ns
            ::endgroup::
        "#]],
    );

    check_output(
        Params::test("changelog-shared").with_tags(&["shared-v0.1.0"]),
        expect![[r#"
            ::group::INIT
            INIT: 0.00ns
            ::endgroup::
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
            ::group::PUBLISH
            existing git tags: ["shared-v0.1.0"]
            publishable packages in workspace: [derive@0.1.0, shared@0.1.0]
            1 package needs publishing: [derive@0.1.0]
            publishing derive@0.1.0
            > cargo publish --no-verify -p derive --token dummy-token
            > git tag derive-v0.1.0
            > git push --tags
            > gh release create derive-v0.1.0 --notes-file - <<<EOF
            - shared changelog for `derive` and `shared`
            EOF
            PUBLISH: 0.00ns
            ::endgroup::
        "#]],
    );
}

#[test]
fn changelog_version_missing() {
    check_error(
        Params::test("changelog-version-missing"),
        expect![[r#"
            changelog at 'sludge-cicd-test-projects/changelog-version-missing/CHANGELOG.md' does not contain an entry for mypkg@0.1.1
        "#]],
        expect![[r#"
            ::group::INIT
            INIT: 0.00ns
            ::endgroup::
        "#]],
    );
}
