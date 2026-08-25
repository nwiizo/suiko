use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use serde_json::Value;

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
            "version": "0.3.3-remedy.1",
            "commit": commit,
        })
    );
    assert_eq!(value.as_object().unwrap().len(), 3);
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
