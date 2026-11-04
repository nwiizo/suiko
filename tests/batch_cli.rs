use std::{fs, path::Path, process::Command};

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;

fn git(dir: &Path, args: &[&str]) {
    let result = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn json(dir: &Path, args: &[&str]) -> Value {
    let output = cargo_bin_cmd!("suiko")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn capabilities_and_summary_describe_actual_features_and_counts() {
    let dir = tempfile::tempdir().unwrap();
    let features = json(dir.path(), &["capabilities", "--json"]);
    assert!(
        features["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == "compare")
    );
    for category in suiko::lint::rule_categories() {
        assert!(
            features["rules"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["category"] == *category)
        );
    }
    fs::write(
        dir.path().join("日本語 a.md"),
        "設定を、保存することができます。\n",
    )
    .unwrap();
    fs::write(dir.path().join("b.md"), "設定を保存できます。\n").unwrap();
    let reports = json(
        dir.path(),
        &["lint", "日本語 a.md", "b.md", "--json", "--no-config"],
    );
    let summary = json(
        dir.path(),
        &[
            "lint",
            "日本語 a.md",
            "b.md",
            "--summary",
            "json",
            "--no-config",
        ],
    );
    let expected: usize = reports
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["findings"].as_array().unwrap().len())
        .sum();
    assert_eq!(summary["total"]["natural"], expected);
    assert_eq!(summary["files"][0]["file"], "日本語 a.md");
    assert_eq!(
        summary["files"][0]["natural"],
        reports[0]["findings"].as_array().unwrap().len()
    );
    assert_eq!(
        summary["categories"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["natural"].as_u64().unwrap())
            .sum::<u64>(),
        expected as u64
    );
    // Windowsはファイル名の制御文字を許さない。
    #[cfg(not(windows))]
    {
        fs::write(dir.path().join("tab\tline\n.md"), "本文です。").unwrap();
        let output = cargo_bin_cmd!("suiko")
            .current_dir(dir.path())
            .args(["lint", "tab\tline\n.md", "--summary", "tsv", "--no-config"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let tsv = String::from_utf8(output.stdout).unwrap();
        assert!(tsv.contains("tab\\tline\\n.md"));
        assert!(tsv.lines().all(|line| line.split('\t').count() == 5));
    }
}

#[test]
fn summary_distinguishes_unmeasured_reading_load_from_zero_findings() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.md"), "設定を、保存してください。\n").unwrap();
    let summary = json(
        dir.path(),
        &["lint", "a.md", "--summary", "json", "--no-reading-load"],
    );
    assert_eq!(summary["reading_load_enabled"], false);
    assert!(summary["total"]["reading_load"].is_null());
    assert!(summary["files"][0]["reading_load"].is_null());
    assert!(
        summary["categories"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["reading_load"].is_null())
    );
    let output = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["lint", "a.md", "--summary", "tsv", "--no-reading-load"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .skip(1)
            .all(|line| line.ends_with('\t'))
    );
}

#[cfg(unix)]
#[test]
fn compare_rejects_symlink_content_in_either_revision() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    fs::write(dir.path().join("article.md"), "設定を保存できます。\n").unwrap();
    symlink("article.md", dir.path().join("link.md")).unwrap();
    git(dir.path(), &["add", "."]);
    git(
        dir.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-qm",
            "initial",
        ],
    );
    cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["compare", "--rev", "HEAD", "link.md", "--json"])
        .assert()
        .failure();
    fs::remove_file(dir.path().join("link.md")).unwrap();
    fs::write(dir.path().join("link.md"), "設定を保存できます。\n").unwrap();
    cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["compare", "--rev", "HEAD", "link.md", "--json"])
        .assert()
        .failure();
    symlink("article.md", dir.path().join("new-link.md")).unwrap();
    cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["compare", "--rev", "HEAD", "new-link.md", "--json"])
        .assert()
        .failure();
}

#[test]
fn compare_tracks_renames_additions_deletions_and_both_lanes_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    let nested = Path::new("日本語 docs").join("手順.md");
    fs::create_dir(dir.path().join("日本語 docs")).unwrap();
    fs::write(dir.path().join(&nested), "ログを、保存してください。\n").unwrap();
    fs::write(
        dir.path().join("元の文書.md"),
        "設定を、保存することができます。\n",
    )
    .unwrap();
    fs::write(dir.path().join("削除.md"), "設定を、保存してください。\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(
        dir.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-qm",
            "initial",
        ],
    );
    git(dir.path(), &["mv", "元の文書.md", "新しい 文書.md"]);
    // Git identifies the staged rename even when the working text changes entirely.
    fs::write(dir.path().join("新しい 文書.md"), "設定を保存できます。\n").unwrap();
    fs::remove_file(dir.path().join("削除.md")).unwrap();
    fs::write(
        dir.path().join("追加.md"),
        "ログを、保存することができます。\n",
    )
    .unwrap();
    let result = json(
        dir.path(),
        &[
            "compare",
            "--rev",
            "HEAD",
            "新しい 文書.md",
            "削除.md",
            "追加.md",
            "--json",
            "--no-config",
        ],
    );
    assert_eq!(result["files"][0]["before_file"], "元の文書.md");
    assert_eq!(result["files"][0]["file_status"], "renamed");
    assert_eq!(result["files"][1]["file_status"], "deleted");
    assert_eq!(result["files"][2]["file_status"], "added");
    assert!(result["total"]["natural"]["resolved"].as_u64().unwrap() > 0);
    assert!(result["total"]["natural"]["new"].as_u64().unwrap() > 0);
    assert!(result["total"]["reading_load"]["before"].is_number());
    let nested_result = json(
        dir.path(),
        &[
            "compare",
            "--rev",
            "HEAD",
            nested.to_str().unwrap(),
            "--json",
            "--no-config",
        ],
    );
    assert_eq!(
        nested_result["files"][0]["before_file"],
        "日本語 docs/手順.md"
    );
    assert_eq!(nested_result["files"][0]["file_status"], "matched");
    assert!(
        nested_result["total"]["natural"]["before"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("新しい 文書.md")).unwrap(),
        "設定を保存できます。\n"
    );
    cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["compare", "--rev", "--help", "追加.md", "--json"])
        .assert()
        .failure();
    cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["compare", "--rev", "HEAD", "missing.md", "--json"])
        .assert()
        .failure();
}

#[test]
fn style_profile_uses_only_selected_sources_and_reports_observations() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("a.md"),
        "設定を読み込みます。値を保存します。\n",
    )
    .unwrap();
    fs::write(dir.path().join("b.md"), "ログを確認した。\n").unwrap();
    let profile = json(dir.path(), &["style-profile", "a.md", "b.md", "--json"]);
    assert_eq!(profile["format"], "suiko-style-profile");
    assert_eq!(profile["sources"].as_array().unwrap().len(), 2);
    assert_eq!(profile["observations"]["sentences"], 3);
    assert_eq!(profile["observations"]["polite_sentences"], 2);
    assert_eq!(profile["sources"][0]["sha256"].as_str().unwrap().len(), 64);
    fs::write(
        dir.path().join("profile.json"),
        serde_json::to_vec(&profile).unwrap(),
    )
    .unwrap();
    let compared = json(
        dir.path(),
        &[
            "style-profile",
            "b.md",
            "--against",
            "profile.json",
            "--json",
        ],
    );
    assert!(
        compared["comparison"]["polite_ratio_delta"]
            .as_f64()
            .unwrap()
            < 0.0
    );
    fs::write(dir.path().join("bad.json"), "{\"format\":\"unrelated\"}").unwrap();
    cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["style-profile", "a.md", "--against", "bad.json", "--json"])
        .assert()
        .failure();
}
