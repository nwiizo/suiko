use std::fs;

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;

fn check(dir: &tempfile::TempDir, extra: &[&str]) -> Value {
    let output = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["check", "final.md", "--before", "before.md", "--json"])
        .args(extra)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn verify(dir: &tempfile::TempDir, extra: &[&str], code: i32) -> Value {
    let output = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["verify", "final.md.suiko-check.json", "--json"])
        .args(extra)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(code),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn drafts() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("before.md"),
        "設定を、保存して下さい。失敗時だけ最大3回再試行できます。\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("final.md"),
        "設定を保存してください。失敗時だけ最大3回再試行できます。\n",
    )
    .unwrap();
    dir
}

#[test]
fn check_records_all_checks_and_verify_can_replay_from_another_directory() {
    let dir = drafts();
    let result = check(&dir, &["--genre", "tech"]);
    assert_eq!(result["status"], "checked");
    assert_eq!(result["check"]["options"]["genre"], "tech");
    assert!(result["check"]["report"]["lint"]["findings"].is_array());
    assert!(result["check"]["report"]["reading_load"]["findings"].is_array());
    assert!(result["check"]["report"]["diff"]["findings"].is_array());
    assert!(result["check"]["report"]["materials"].is_null());
    let unchanged = fs::read_to_string(dir.path().join("final.md")).unwrap();
    assert_eq!(verify(&dir, &["--replay"], 0)["status"], "current");
    let other = tempfile::tempdir().unwrap();
    cargo_bin_cmd!("suiko")
        .current_dir(other.path())
        .arg("verify")
        .arg(dir.path().join("final.md.suiko-check.json"))
        .arg("--replay")
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(dir.path().join("final.md")).unwrap(),
        unchanged
    );
}

#[test]
fn verify_rejects_changed_original_final_material_and_config() {
    for (file, extra) in [
        ("before.md", vec![]),
        ("final.md", vec![]),
        ("material.md", vec!["--material", "material.md"]),
        (".suiko.toml", vec![]),
    ] {
        let dir = drafts();
        fs::write(dir.path().join("material.md"), "最大3回再試行できます。\n").unwrap();
        fs::write(
            dir.path().join(".suiko.toml"),
            "version = 1\ngenre = 'tech'\n",
        )
        .unwrap();
        check(&dir, &extra);
        fs::write(dir.path().join(file), "changed\n").unwrap();
        let result = verify(&dir, &[], 2);
        assert_eq!(result["status"], "stale", "{file}");
        assert!(!result["changes"].as_array().unwrap().is_empty());
    }
}

#[test]
fn verify_detects_new_auto_config_missing_input_and_record_corruption() {
    let dir = drafts();
    check(&dir, &[]);
    fs::write(dir.path().join(".suiko.toml"), "version = 1\n").unwrap();
    assert_eq!(verify(&dir, &[], 2)["status"], "stale");
    check(&dir, &["--no-config"]);
    fs::write(dir.path().join(".suiko.toml"), "version = 2\n").unwrap();
    assert_eq!(verify(&dir, &[], 0)["status"], "current");
    fs::remove_file(dir.path().join("before.md")).unwrap();
    assert_eq!(verify(&dir, &[], 2)["status"], "stale");
    let path = dir.path().join("final.md.suiko-check.json");
    let mut stored: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    stored["record"]["report"]["lint"]["findings"] = serde_json::json!([]);
    stored["record"]["options"]["genre"] = serde_json::json!("business");
    fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();
    cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["verify", "final.md.suiko-check.json"])
        .assert()
        .code(1);
}

#[test]
fn check_preserves_inputs_and_unrelated_existing_record_targets() {
    let dir = drafts();
    for path in ["final.md", "before.md", "notes.json"] {
        if path == "notes.json" {
            fs::write(dir.path().join(path), "{\"notes\": 1}").unwrap();
        }
        let original = fs::read(dir.path().join(path)).unwrap();
        cargo_bin_cmd!("suiko")
            .current_dir(dir.path())
            .args([
                "check",
                "final.md",
                "--before",
                "before.md",
                "--record",
                path,
            ])
            .assert()
            .code(1);
        assert_eq!(fs::read(dir.path().join(path)).unwrap(), original);
    }
}

#[test]
fn record_cannot_create_an_auto_config_through_an_alias_path() {
    let dir = drafts();
    fs::create_dir(dir.path().join("child")).unwrap();
    let output = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args([
            "check",
            "final.md",
            "--record",
            "child/../.suiko.toml",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(!dir.path().join(".suiko.toml").exists());
}

#[test]
fn check_uses_custom_rules_and_persists_failure_threshold() {
    let dir = drafts();
    fs::write(dir.path().join(".suiko.toml"), "version = 1\ngenre = 'tech'\nfail_on = 'info'\n[[word_rules]]\nid = 'local'\ntokens = [{ surface = '保存' }]\nmessage = '保存先を確認'\n").unwrap();
    fs::write(dir.path().join("final.md"), "設定を、保存して下さい。\n").unwrap();
    let output = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["check", "final.md", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(dir.path().join("final.md.suiko-check.json").is_file());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["check"]["threshold_exceeded"], true);
    assert!(
        report["check"]["report"]["lint"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["category"] == "custom_wording/local")
    );
    assert_eq!(verify(&dir, &[], 2)["status"], "current");
}

#[test]
fn materials_compare_units_context_and_unconfirmed_claims_by_default() {
    let dir = drafts();
    fs::write(dir.path().join("material.md"), "検証環境の応答時間は300msでした。\n本番環境の応答時間は未測定です。\n処理件数は1万件です。\n").unwrap();
    fs::write(
        dir.path().join("final.md"),
        "本番環境の応答時間は300msです。\n処理件数は1万人です。\n処理性能は必ず向上します。\n",
    )
    .unwrap();
    let report = check(&dir, &["--genre", "tech", "--material", "material.md"]);
    let findings = report["check"]["report"]["materials"]["findings"]
        .as_array()
        .unwrap();
    for category in [
        "material_value_context",
        "material_value_unmatched",
        "material_caveat",
        "material_claim_unmatched",
    ] {
        assert!(
            findings.iter().any(|f| f["category"] == category),
            "missing {category}: {findings:?}"
        );
    }
    assert!(findings.iter().any(|f| {
        f["evidence"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|r| r["line"] == 2))
    }));
}

#[test]
fn materials_accept_supported_values_and_ignore_code_examples() {
    let dir = drafts();
    let text = "# 測定結果\n\n- 検証環境の応答時間は300msでした。\n- 本番環境の応答時間は未測定です。\n\n| 項目 | 結果 |\n| --- | --- |\n| 処理件数 | 1,000件 |\n";
    fs::write(dir.path().join("material.md"), text).unwrap();
    fs::write(
        dir.path().join("final.md"),
        format!(
            "{}\n```text\n性能が必ず99倍になります。\n```\n<!-- 999件 -->\n",
            text.replace("1,000", "１０００")
        ),
    )
    .unwrap();
    let report = check(&dir, &["--material", "material.md"]);
    assert!(
        report["check"]["report"]["materials"]["findings"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{report}"
    );
}

#[test]
fn verify_detects_a_different_executable_and_replay_detects_a_different_result() {
    let dir = drafts();
    check(&dir, &[]);
    let path = dir.path().join("final.md.suiko-check.json");
    let original: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    for (field, changed, replay, expected) in [
        (
            "tool_sha256",
            serde_json::json!("0".repeat(64)),
            false,
            "tool_changed",
        ),
        ("report", serde_json::json!({}), true, "result_changed"),
    ] {
        let mut stored = original.clone();
        stored["record"][field] = changed;
        stored["checksum"] = serde_json::json!(suiko::diff::sha256_hex(
            &serde_json::to_string(&stored["record"]).unwrap()
        ));
        fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();
        let args = if replay { vec!["--replay"] } else { vec![] };
        let result = verify(&dir, &args, 2);
        assert!(
            result["changes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|change| change["kind"] == expected)
        );
    }
}

#[test]
fn failed_input_and_failed_save_do_not_produce_success_or_replace_an_existing_record() {
    let dir = drafts();
    check(&dir, &[]);
    let path = dir.path().join("final.md.suiko-check.json");
    let original = fs::read(&path).unwrap();
    for extra in [
        vec!["--material", "missing.md"],
        vec!["--record", "missing/record.json"],
    ] {
        let output = cargo_bin_cmd!("suiko")
            .current_dir(dir.path())
            .args(["check", "final.md", "--json"])
            .args(extra)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert_eq!(fs::read(&path).unwrap(), original);
    }
    assert!(fs::read_dir(dir.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")
    }));
}

#[cfg(unix)]
#[test]
fn symlink_inputs_are_rechecked_and_symlink_outputs_are_rejected() {
    use std::os::unix::fs::symlink;
    let dir = drafts();
    fs::rename(dir.path().join("final.md"), dir.path().join("first.md")).unwrap();
    symlink("first.md", dir.path().join("final.md")).unwrap();
    check(&dir, &[]);
    fs::write(dir.path().join("second.md"), "設定を保存できません。\n").unwrap();
    fs::remove_file(dir.path().join("final.md")).unwrap();
    symlink("second.md", dir.path().join("final.md")).unwrap();
    assert_eq!(verify(&dir, &[], 2)["status"], "stale");
    symlink("before.md", dir.path().join("record-link.json")).unwrap();
    let original = fs::read(dir.path().join("before.md")).unwrap();
    cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["check", "final.md", "--record", "record-link.json"])
        .assert()
        .code(1);
    assert_eq!(fs::read(dir.path().join("before.md")).unwrap(), original);
}
#[test]
fn code_comment_edits_remain_protected_even_inside_lint_ignore_regions() {
    let dir = tempfile::tempdir().unwrap();
    let before = "<!-- suiko:ignore-start -->\n```sh\n# 設定を、保存する\nsave\n```\n<!-- suiko:ignore-end -->\n";
    let after = before.replace("設定を、", "設定を");
    fs::write(dir.path().join("before.md"), before).unwrap();
    fs::write(dir.path().join("final.md"), &after).unwrap();
    let output = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args([
            "check",
            "final.md",
            "--before",
            "before.md",
            "--fail-on",
            "warn",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        result["check"]["report"]["diff"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["category"] == "protected_changed")
    );
    assert!(dir.path().join("final.md.suiko-check.json").exists());
    assert_eq!(
        fs::read_to_string(dir.path().join("final.md")).unwrap(),
        after
    );
}
