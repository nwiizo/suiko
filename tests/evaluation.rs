use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

fn eval_command() -> Command {
    Command::cargo_bin("suiko-eval").expect("suiko-eval binary")
}

#[test]
fn report_summarizes_human_and_ai_documents_by_category() {
    eval_command()
        .args(["report", "eval/corpus.toml"])
        .assert()
        .success()
        .stdout(predicate::str::contains("documents: human=2 ai=1"))
        .stdout(predicate::str::contains("fpr=0.000"))
        .stdout(predicate::str::contains("detection=1.000"))
        .stdout(predicate::str::contains("genre=essay human=2 ai=1"))
        .stdout(predicate::str::contains(
            "nominal_ending\thuman=0/2 fpr=0.000 findings=0\tai=0/1 detection=0.000 findings=0",
        ))
        .stdout(predicate::str::contains(
            "forbidden_phrase\thuman=0/2 fpr=0.000 findings=0\tai=1/1 detection=1.000 findings=8",
        ))
        .stdout(predicate::str::contains(
            "lane=reading_load category=sentence_too_long\thuman=1/2 prevalence=0.500",
        ));
}

#[test]
fn sweep_compares_selected_thresholds_without_changing_the_manifest() {
    eval_command()
        .args([
            "sweep",
            "eval/corpus.toml",
            "--rule",
            "repeated-sentence-lead",
            "--values",
            "3,5,7",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("rule: repeated_sentence_lead"))
        .stdout(predicate::str::contains("value=3"))
        .stdout(predicate::str::contains("value=7"));
}

#[test]
fn length_analysis_reports_document_buckets_separately() {
    eval_command()
        .args(["length-analysis", "eval/corpus.toml"])
        .assert()
        .success()
        .stdout(predicate::str::contains("bucket=<1000"))
        .stdout(predicate::str::contains("bucket=1000-3999"))
        .stdout(predicate::str::contains("bucket=>=4000"))
        .stdout(predicate::str::contains(
            "bucket=>=4000 category=repeated_sentence_lead human=1/1 fpr=1.000",
        ))
        .stdout(predicate::str::contains(
            "bucket=>=4000 lane=reading_load category=sentence_too_long",
        ));
}

#[test]
fn manifest_rejects_content_that_does_not_match_its_hash() {
    let dir = tempdir().expect("temporary directory");
    fs::write(dir.path().join("document.md"), "実測した文書です。\n").expect("write document");
    fs::write(
        dir.path().join("corpus.toml"),
        r#"version = 1

[[document]]
id = "human-001"
path = "document.md"
label = "human"
genre = "essay"
source = "local fixture"
license = "MIT"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
"#,
    )
    .expect("write manifest");

    eval_command()
        .args([
            "report",
            dir.path().join("corpus.toml").to_str().expect("UTF-8 path"),
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("SHA-256"));
}
