use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

fn eval_command() -> Command {
    Command::cargo_bin("suiko-eval").expect("suiko-eval binary")
}

#[test]
fn fetch_downloads_html_and_records_a_compatible_lock_entry() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local HTTP server");
    let address = listener.local_addr().expect("local HTTP address");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept HTTP request");
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).expect("read HTTP request");
        let body = concat!(
            "<html><article>",
            "<h2>概要</h2>",
            "<p>第一段落です。</p>",
            "<p>第二段落&amp;詳細。</p>",
            "</article></html>",
        );
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .expect("write HTTP response");
    });

    let dir = tempdir().expect("temporary directory");
    let eval_dir = dir.path().join("eval");
    fs::create_dir_all(&eval_dir).expect("create eval directory");
    let sources_path = eval_dir.join("sources.toml");
    fs::write(
        &sources_path,
        format!(
            r#"version = 1

[[source]]
id = "local-html"
type = "web"
url = "http://{address}/article"
title = "テスト記事"
author = "テスト著者"
genre = "essay"
split = "dev"
"#,
        ),
    )
    .expect("write sources manifest");

    eval_command()
        .args([
            "fetch",
            sources_path.to_str().expect("UTF-8 path"),
            "--id",
            "local-html",
        ])
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains(
            "summary: 1 succeeded, 0 failed (total 1)",
        ));
    server.join().expect("join local HTTP server");

    let body = "## 概要\n\n第一段落です。\n\n第二段落&詳細。";
    let expected = format!(
        "---\nid: local-html\nsource_url: http://{address}/article\ntitle: \"テスト記事\"\nauthor: \"テスト著者\"\ngenre: essay\nextract_method: generic\nchars: 24\n---\n\n{body}\n"
    );
    let fetched_path = eval_dir.join("corpus/external/local-html.md");
    assert_eq!(
        fs::read_to_string(&fetched_path).expect("read fetched document"),
        expected
    );

    let lock: serde_json::Value = serde_json::from_slice(
        &fs::read(eval_dir.join("corpus/external-lock.json")).expect("read lock file"),
    )
    .expect("valid lock JSON");
    let entry = &lock["entries"]["local-html"];
    assert_eq!(lock["version"], 1);
    assert_eq!(entry["url"], format!("http://{address}/article"));
    assert_eq!(entry["chars"], 24);
    assert_eq!(entry["extract_method"], "generic");
    assert!(
        entry["sha256"]
            .as_str()
            .is_some_and(|value| value.len() == 64)
    );
    assert!(
        entry["fetched_at"]
            .as_str()
            .is_some_and(|value| value.ends_with('Z') && value.len() == 20)
    );
}

#[test]
fn fetch_preserves_completed_entries_when_a_later_document_cannot_be_written() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local HTTP server");
    let address = listener.local_addr().expect("local HTTP address");
    let server = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().expect("accept HTTP request");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).expect("read HTTP request");
            let body = "<html><article><p>取得本文です。</p></article></html>";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("write HTTP response");
        }
    });

    let dir = tempdir().expect("temporary directory");
    let eval_dir = dir.path().join("eval");
    let external_dir = eval_dir.join("corpus/external");
    fs::create_dir_all(external_dir.join("write-fails.md"))
        .expect("create directory at second output path");
    let sources_path = eval_dir.join("sources.toml");
    fs::write(
        &sources_path,
        format!(
            r#"version = 1

[[source]]
id = "write-succeeds"
type = "web"
url = "http://{address}/first"
title = "成功"
author = "著者"
genre = "essay"

[[source]]
id = "write-fails"
type = "web"
url = "http://{address}/second"
title = "失敗"
author = "著者"
genre = "essay"
"#,
        ),
    )
    .expect("write sources manifest");

    eval_command()
        .args(["fetch", sources_path.to_str().expect("UTF-8 path")])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("summary: 1 succeeded, 1 failed (total 2)").count(1));
    server.join().expect("join local HTTP server");

    let lock: serde_json::Value = serde_json::from_slice(
        &fs::read(eval_dir.join("corpus/external-lock.json")).expect("read lock file"),
    )
    .expect("valid lock JSON");
    assert!(lock["entries"]["write-succeeds"]["sha256"].is_string());
    assert!(lock["entries"]["write-fails"]["error"].is_string());
}

#[test]
fn fetch_reports_output_directory_creation_as_a_write_error() {
    let dir = tempdir().expect("temporary directory");
    let eval_dir = dir.path().join("eval");
    fs::create_dir_all(&eval_dir).expect("create eval directory");
    fs::write(eval_dir.join("corpus"), "not a directory").expect("create path blocker");
    let sources_path = eval_dir.join("sources.toml");
    fs::write(&sources_path, "version = 1\n").expect("write sources manifest");

    eval_command()
        .args(["fetch", sources_path.to_str().expect("UTF-8 path")])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("評価データを書き込めません"));
}

#[test]
fn report_summarizes_human_and_ai_documents_by_category() {
    eval_command()
        .args(["report", "eval/corpus.toml"])
        .assert()
        .success()
        .stdout(predicate::str::contains("documents: human=14 ai=4"))
        .stdout(predicate::str::contains("fpr=0.000"))
        .stdout(predicate::str::contains("genre=essay human=14 ai=2"))
        .stdout(predicate::str::contains(
            "nominal_ending\thuman=0/14 fpr=0.000 ci95=0.000-0.215 findings=0\tai=1/4 detection=0.250 ci95=0.046-0.699 low_n findings=1",
        ))
        .stdout(predicate::str::contains(
            "forbidden_phrase\thuman=6/14 fpr=0.429 ci95=0.214-0.674 findings=14\tai=3/4 detection=0.750 ci95=0.301-0.954 low_n findings=19",
        ))
        .stdout(predicate::str::contains(
            "lane=reading_load category=sentence_too_long\thuman=13/14 prevalence=0.929",
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
            "value=110 human=10/11 prevalence=0.909",
        ));
}

#[test]
fn labeled_reports_detection_and_fpr_per_category() {
    eval_command()
        .args(["labeled", "eval/corpus.toml"])
        .assert()
        .success()
        .stdout(predicate::str::contains("samples: total=89 categories=19"))
        .stdout(predicate::str::contains("ci=wilson95 low_n<5"))
        .stdout(predicate::str::contains("corpus: sha256="))
        .stdout(predicate::str::contains(
            "category=nominal_ending\tfire=1/1 detection=1.000 ci95=0.207-1.000 low_n\tsilent_fired=0/1 fpr=0.000 ci95=0.000-0.793 low_n",
        ))
        .stdout(predicate::str::contains(
            "category=low_lexical_diversity_ttr\tfire=1/1 detection=1.000 ci95=0.207-1.000 low_n\tsilent_fired=1/2 fpr=0.500 ci95=0.095-0.905 low_n",
        ))
        .stdout(predicate::str::contains(
            "category=abstract_metaphor\tfire=5/5 detection=1.000 ci95=0.566-1.000\tsilent_fired=0/9 fpr=0.000 ci95=0.000-0.299",
        ))
        .stdout(predicate::str::contains(
            "category=technical_jargon_metaphor\tfire=6/6 detection=1.000 ci95=0.610-1.000\tsilent_fired=0/8 fpr=0.000 ci95=0.000-0.324",
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
            "bucket=>=4000 category=repeated_sentence_lead human=0/11 fpr=0.000",
        ))
        .stdout(predicate::str::contains(
            "bucket=>=4000 lane=reading_load category=sentence_too_long",
        ));
}

// calibrate契約: 制約(人間fprのWilson上限)をfeasible判定に使い、dev splitのみで
// 探索する。分母が5未満の側があれば事前条件未達を明示する。
#[test]
fn calibrate_marks_feasibility_against_the_wilson_upper_bound() {
    eval_command()
        .args([
            "calibrate",
            "eval/corpus.toml",
            "--rule",
            "low-specificity",
            "--values=-0.15,-0.10",
            "--max-human-fpr-upper",
            "0.10",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "constraint: human fpr wilson95 upper <= 0.100",
        ))
        .stdout(predicate::str::contains("feasible="))
        .stdout(predicate::str::contains("事前条件未達"))
        .stdout(predicate::str::contains("recommendation:"));
}

// vocab契約: 語句ごとの人間/AI出現(文書数と10万字あたり率)と対数頻度比を出し、
// 追加候補は判断材料として提示するだけで自動採用しない。
#[test]
fn vocab_reports_per_phrase_rates_and_candidates() {
    eval_command()
        .args([
            "vocab",
            "eval/corpus.toml",
            "--exclude-id-prefix",
            "aozora-",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("excluded: id prefix \"aozora-\""))
        .stdout(predicate::str::contains("list=forbidden_phrase phrase="))
        .stdout(predicate::str::contains("list=hype_expression phrase="))
        .stdout(predicate::str::contains("log2_ratio="))
        .stdout(predicate::str::contains("candidates:"));
}

// report --split holdout: holdoutの一度きり評価の経路。dev文書は含まれない。
#[test]
fn report_can_evaluate_the_holdout_split_alone() {
    eval_command()
        .args(["report", "eval/corpus.toml", "--split", "holdout"])
        .assert()
        .success()
        .stdout(predicate::str::contains("documents: human=3 ai=0"))
        .stdout(predicate::str::contains(
            "split=holdoutのみを評価対象にしている",
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
