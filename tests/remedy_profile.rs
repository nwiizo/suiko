use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use serde_json::{Value, json};

const REMEDY_ARGS: &[&str] = &[
    "lint",
    "--profile",
    "remedy-seo",
    "--input-format",
    "html",
    "--format",
    "json",
    "--redact-excerpts",
    "-",
];

const REMEDY_FILLER_ARGS: &[&str] = &[
    "lint",
    "--profile",
    "remedy-seo-filler",
    "--input-format",
    "html",
    "--format",
    "json",
    "--redact-excerpts",
    "-",
];

const REMEDY_ADVISORY_ARGS: &[&str] = &[
    "lint",
    "--profile",
    "remedy-seo-advisory",
    "--input-format",
    "html",
    "--format",
    "json",
    "--redact-excerpts",
    "-",
];

const REMEDY_LLM_PACKET_ARGS: &[&str] = &[
    "lint",
    "--profile",
    "remedy-seo-llm-packet",
    "--input-format",
    "html",
    "--format",
    "json",
    "--redact-excerpts",
    "-",
];

fn suiko() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("suiko"))
}

#[test]
fn version_json_has_exact_identity_schema() {
    let commit = env!("SUIKO_REMEDY_COMMIT");
    let output = suiko()
        .arg("--version-json")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "name": "suiko-remedy",
            "version": "0.3.3-remedy.2",
            "commit": commit,
        })
    );
    assert_eq!(value.as_object().unwrap().len(), 3);
}

#[test]
fn filler_profile_ignores_masu_and_translationese_only_input() {
    let html = "<p>進めます。確認します。整えます。終えます。</p>\
                <p>一方で、Aです。</p><p>一方で、Bです。</p><p>一方で、Cです。</p>";
    suiko()
        .args(REMEDY_FILLER_ARGS)
        .write_stdin(html)
        .assert()
        .success()
        .stdout("{\"schema_version\":\"1\",\"findings\":[]}\n");
}

#[test]
fn filler_profile_emits_only_filler_and_returns_two() {
    let output = suiko()
        .args(REMEDY_FILLER_ARGS)
        .write_stdin(format!("<p>{}</p>", "必要があります。".repeat(8)))
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["schema_version"], "1");
    assert_eq!(value["findings"].as_array().unwrap().len(), 1);
    assert_eq!(value["findings"][0]["rule_id"], "filler");
    assert_eq!(value["findings"][0]["category"], "readability");
    assert_eq!(value["findings"][0]["severity"], "warn");
    assert_eq!(value["findings"][0].as_object().unwrap().len(), 4);
}

#[test]
fn clean_html_returns_zero_and_exact_empty_schema() {
    suiko()
        .args(REMEDY_ARGS)
        .write_stdin("<h2>見出し</h2><p>簡潔な本文です。</p>")
        .assert()
        .success()
        .stdout("{\"schema_version\":\"1\",\"findings\":[]}\n");
}

#[test]
fn finding_returns_two_is_sorted_and_never_leaks_evidence() {
    let secret = "秘密の固有本文";
    let html = format!(
        "<p>{secret}を確認します。進めます。比較します。整理します。</p>\
         <p>ことが可能です。必要があります。必要があります。必要があります。\
         必要があります。必要があります。必要があります。必要があります。必要があります。</p>"
    );
    let output = suiko()
        .args(REMEDY_ARGS)
        .write_stdin(html)
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let rendered = String::from_utf8(output.clone()).unwrap();
    assert!(!rendered.contains(secret));
    assert!(!rendered.contains("必要があります"));
    let value: Value = serde_json::from_slice(&output).unwrap();
    let findings = value["findings"].as_array().unwrap();
    let ids = findings
        .iter()
        .map(|item| item["rule_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["filler", "masu-streak", "translationese"]);
    for item in findings {
        assert_eq!(item.as_object().unwrap().len(), 4);
        let hash = item["evidence_sha256"].as_str().unwrap();
        assert_eq!(hash.len(), 64);
        assert!(
            hash.bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
    }
}

#[test]
fn hidden_protected_and_swell_content_cannot_trigger_findings() {
    let repeated = "します。します。します。します。必要があります。必要があります。必要があります。必要があります。必要があります。必要があります。必要があります。必要があります。";
    let html = format!(
        "<table><tbody><tr><td><p>{repeated}</p></td></tr></tbody></table>\
         <blockquote><p>{repeated}</p></blockquote><div aria-hidden='true'><p>{repeated}</p></div>\
         <div class='p-blogParts'><p>{repeated}</p></div><script><p>{repeated}</p></script>"
    );
    suiko()
        .args(REMEDY_ARGS)
        .write_stdin(html)
        .assert()
        .success();
}

#[test]
fn malformed_html_entities_and_nested_lists_are_deterministic() {
    let html = "<p>A&amp;Bです。<ul><li>親です。<ul><li>します。します。します。します。</li></ul>";
    let first = suiko()
        .args(REMEDY_ARGS)
        .write_stdin(html)
        .output()
        .unwrap();
    let second = suiko()
        .args(REMEDY_ARGS)
        .write_stdin(html)
        .output()
        .unwrap();
    assert_eq!(first.status.code(), second.status.code());
    assert_eq!(first.stdout, second.stdout);
    assert_eq!(first.stderr, second.stderr);
}

#[test]
fn remedy_contract_rejects_files_and_nonexact_options_as_infra() {
    suiko()
        .args([
            "lint",
            "--profile",
            "remedy-seo",
            "--input-format",
            "html",
            "--format",
            "json",
            "--redact-excerpts",
            "README.md",
        ])
        .assert()
        .code(1);
    suiko()
        .args([
            "lint",
            "--profile",
            "remedy-seo",
            "--input-format",
            "html",
            "--format",
            "json",
            "--redact-excerpts",
            "--no-config",
            "-",
        ])
        .assert()
        .code(1);
}

#[test]
fn remedy_rejects_more_than_input_limit_without_echoing_input() {
    let oversized = vec![b'x'; 256 * 1024 + 1];
    suiko()
        .args(REMEDY_ARGS)
        .write_stdin(oversized)
        .assert()
        .code(1)
        .stdout("");
}

#[test]
fn remedy_rejects_deep_markup_before_html_parsing() {
    suiko()
        .args(REMEDY_ARGS)
        .write_stdin("<div>".repeat(4_097))
        .assert()
        .code(1)
        .stdout("")
        .stderr(contains("too many markup openers"));
}

#[test]
fn ordinary_upstream_version_is_unchanged() {
    suiko()
        .arg("--version")
        .assert()
        .success()
        .stdout("suiko 0.3.3\n");
}

#[test]
fn ordinary_no_arg_cli_keeps_clap_usage_and_exit_two() {
    suiko()
        .assert()
        .code(2)
        .stderr(contains("Usage:").and(contains("a subcommand is required")));
}

#[test]
fn advisory_is_exact_allowlisted_redacted_and_nonblocking() {
    let secret = "機密顧客名アルファ";
    let html = format!(
        "<p>{secret}の結合部分の検証を行います。</p>\
         <p>この方針は実装判断の羅針盤になります。</p>\
         <p>重要なのは距離を克服することができる点だと言えるでしょう。</p>"
    );
    let output = suiko()
        .args(REMEDY_ADVISORY_ARGS)
        .write_stdin(html)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rendered = String::from_utf8(output.clone()).unwrap();
    assert!(!rendered.contains(secret));
    assert!(!rendered.contains("検証を行います"));
    assert!(!rendered.contains("羅針盤"));
    assert!(!rendered.contains("克服"));
    let value: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["schema_version"], "1");
    assert_eq!(value["suiko_version"], "0.3.3");
    assert_eq!(value["commit"], env!("SUIKO_REMEDY_COMMIT"));
    assert_eq!(value["profile"], "remedy-seo-advisory");
    assert_eq!(value["source"]["format"], "html");
    assert_eq!(value["source"]["block_count"], 3);
    assert_eq!(value["rules"], json!(["redundant_light_verb"]));
    let findings = value["findings"].as_array().unwrap();
    assert!(!findings.is_empty());
    for finding in findings {
        assert_eq!(finding["severity"], "info");
        assert_eq!(finding.as_object().unwrap().len(), 4);
        assert!(
            value["rules"]
                .as_array()
                .unwrap()
                .contains(&finding["rule"])
        );
        assert_eq!(finding["rule"], "redundant_light_verb");
        assert_ne!(finding["rule"], "translationese");
        assert_ne!(finding["rule"], "sentence_too_long");
    }
}

#[test]
fn advisory_visible_extraction_handles_entities_breaks_malformed_and_nested_lists() {
    let html = "<!-- <p>コメント</p> --><p>A&amp;B<br>短い文です。<p>壊れた段落\
        <table><tbody><tr><td><p>表の秘密</p></td></tr></tbody></table><div hidden><p>隠した秘密</p></div>\
        <div class='p-blogParts'><p>CTAの秘密</p></div>\
        <ul><li>親項目<ul><li>子項目</li></ul></li></ul>";
    let output = suiko()
        .args(REMEDY_ADVISORY_ARGS)
        .write_stdin(html)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rendered = String::from_utf8(output.clone()).unwrap();
    for secret in ["コメント", "表の秘密", "隠した秘密", "CTAの秘密"] {
        assert!(!rendered.contains(secret));
    }
    let value: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["source"]["block_count"], 4);
    let expected_visible = "A&B 短い文です。\n壊れた段落\n親項目\n子項目";
    assert_eq!(
        value["source"]["visible_sha256"],
        suiko::remedy::sha256_hex(expected_visible.as_bytes())
    );
}

#[test]
fn advisory_output_is_byte_identical_and_evidence_is_document_bound() {
    let html = "<p>結合部分の検証を行います。</p>";
    let run = |input: &str| {
        suiko()
            .args(REMEDY_ADVISORY_ARGS)
            .write_stdin(input)
            .output()
            .unwrap()
    };
    let first = run(html);
    let second = run(html);
    assert!(first.status.success());
    assert_eq!(first.stdout, second.stdout);
    assert_eq!(first.stderr, second.stderr);

    let other = run("<p>前置きです。</p><p>結合部分の検証を行います。</p>");
    let first: Value = serde_json::from_slice(&first.stdout).unwrap();
    let other: Value = serde_json::from_slice(&other.stdout).unwrap();
    assert_ne!(
        first["findings"][0]["evidence_sha256"],
        other["findings"][0]["evidence_sha256"]
    );
}

#[test]
fn llm_packet_is_bounded_deterministic_and_has_no_source_identity() {
    let html = "<p>東京の本社の営業部の担当者が、結合部分の検証を行います。</p>";
    let run = || {
        suiko()
            .args(REMEDY_LLM_PACKET_ARGS)
            .write_stdin(html)
            .output()
            .unwrap()
    };
    let first = run();
    let second = run();
    assert!(first.status.success());
    assert_eq!(first.stdout, second.stdout);
    let rendered = String::from_utf8(first.stdout.clone()).unwrap();
    assert!(!rendered.contains("05_final.html"));
    assert!(!rendered.contains("article_id"));
    let value: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(value["profile"], "remedy-seo-llm-packet");
    assert_eq!(value["per_rule_limit"], 2);
    assert!(value["summary"]["candidate_total"].as_u64().unwrap() <= 16);
    assert_eq!(
        value["summary"]["candidate_total"].as_u64().unwrap() as usize,
        value["candidates"].as_array().unwrap().len()
    );
    assert!(
        value["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .all(|candidate| {
                candidate["candidate_id"].as_str().unwrap().len() == 64
                    && candidate["context"]["target"].is_string()
                    && candidate["context"]["target_truncated"].is_boolean()
            })
    );
}

#[test]
fn advisory_contract_and_invalid_input_fail_as_infrastructure() {
    suiko()
        .args([
            "lint",
            "--profile",
            "remedy-seo-advisory",
            "--input-format",
            "html",
            "--format",
            "json",
            "--redact-excerpts",
            "README.md",
        ])
        .assert()
        .code(1);
    suiko()
        .args(REMEDY_ADVISORY_ARGS)
        .write_stdin([0xff, 0xfe])
        .assert()
        .code(1)
        .stdout("");
    suiko()
        .args(REMEDY_ADVISORY_ARGS)
        .write_stdin(vec![b'x'; 256 * 1024 + 1])
        .assert()
        .code(1)
        .stdout("");
    suiko()
        .args(REMEDY_ADVISORY_ARGS)
        .write_stdin("<div>".repeat(4_097))
        .assert()
        .code(1)
        .stdout("");
}
