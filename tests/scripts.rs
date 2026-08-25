#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};
use tempfile::tempdir;

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).expect("write executable fixture");
    let mut permissions = fs::metadata(path).expect("fixture metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("make fixture executable");
}

fn command_path(command: &str) -> PathBuf {
    let output = Command::new("/bin/sh")
        .args(["-c", "command -v \"$1\"", "sh", command])
        .output()
        .expect("resolve command path");
    assert!(output.status.success(), "command not found: {command}");
    PathBuf::from(
        String::from_utf8(output.stdout)
            .expect("command path is UTF-8")
            .trim(),
    )
}

fn pinned_versions() -> (String, String) {
    let manifest: toml::Value = toml::from_str(include_str!("../crates/suiko-sudachi/Cargo.toml"))
        .expect("parse suiko-sudachi manifest");
    let sudachi = manifest["package"]["version"]
        .as_str()
        .expect("package version");
    let dictionary = include_str!("../build.rs")
        .lines()
        .find(|line| line.starts_with("const DICT_NAME"))
        .and_then(|line| line.split('"').nth(1))
        .and_then(|name| name.split_whitespace().nth(1))
        .expect("dictionary version");
    (sudachi.to_owned(), dictionary.to_owned())
}

fn sha256(path: &Path) -> String {
    let digest = Sha256::digest(fs::read(path).expect("read file for digest"));
    digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join("")
}

fn install_release_helper_fixture(root: &Path) -> (PathBuf, PathBuf) {
    let scripts = root.join("scripts");
    let bin = root.join("bin");
    let state = root.join("state");
    fs::create_dir_all(&scripts).expect("create scripts fixture");
    fs::create_dir_all(&bin).expect("create bin fixture");
    fs::create_dir_all(&state).expect("create state fixture");
    fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/build-remedy-release.sh"
        ),
        scripts.join("build-remedy-release.sh"),
    )
    .expect("copy release helper");
    symlink(command_path("dirname"), bin.join("dirname")).expect("link dirname");
    write_executable(
        &bin.join("git"),
        r#"#!/bin/sh
case "$*" in
  *status*)
    printf 'status\n' >> "$TEST_STATE/git.log"
    if [ -f "$TEST_STATE/dirty" ]; then printf '%s\n' ' M tracked'; fi
    ;;
  *rev-parse*) printf '%s\n' '0123456789abcdef0123456789abcdef01234567' ;;
  *) exit 2 ;;
esac
"#,
    );
    write_executable(
        &bin.join("cargo"),
        r#"#!/bin/sh
printf '%s\n' "$*" > "$TEST_STATE/cargo.args"
printf '%s\n' "$SUIKO_REMEDY_COMMIT" > "$TEST_STATE/commit"
if [ "${MAKE_DIRTY:-}" = 1 ]; then : > "$TEST_STATE/dirty"; fi
"#,
    );
    (scripts.join("build-remedy-release.sh"), state)
}

#[test]
fn sudachi_update_check_runs_without_python() {
    let dir = tempdir().expect("temporary directory");
    let bin = dir.path().join("bin");
    fs::create_dir(&bin).expect("create fixture bin directory");
    for command in ["cut", "dirname", "grep", "head", "sed"] {
        symlink(command_path(command), bin.join(command)).expect("link fixture command");
    }
    let (sudachi, dictionary) = pinned_versions();
    write_executable(
        &bin.join("gh"),
        &format!(
            r#"#!/bin/sh
case "$2" in
  repos/WorksApplications/sudachi.rs/tags) printf '%s\n' '{sudachi}' ;;
  repos/WorksApplications/SudachiDict/releases/latest) printf '%s\n' '{dictionary}' ;;
  *) exit 2 ;;
esac
"#
        ),
    );
    write_executable(&bin.join("curl"), "#!/bin/sh\nprintf '%s' '404'\n");

    let output = Command::new("/bin/sh")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/check-sudachi-updates.sh"
        ))
        .env("PATH", &bin)
        .output()
        .expect("run update check");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("更新なし"),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn remedy_release_helper_forces_locked_build_and_checks_afterward() {
    let dir = tempdir().expect("temporary directory");
    let (helper, state) = install_release_helper_fixture(dir.path());
    let output = Command::new("/bin/sh")
        .arg(helper)
        .arg("--target")
        .arg("test-target")
        .env("PATH", dir.path().join("bin"))
        .env("TEST_STATE", &state)
        .output()
        .expect("run release helper");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(state.join("cargo.args")).unwrap().trim(),
        "build --release --locked --bins --target test-target"
    );
    assert_eq!(
        fs::read_to_string(state.join("commit")).unwrap().trim(),
        "0123456789abcdef0123456789abcdef01234567"
    );
    assert_eq!(
        fs::read_to_string(state.join("git.log"))
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[test]
fn remedy_release_helper_rejects_build_side_effects() {
    let dir = tempdir().expect("temporary directory");
    let (helper, state) = install_release_helper_fixture(dir.path());
    let output = Command::new("/bin/sh")
        .arg(helper)
        .env("PATH", dir.path().join("bin"))
        .env("TEST_STATE", state)
        .env("MAKE_DIRTY", "1")
        .output()
        .expect("run release helper");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("changed the checkout"));
}

#[test]
fn official_dictionary_notices_match_recorded_digests() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("third_party/sudachidict");
    assert_eq!(
        sha256(&root.join("LEGAL")),
        "725a8776b38e058b185e905594bc9a2437dbf3787df022fffeefedb9a84e4665"
    );
    assert_eq!(
        sha256(&root.join("LICENSE-2.0.txt")),
        "cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30"
    );
}

#[test]
fn release_notice_verifier_accepts_exact_staging_and_rejects_changes() {
    let dir = tempdir().expect("temporary directory");
    let release = dir.path().join("release");
    let sudachi = release.join("licenses/suiko-sudachi");
    let dictionary = release.join("licenses/sudachidict");
    fs::create_dir_all(&sudachi).unwrap();
    fs::create_dir_all(&dictionary).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::copy(
        root.join("crates/suiko-sudachi/LICENSE"),
        sudachi.join("LICENSE-2.0.txt"),
    )
    .unwrap();
    for name in ["LEGAL", "LICENSE-2.0.txt", "PROVENANCE.md"] {
        fs::copy(
            root.join("third_party/sudachidict").join(name),
            dictionary.join(name),
        )
        .unwrap();
    }
    let verifier = root.join("scripts/verify-release-notices.sh");
    assert!(
        Command::new("/bin/sh")
            .arg(&verifier)
            .arg(&release)
            .output()
            .unwrap()
            .status
            .success()
    );
    fs::write(dictionary.join("LEGAL"), "changed").unwrap();
    assert!(
        !Command::new("/bin/sh")
            .arg(verifier)
            .arg(release)
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[test]
fn release_workflow_packages_both_runtime_binaries() {
    let workflow = include_str!("../.github/workflows/release.yml");
    assert!(workflow.contains("target/${TARGET}/release/suiko-remedy"));
    assert!(workflow.contains("target/$env:TARGET/release/suiko-remedy.exe"));
    assert!(workflow.contains("scripts/verify-release-notices.sh"));
}
