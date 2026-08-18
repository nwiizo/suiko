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
        .stdout(predicate::str::contains("documents: human=2 ai=4"))
        .stdout(predicate::str::contains("fpr=0.000"))
        .stdout(predicate::str::contains("genre=essay human=2 ai=2"))
        .stdout(predicate::str::contains(
            "nominal_ending\thuman=0/2 fpr=0.000 ci95=0.000-0.658 low_n findings=0\tai=1/4 detection=0.250 ci95=0.046-0.699 low_n findings=1",
        ))
        .stdout(predicate::str::contains(
            "forbidden_phrase\thuman=0/2 fpr=0.000 ci95=0.000-0.658 low_n findings=0\tai=3/4 detection=0.750 ci95=0.301-0.954 low_n findings=19",
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
            "3,7",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("rule: repeated_sentence_lead"))
        .stdout(predicate::str::contains(
            "split=devのみで探索する。holdoutは閾値選定に使わない",
        ))
        .stdout(predicate::str::contains("value=3"))
        .stdout(predicate::str::contains("value=7"));
}

#[test]
fn sweep_reports_reading_load_rules_as_prevalence() {
    eval_command()
        .args([
            "sweep",
            "eval/corpus.toml",
            "--rule",
            "sentence-too-long",
            "--values",
            "110",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("rule: sentence_too_long"))
        .stdout(predicate::str::contains(
            "value=110 human=1/2 prevalence=0.500",
        ));
}

#[test]
fn labeled_reports_detection_and_fpr_per_category() {
    eval_command()
        .args(["labeled", "eval/corpus.toml"])
        .assert()
        .success()
        .stdout(predicate::str::contains("samples: total=43 categories=14"))
        .stdout(predicate::str::contains("ci=wilson95 low_n<5"))
        .stdout(predicate::str::contains("corpus: sha256="))
        .stdout(predicate::str::contains(
            "category=nominal_ending\tfire=1/1 detection=1.000 ci95=0.207-1.000 low_n\tsilent_fired=0/1 fpr=0.000 ci95=0.000-0.793 low_n",
        ))
        .stdout(predicate::str::contains(
            "category=low_lexical_diversity_ttr\tfire=1/1 detection=1.000 ci95=0.207-1.000 low_n\tsilent_fired=1/2 fpr=0.500 ci95=0.095-0.905 low_n",
        ))
        .stdout(predicate::str::contains("mismatches: 1"))
        .stdout(predicate::str::contains("mismatch id=low-ttr-silent-002"));
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

#[test]
fn manifest_rejects_a_sample_with_an_unknown_category() {
    let dir = tempdir().expect("temporary directory");
    fs::write(dir.path().join("document.md"), "実測した文書です。\n").expect("write document");
    fs::write(dir.path().join("sample.md"), "検証用の本文です。\n").expect("write sample");
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
sha256 = "73366d99dc2eff0d36a13b2f8bb3a403541298c3e0908a163676274fabbf6e3d"

[[sample]]
id = "sample-001"
path = "sample.md"
category = "no_such_rule"
expect = "fire"
"#,
    )
    .expect("write manifest");

    eval_command()
        .args([
            "labeled",
            dir.path().join("corpus.toml").to_str().expect("UTF-8 path"),
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("未知のルール"));
}
