use std::fs;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::Value;
use tempfile::tempdir;

fn draft(contents: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().expect("create temporary directory");
    let path = dir.path().join("draft.md");
    fs::write(&path, contents).expect("write draft");
    (dir, path)
}

#[test]
fn help_describes_the_three_analysis_commands() {
    cargo_bin_cmd!("suiko")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("lint"))
        .stdout(predicate::str::contains("outline"))
        .stdout(predicate::str::contains("terms"));
}

#[test]
fn lint_json_reports_findings_with_source_lines() {
    let (_dir, path) =
        draft("# 提案\n\n重要なのは、距離を克服することができる点だと言えるでしょう。\n");

    let output = cargo_bin_cmd!("suiko")
        .args(["lint", path.to_str().expect("UTF-8 path"), "--json"])
        .output()
        .expect("run suiko lint");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    assert_eq!(json["file"], path.to_string_lossy().as_ref());
    assert_eq!(json["findings"][0]["line"], 3);
    let categories = json["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .map(|finding| finding["category"].as_str().expect("category"))
        .collect::<Vec<_>>();
    assert!(categories.contains(&"forbidden_phrase"));
    assert!(categories.contains(&"translationese"));
}

#[test]
fn outline_json_extracts_headings_leads_and_bullets() {
    let (_dir, path) = draft("# 結論\n\n最初の文です。続きです。\n\n- 一つ\n- 二つ\n");

    let output = cargo_bin_cmd!("suiko")
        .args(["outline", path.to_str().expect("UTF-8 path"), "--json"])
        .output()
        .expect("run suiko outline");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    assert_eq!(json["outline"][0]["kind"], "heading");
    assert_eq!(json["outline"][0]["text"], "結論");
    assert_eq!(json["outline"][1]["kind"], "lead");
    assert_eq!(json["outline"][1]["text"], "最初の文です。");
    assert_eq!(json["outline"][2]["kind"], "bullets");
    assert_eq!(json["outline"][2]["text"], "(箇条書き 2 項目)");
}

#[test]
fn outline_heading_stats_are_stable_with_lindera_ipadic() {
    let output = cargo_bin_cmd!("suiko")
        .args(["outline", "tests/fixtures/outline-lindera.md", "--json"])
        .output()
        .expect("run suiko outline");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    let stats = &json["heading_stats"];
    assert_eq!(stats["total_headings"], 4);
    assert_eq!(stats["level_distribution"]["2"], 2);
    assert_eq!(stats["overall"]["length_mean"], 5.5);
    assert_eq!(stats["overall"]["length_cv"], 0.396);
    assert_eq!(stats["overall"]["nominal_ending_ratio"], 0.75);
    assert_eq!(stats["overall"]["dominant_pos_signature_ratio"], 0.5);
    assert_eq!(stats["overall"]["template_hits"][0]["matched"], "まとめ");
}

#[test]
fn terms_json_extracts_acronyms_and_katakana_terms_in_first_seen_order() {
    let (_dir, path) = draft("APIとは接続仕様です。クラウドサービスをAPIで呼びます。\n");

    let output = cargo_bin_cmd!("suiko")
        .args(["terms", path.to_str().expect("UTF-8 path"), "--json"])
        .output()
        .expect("run suiko terms");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    assert_eq!(json["terms"][0]["term"], "API");
    assert_eq!(json["terms"][0]["first_line"], 1);
    assert_eq!(json["terms"][0]["count"], 2);
    assert_eq!(json["terms"][0]["has_gloss_hint"], true);
    assert_eq!(json["terms"][1]["term"], "クラウドサービス");
}

#[test]
fn terms_ignore_tokenizer_noise_and_trim_middle_dots() {
    let (_dir, path) =
        draft("the of to problem wicked tame A B 巧 向 章。Rust APIと項目・ルールを確認します。\n");

    let output = cargo_bin_cmd!("suiko")
        .args(["terms", path.to_str().expect("UTF-8 path"), "--json"])
        .output()
        .expect("run suiko terms");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    let terms = json["terms"]
        .as_array()
        .expect("terms array")
        .iter()
        .map(|term| term["term"].as_str().expect("term"))
        .collect::<Vec<_>>();
    assert_eq!(terms, vec!["Rust", "API", "ルール"]);
}

#[test]
fn terms_context_can_find_a_gloss_marker_on_the_following_line() {
    let (_dir, path) = draft("APIを利用します。\nAPIとは接続仕様です。\n");

    let output = cargo_bin_cmd!("suiko")
        .args(["terms", path.to_str().expect("UTF-8 path"), "--json"])
        .output()
        .expect("run suiko terms");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    let api = &json["terms"][0];
    assert_eq!(api["term"], "API");
    assert_eq!(api["first_line"], 1);
    assert_eq!(api["has_gloss_hint"], true);
    assert!(
        api["context"]
            .as_str()
            .expect("context")
            .contains("APIとは接続仕様です。")
    );
}

#[test]
fn lint_does_not_treat_a_nominalizing_no_as_an_inanimate_subject() {
    let (_dir, path) = draft("見るべきなのは、「自分だけ少し」を生み出します。\n");

    let output = cargo_bin_cmd!("suiko")
        .args(["lint", path.to_str().expect("UTF-8 path"), "--json"])
        .output()
        .expect("run suiko lint");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    assert_eq!(
        json["stats"]["by_category"]["inanimate_subject_morph"],
        Value::Null
    );
}

#[test]
fn terms_ignore_heading_like_lines_inside_both_code_fence_styles() {
    let (_dir, path) =
        draft("# VisibleAPI\n\n```yaml\n# HiddenConfig\n```\n\n~~~yaml\n# OtherHidden\n~~~\n");

    let output = cargo_bin_cmd!("suiko")
        .args(["terms", path.to_str().expect("UTF-8 path"), "--json"])
        .output()
        .expect("run suiko terms");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    let terms = json["terms"].as_array().expect("terms array");
    let names = terms
        .iter()
        .map(|term| term["term"].as_str().expect("term"))
        .collect::<Vec<_>>();
    assert!(names.contains(&"VisibleAPI"));
    assert!(!names.contains(&"HiddenConfig"));
    assert!(!names.contains(&"OtherHidden"));
    let visible = terms
        .iter()
        .find(|term| term["term"] == "VisibleAPI")
        .expect("visible heading term");
    assert_eq!(visible["first_line"], 1);
}

#[test]
fn lint_ignores_embed_citation_lines_but_keeps_markdown_link_text() {
    let citations = (1..=7)
        .map(|index| format!("[https://example.com/{index}:embed:cite]"))
        .collect::<Vec<_>>()
        .join("\n");
    let (_dir, path) = draft(&format!(
        "{citations}\n[表示API](https://example.com)です。\n"
    ));

    let output = cargo_bin_cmd!("suiko")
        .args(["lint", path.to_str().expect("UTF-8 path"), "--json"])
        .output()
        .expect("run suiko lint");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    assert_eq!(json["stats"]["total_sentences"], 1);
    assert_eq!(
        json["stats"]["by_category"]["repeated_sentence_lead"],
        Value::Null
    );

    let terms_output = cargo_bin_cmd!("suiko")
        .args(["terms", path.to_str().expect("UTF-8 path"), "--json"])
        .output()
        .expect("run suiko terms");
    assert!(terms_output.status.success());
    let terms_json: Value =
        serde_json::from_slice(&terms_output.stdout).expect("valid JSON output");
    assert!(
        terms_json["terms"]
            .as_array()
            .expect("terms array")
            .iter()
            .any(|term| term["term"] == "API")
    );
}

#[test]
fn terms_ignore_inline_html_markup_but_keep_visible_text() {
    let (_dir, path) = draft(
        "<span style=\"font-size: 125%\" data-name=\"HiddenConfig\">APIとRustを説明します。</span>\n",
    );

    let output = cargo_bin_cmd!("suiko")
        .args(["terms", path.to_str().expect("UTF-8 path"), "--json"])
        .output()
        .expect("run suiko terms");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    let terms = json["terms"]
        .as_array()
        .expect("terms array")
        .iter()
        .map(|term| term["term"].as_str().expect("term"))
        .collect::<Vec<_>>();
    assert!(terms.contains(&"API"));
    assert!(terms.contains(&"Rust"));
    assert!(!terms.contains(&"span"));
    assert!(!terms.contains(&"style"));
    assert!(!terms.contains(&"HiddenConfig"));
}

#[test]
fn unreadable_input_is_an_execution_error() {
    cargo_bin_cmd!("suiko")
        .args(["lint", "missing.md"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("見つかりません"));
}

#[test]
fn lint_accepts_standard_input() {
    let output = cargo_bin_cmd!("suiko")
        .args(["lint", "-", "--json"])
        .write_stdin("重要なのは、実測値です。\n")
        .output()
        .expect("run suiko lint with stdin");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    assert_eq!(json["file"], "-");
    assert_eq!(json["findings"][0]["category"], "forbidden_phrase");
}

#[test]
fn multiple_files_are_a_json_array_of_compatible_reports() {
    let dir = tempdir().expect("create temporary directory");
    let first = dir.path().join("first.md");
    let second = dir.path().join("second.md");
    fs::write(&first, "重要なのは、実測値です。\n").expect("write first draft");
    fs::write(&second, "昨日、田中さんと確認した。\n").expect("write second draft");

    let output = cargo_bin_cmd!("suiko")
        .args([
            "lint",
            first.to_str().expect("UTF-8 path"),
            second.to_str().expect("UTF-8 path"),
            "--json",
        ])
        .output()
        .expect("run suiko lint for multiple files");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    let reports = json.as_array().expect("report array");
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0]["file"], first.to_string_lossy().as_ref());
    assert_eq!(reports[1]["file"], second.to_string_lossy().as_ref());
}

#[test]
fn fail_on_turns_selected_findings_into_a_ci_exit_code() {
    let (_dir, path) = draft("と言えるでしょう。\n");

    cargo_bin_cmd!("suiko")
        .args([
            "lint",
            path.to_str().expect("UTF-8 path"),
            "--fail-on",
            "warn",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("forbidden_phrase"))
        .stderr(predicate::str::is_empty());
}

#[test]
fn baseline_marks_persisting_findings_without_changing_the_base_shape() {
    let (dir, path) = draft("と言えるでしょう。\n");
    let baseline = dir.path().join("baseline.json");
    let first = cargo_bin_cmd!("suiko")
        .args(["lint", path.to_str().expect("UTF-8 path"), "--json"])
        .output()
        .expect("create baseline");
    assert!(first.status.success());
    fs::write(&baseline, first.stdout).expect("write baseline");

    let output = cargo_bin_cmd!("suiko")
        .args([
            "lint",
            path.to_str().expect("UTF-8 path"),
            "--json",
            "--baseline",
            baseline.to_str().expect("UTF-8 path"),
        ])
        .output()
        .expect("compare baseline");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    assert_eq!(json["baseline"]["summary"]["persisting"], 1);
    assert_eq!(json["baseline"]["summary"]["new"], 0);
    assert_eq!(json["findings"][0]["status"], "persisting");
}

#[test]
fn reading_load_is_reported_in_a_separate_json_lane() {
    let long_sentence = format!(
        "{}。\n",
        "この文には分割すべき情報が含まれています".repeat(8)
    );
    let (_dir, path) = draft(&long_sentence);

    let output = cargo_bin_cmd!("suiko")
        .args([
            "lint",
            path.to_str().expect("UTF-8 path"),
            "--json",
            "--reading-load",
        ])
        .output()
        .expect("run reading-load lane");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    assert_eq!(json["reading_load"]["stats"]["total"], 1);
    assert_eq!(
        json["reading_load"]["findings"][0]["category"],
        "sentence_too_long"
    );
}

#[test]
fn discovered_config_disables_rules_and_allows_a_matching_finding() {
    let (dir, path) = draft("重要なのは、実測値です。\n距離を克服することができる仕組みです。\n");
    fs::write(
        dir.path().join(".suiko.toml"),
        r#"version = 1
disabled_rules = ["translationese", "translationese_morph"]

[[allow]]
category = "forbidden_phrase"
text = "重要なのは"
reason = "連載固有の見出し"
"#,
    )
    .expect("write config");

    let output = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["lint", path.to_str().expect("UTF-8 path"), "--json"])
        .output()
        .expect("run suiko lint with discovered config");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    assert_eq!(json["stats"]["total_findings"], 0);
    assert_eq!(json["stats"]["by_category"], serde_json::json!({}));
}

#[test]
fn config_can_disable_a_reading_load_rule() {
    let long_sentence = format!(
        "{}。\n",
        "この文には分割すべき情報が含まれています".repeat(8)
    );
    let (dir, path) = draft(&long_sentence);
    fs::write(
        dir.path().join(".suiko.toml"),
        "version = 1\ndisabled_rules = [\"sentence_too_long\"]\n",
    )
    .expect("write config");

    let output = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args([
            "lint",
            path.to_str().expect("UTF-8 path"),
            "--json",
            "--reading-load",
        ])
        .output()
        .expect("run suiko lint with reading-load config");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    assert_eq!(json["reading_load"]["stats"]["total"], 0);
    assert_eq!(
        json["reading_load"]["stats"]["by_category"],
        serde_json::json!({})
    );
}

#[test]
fn command_line_genre_and_fail_on_override_config_defaults() {
    let (dir, path) = draft("と言えるでしょう。\n");
    fs::write(
        dir.path().join(".suiko.toml"),
        "version = 1\ngenre = \"tech\"\nfail_on = \"critical\"\n",
    )
    .expect("write config");

    let configured = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["lint", path.to_str().expect("UTF-8 path"), "--json"])
        .output()
        .expect("run suiko lint with config defaults");
    assert!(configured.status.success());
    let configured_json: Value =
        serde_json::from_slice(&configured.stdout).expect("valid JSON output");
    assert_eq!(configured_json["stats"]["genre"], "tech");

    let overridden = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args([
            "lint",
            path.to_str().expect("UTF-8 path"),
            "--json",
            "--genre",
            "essay",
            "--fail-on",
            "warn",
        ])
        .output()
        .expect("run suiko lint with CLI overrides");
    assert_eq!(overridden.status.code(), Some(2));
    let overridden_json: Value =
        serde_json::from_slice(&overridden.stdout).expect("valid JSON output");
    assert_eq!(overridden_json["stats"]["genre"], "essay");
}

#[test]
fn explicit_config_overrides_discovery_and_no_config_skips_discovery() {
    let (dir, path) = draft("と言えるでしょう。\n");
    fs::write(
        dir.path().join(".suiko.toml"),
        "version = 1\ndisabled_rules = [\"forbidden_phrase\"]\n",
    )
    .expect("write discovered config");
    fs::write(
        dir.path().join("alternate.toml"),
        "version = 1\ngenre = \"business\"\n",
    )
    .expect("write explicit config");

    let explicit = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args([
            "lint",
            path.to_str().expect("UTF-8 path"),
            "--json",
            "--config",
            "alternate.toml",
        ])
        .output()
        .expect("run suiko lint with explicit config");
    assert!(explicit.status.success());
    let explicit_json: Value = serde_json::from_slice(&explicit.stdout).expect("valid JSON output");
    assert_eq!(explicit_json["stats"]["genre"], "business");
    assert_eq!(explicit_json["stats"]["total_findings"], 1);

    let without_config = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args([
            "lint",
            path.to_str().expect("UTF-8 path"),
            "--json",
            "--no-config",
        ])
        .output()
        .expect("run suiko lint without config");
    assert!(without_config.status.success());
    let without_config_json: Value =
        serde_json::from_slice(&without_config.stdout).expect("valid JSON output");
    assert_eq!(without_config_json["stats"]["genre"], Value::Null);
    assert_eq!(without_config_json["stats"]["total_findings"], 1);
}

#[test]
fn invalid_config_is_an_execution_error() {
    let (dir, path) = draft("と言えるでしょう。\n");
    fs::write(
        dir.path().join(".suiko.toml"),
        "version = 1\ndisabled_rules = [\"unknown_rule\"]\n",
    )
    .expect("write invalid config");

    cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["lint", path.to_str().expect("UTF-8 path")])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("unknown_rule"));
}

#[test]
fn config_rejects_unknown_keys_and_versions() {
    let (dir, path) = draft("と言えるでしょう。\n");
    let config = dir.path().join(".suiko.toml");
    fs::write(&config, "version = 1\nunknown = true\n").expect("write unknown key config");

    cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["lint", path.to_str().expect("UTF-8 path")])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("unknown"));

    fs::write(&config, "version = 2\n").expect("write unsupported version config");
    cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["lint", path.to_str().expect("UTF-8 path")])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("version = 2"));
}
