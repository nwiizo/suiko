//! 改稿前後の指摘の対応づけ（compare・check）と、まず直す候補の出力を固定する。

use std::{fs, path::Path, process::Command};

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;

const BEFORE: &str = "# 概要\n\nその変更は大きな違いを作ります。\n\nもちろん、速いです。\n";
const AFTER: &str = "# 概要\n\n前提を一段落足しました。\n\nその新しい変更は大きな違いを作ってくれます。\n\n本体の質感も上質です。\n";

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

fn natural<'a>(file: &'a Value, status: &str) -> Vec<&'a Value> {
    file["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["lane"] == "natural" && item["status"] == status)
        .collect()
}

#[test]
fn compare_follows_edited_sentences_and_lists_individual_findings() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    fs::write(dir.path().join("article.md"), BEFORE).unwrap();
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
    fs::write(dir.path().join("article.md"), AFTER).unwrap();
    let base = [
        "compare",
        "--rev",
        "HEAD",
        "article.md",
        "--no-config",
        "--json",
    ];

    let all = json(dir.path(), &[&base[..], &["--findings", "all"]].concat());
    let file = &all["files"][0];
    let kept = natural(file, "persisting");
    let calque = kept
        .iter()
        .find(|item| item["category"] == "calque_phrase")
        .unwrap_or_else(|| panic!("{file}"));
    // 行が2つずれ、文も書き換わったが、同じ文の指摘として続く。
    assert_eq!(calque["before_line"], 3);
    assert_eq!(calque["after_line"], 5);
    assert!(
        ["same_sentence", "edited_sentence"].contains(&calque["basis"].as_str().unwrap()),
        "{calque}"
    );
    assert!(
        natural(file, "resolved")
            .iter()
            .any(|item| item["category"] == "short_concession" && item["after_line"].is_null())
    );
    assert!(
        natural(file, "new")
            .iter()
            .any(|item| item["category"] == "thin_evaluation" && item["before_line"].is_null())
    );
    // 一覧と集計の件数が一致する。
    for (status, key) in [
        ("persisting", "persisting"),
        ("resolved", "resolved"),
        ("new", "new"),
    ] {
        assert_eq!(
            natural(file, status).len() as u64,
            file["natural"][key].as_u64().unwrap(),
            "{status}"
        );
    }

    let only_new = json(dir.path(), &[&base[..], &["--findings", "new"]].concat());
    assert!(
        only_new["files"][0]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["status"] == "new")
    );
    // --findings を付けなければ従来の集計JSONのまま。
    let plain = json(dir.path(), &base);
    assert!(plain["files"][0].get("findings").is_none());
    cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args([
            "compare",
            "--rev",
            "HEAD",
            "article.md",
            "--findings",
            "all",
        ])
        .assert()
        .failure();
}

#[test]
fn check_records_where_kept_findings_came_from() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("before.md"), BEFORE).unwrap();
    fs::write(dir.path().join("final.md"), AFTER).unwrap();
    let result = json(
        dir.path(),
        &[
            "check",
            "final.md",
            "--before",
            "before.md",
            "--no-config",
            "--json",
        ],
    );
    let changes = &result["check"]["report"]["finding_changes"];
    let kept = changes["kept"]
        .as_array()
        .unwrap()
        .iter()
        .find(|key| key["category"] == "calque_phrase")
        .unwrap_or_else(|| panic!("{changes}"));
    assert_eq!(kept["line"], 5);
    assert_eq!(kept["before_line"], 3);
    assert!(kept["basis"].is_string());
    assert!(
        changes["introduced"]
            .as_array()
            .unwrap()
            .iter()
            .all(|key| key.get("basis").is_none())
    );
}

#[test]
fn check_keeps_a_finding_whose_excerpt_changed_with_the_sentence() {
    let dir = tempfile::tempdir().unwrap();
    // thin_evaluation は文全体を抜粋にするため、副詞を足すと抜粋も変わる。
    fs::write(dir.path().join("before.md"), "本体の質感も上質です。\n").unwrap();
    fs::write(
        dir.path().join("final.md"),
        "最初に一文を足しました。\n本体の質感もとても上質です。\n",
    )
    .unwrap();
    let result = json(
        dir.path(),
        &[
            "check",
            "final.md",
            "--before",
            "before.md",
            "--no-config",
            "--json",
        ],
    );
    let changes = &result["check"]["report"]["finding_changes"];
    let kept = &changes["kept"]
        .as_array()
        .unwrap()
        .iter()
        .find(|key| key["category"] == "thin_evaluation")
        .unwrap_or_else(|| panic!("{changes}"));
    assert_eq!(kept["basis"], "edited_sentence");
    assert_eq!(
        (kept["before_line"].as_u64(), kept["line"].as_u64()),
        (Some(1), Some(2))
    );
    assert!(
        !changes["resolved"]
            .as_array()
            .unwrap()
            .iter()
            .any(|key| key["category"] == "thin_evaluation")
    );
}

#[test]
fn lint_lists_first_fixes_by_severity_and_rule_confidence() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("article.md"),
        "その変更は大きな違いを作ります。\n本体の質感も上質です。\n",
    )
    .unwrap();
    let result = json(dir.path(), &["lint", "article.md", "--no-config", "--json"]);
    let fixes = result["first_fixes"].as_array().unwrap();
    assert!(!fixes.is_empty() && fixes.len() <= 5, "{result}");
    assert_eq!(fixes[0]["rank"], 1);
    assert!(
        fixes
            .iter()
            .all(|fix| ["high", "medium", "low"].contains(&fix["confidence"].as_str().unwrap()))
    );
    let text = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["lint", "article.md", "--no-config"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&text.stdout).contains("まず直す候補"));

    fs::write(dir.path().join("clean.md"), "設定を保存します。\n").unwrap();
    let clean = json(dir.path(), &["lint", "clean.md", "--no-config", "--json"]);
    assert!(clean.get("first_fixes").is_none(), "{clean}");
}
