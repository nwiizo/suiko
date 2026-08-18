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

// span契約: 列はUnicode scalar数え・1始まり(全角も1)、byteは各行内の
// UTF-8 offset・0始まり、いずれも半開区間。文書全体指標のfindingはspanを持たない。
#[test]
fn findings_carry_line_column_and_byte_spans() {
    // 同一表現の複数出現と全角文字: 2つのfindingが別の位置を一意に指す
    let (_dir, path) = draft("Ｘ重要なのは速度で、重要なのは品質です。\n");
    let output = cargo_bin_cmd!("suiko")
        .args(["lint", path.to_str().expect("UTF-8 path"), "--json"])
        .output()
        .expect("run suiko lint");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    let spans = json["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .filter(|finding| finding["category"] == "forbidden_phrase")
        .map(|finding| finding["span"].clone())
        .collect::<Vec<_>>();
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0]["start_column"], 2);
    assert_eq!(spans[0]["end_column"], 7);
    assert_eq!(spans[0]["start_byte"], 3);
    assert_eq!(spans[0]["end_byte"], 18);
    assert_eq!(spans[1]["start_column"], 11);
    assert_eq!(spans[1]["start_byte"], 30);

    // 複数行にまたがるfinding
    let (_dir2, path2) = draft("それは組織の再構築である。\nなぜなら場所に縛られないからだ。\n");
    let output = cargo_bin_cmd!("suiko")
        .args([
            "lint",
            path2.to_str().expect("UTF-8 path"),
            "--experimental",
            "--json",
        ])
        .output()
        .expect("run suiko lint");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    let cleft = json["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .find(|finding| finding["category"] == "english_syntax_cleft_because")
        .cloned()
        .expect("cleft finding");
    assert_eq!(cleft["span"]["start_line"], 1);
    assert_eq!(cleft["span"]["end_line"], 2);
    assert_eq!(cleft["span"]["end_byte"], 45);

    // 結合文字(か+U+3099)はUnicode scalarとして数える
    let (_dir3, path3) = draft("か\u{3099}きく 重要なのは品質です。\n");
    let output = cargo_bin_cmd!("suiko")
        .args(["lint", path3.to_str().expect("UTF-8 path"), "--json"])
        .output()
        .expect("run suiko lint");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    let span = json["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .find(|finding| finding["category"] == "forbidden_phrase")
        .map(|finding| finding["span"].clone())
        .expect("span");
    assert_eq!(span["start_column"], 6);
    assert_eq!(span["start_byte"], 13);

    // 文書全体指標のfindingはspanを持たない
    let (_dir4, path4) =
        draft("短い。とても短い文。同じ長さ。似た長さの文。また同じ。等しい長さ。\n");
    let output = cargo_bin_cmd!("suiko")
        .args(["lint", path4.to_str().expect("UTF-8 path"), "--json"])
        .output()
        .expect("run suiko lint");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    for finding in json["findings"].as_array().expect("findings array") {
        if finding["category"] == "low_sentence_variance" {
            assert!(finding.get("span").is_none());
        }
    }
}

// 参考文献リスト行([1]…、[^1]: …)とManning式コード注釈行(#A …)は本文から
// マスクし、読解負荷や文統計に数えない。文中の参照(研究[1]は…)は本文に残す。
#[test]
fn reference_and_code_annotation_lines_are_masked_from_prose() {
    let long_tail = "とても長い説明をここへ続けて九十字の目安を確実に超えるようにし、読解負荷の対象になるかどうかを確かめられるだけの十分な長さを確保したうえで、さらに念のため補足の語句も付け加えておきます";
    let (_dir, path) = draft(&format!(
        "[1] McKinsey & Company. {long_tail}。\n\n#A 図8.2の状態管理システムを初期化する注釈で{long_tail}。\n\n研究[1]によると、{long_tail}。\n"
    ));

    let output = cargo_bin_cmd!("suiko")
        .args([
            "lint",
            path.to_str().expect("UTF-8 path"),
            "--reading-load",
            "--json",
        ])
        .output()
        .expect("run suiko lint");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    assert_eq!(json["stats"]["masking"]["reference_lines"], 1);
    assert_eq!(json["stats"]["masking"]["code_annotation_lines"], 1);
    // 本文として残るのは文中参照の1文だけで、それはsentence_too_longに数える
    assert_eq!(json["reading_load"]["stats"]["sentences"], 1);
    assert_eq!(
        json["reading_load"]["stats"]["by_category"]["sentence_too_long"],
        1
    );
}

// antithesis_repetitionのseverity: 一致3回でも長文では比率が0.02未満になり
// infoに留まる。短い文書では同じ3回がcriticalになる(別テストで固定済み)。
#[test]
fn antithesis_severity_stays_info_in_long_documents() {
    let mut body = String::new();
    for index in 0..160 {
        body.push_str(&format!("これは水増し用の文番号{index}である。"));
        if index % 60 == 0 {
            body.push_str("速さではなく、正確さを優先する。");
        }
        if index % 50 == 0 {
            body.push('\n');
        }
    }
    let (_dir, path) = draft(&body);

    let output = cargo_bin_cmd!("suiko")
        .args(["lint", path.to_str().expect("UTF-8 path"), "--json"])
        .output()
        .expect("run suiko lint");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    let antithesis = json["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .find(|finding| finding["category"] == "antithesis_repetition")
        .cloned()
        .expect("antithesis finding");
    assert_eq!(antithesis["severity"], "info");
}

// FAQのQ./A.のような短いマーカー+区切りの反復は、定型フィールドとして
// 散文の無意識な文頭反復と区別する。
#[test]
fn faq_marker_leads_are_flagged_as_structured_labels() {
    let faq = (1..=7)
        .map(|index| {
            format!(
                "Q. 質問{index}はどこで確認できますか。\n\nA. 回答{index}はマニュアルに記載されています。\n"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let (_dir, path) = draft(&faq);

    let output = cargo_bin_cmd!("suiko")
        .args([
            "lint",
            path.to_str().expect("UTF-8 path"),
            "--genre",
            "tech",
            "--experimental",
            "--json",
        ])
        .output()
        .expect("run suiko lint");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    let leads = json["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .filter(|finding| finding["category"] == "repeated_sentence_lead")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(leads.len(), 2);
    for finding in &leads {
        assert!(
            finding["detail"]
                .as_str()
                .expect("detail text")
                .contains("定型フィールド")
        );
    }
}

// 局所AIパターン: 短い文書でも装飾箇条書き、述語+コロン、誇張表現を検出する。
// 名詞ラベル(「使用方法:」)と装飾なし箇条書きは対象外。
#[test]
fn local_ai_patterns_fire_on_short_documents() {
    let (_dir, path) = draft(
        "革命的な技術で業界を変えます。\n実行します:\n- **重要**: これは重要な項目です\n- ✅ 完了した項目です\n",
    );
    let output = cargo_bin_cmd!("suiko")
        .args([
            "lint",
            path.to_str().expect("UTF-8 path"),
            "--experimental",
            "--genre",
            "tech",
            "--json",
        ])
        .output()
        .expect("run suiko lint");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    assert_eq!(json["stats"]["by_category"]["hype_expression"], 1);
    assert_eq!(json["stats"]["by_category"]["predicate_colon_lead"], 1);
    assert_eq!(json["stats"]["by_category"]["bullet_bold_label"], 1);
    assert_eq!(json["stats"]["by_category"]["bullet_emoji"], 1);

    let (_dir2, path2) = draft("使用方法:\n- 設定を開く\n- 保存する\n");
    let output = cargo_bin_cmd!("suiko")
        .args([
            "lint",
            path2.to_str().expect("UTF-8 path"),
            "--experimental",
            "--genre",
            "tech",
            "--json",
        ])
        .output()
        .expect("run suiko lint");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    assert_eq!(json["stats"]["total_findings"], 0);
}

#[test]
fn antithesis_matches_aggregate_into_a_single_document_finding() {
    let (_dir, path) = draft("Aではなく、BだけでなくCもあります。\nDではなく、Eです。\n");

    let output = cargo_bin_cmd!("suiko")
        .args(["lint", path.to_str().expect("UTF-8 path"), "--json"])
        .output()
        .expect("run suiko lint");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    let antithesis = json["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .filter(|finding| finding["category"] == "antithesis_repetition")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(antithesis.len(), 1);
    assert_eq!(antithesis[0]["severity"], "critical");
    assert_eq!(antithesis[0]["related_lines"], serde_json::json!([1, 2]));
    let detail = antithesis[0]["detail"].as_str().expect("detail text");
    assert!(detail.contains("3回"));
    assert!(detail.contains("総文数2"));
    assert!(detail.contains("150.0%"));
    assert!(detail.contains("100%を超える"));
}

#[test]
fn repeated_leads_aggregate_per_key_and_flag_label_fields() {
    let glossary = (1..=6)
        .map(|index| format!("定義：項目{index}の説明です。\n\n主張：項目{index}の要点です。\n"))
        .collect::<Vec<_>>()
        .join("\n");
    let (_dir, path) = draft(&format!("{glossary}\nまとめると、以上です。\n"));

    let output = cargo_bin_cmd!("suiko")
        .args([
            "lint",
            path.to_str().expect("UTF-8 path"),
            "--experimental",
            "--json",
        ])
        .output()
        .expect("run suiko lint");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    let leads = json["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .filter(|finding| finding["category"] == "repeated_sentence_lead")
        .cloned()
        .collect::<Vec<_>>();
    // 12行の反復でも、反復キー(定義：/主張：)ごとに1件へ集約する
    assert_eq!(leads.len(), 2);
    for finding in &leads {
        let detail = finding["detail"].as_str().expect("detail text");
        assert!(detail.contains("6回反復"));
        assert!(detail.contains("定型フィールド"));
        assert_eq!(finding["related_lines"].as_array().expect("lines").len(), 6);
    }
    // 集約しても他カテゴリのfindingは見失わない
    assert_eq!(json["stats"]["by_category"]["forbidden_phrase"], 1);
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
fn outline_heading_stats_are_stable_with_sudachi() {
    let output = cargo_bin_cmd!("suiko")
        .args(["outline", "tests/fixtures/outline-sudachi.md", "--json"])
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

// 用語クラス別の抽出契約: カタカナ複合語、ASCII略語、製品名、日本語固有名詞を
// 拾い、一般名詞(毎日、世界、物事、本質)と漢字複合語内のカタカナ断片を拾わない。
// 既知の限界: 空白区切りの製品名(GitHub Actions等)は語ごとに分かれる。
#[test]
fn terms_extract_expected_classes_and_skip_generic_nouns() {
    let output = cargo_bin_cmd!("suiko")
        .args(["terms", "tests/fixtures/terms-classes.md", "--json"])
        .output()
        .expect("run suiko terms");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    let terms = json["terms"]
        .as_array()
        .expect("terms array")
        .iter()
        .map(|term| term["term"].as_str().expect("term text").to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        terms,
        vec![
            "オブザーバビリティプラットフォーム",
            "メトリクス",
            "トレース",
            "SLO",
            "ダッシュボード",
            "Kubernetes",
            "クラスタ",
            "GitHub",
            "Actions",
            "API",
            "Gateway",
            "RateLimitPolicy",
            "東京",
            "田中太郎",
        ]
    );
}

// GitHub Actions注釈: severityをerror/warning/noticeへ写し、spanの行・列を
// workflowコマンドのプロパティで渡す。改行と%はエスケープする。
#[test]
fn github_format_emits_workflow_command_annotations() {
    let (_dir, path) = draft("重要なのは、この点です。一文がとても長いということはありません。\n");

    let output = cargo_bin_cmd!("suiko")
        .args([
            "lint",
            path.to_str().expect("UTF-8 path"),
            "--format",
            "github",
        ])
        .output()
        .expect("run suiko lint");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let annotation = stdout
        .lines()
        .find(|line| line.contains("title=suiko forbidden_phrase"))
        .expect("forbidden_phrase annotation");
    assert!(annotation.starts_with("::notice file="));
    assert!(annotation.contains("line=1"));
    assert!(annotation.contains("col=1"));
    assert!(annotation.contains("endColumn=6"));
    assert!(annotation.contains("::禁止語/LLM常套句ヒット"));
}

// SARIF 2.1.0: エディタ・コードスキャン向け。columnKind=unicodeCodePointsを
// 宣言し、spanの列をそのまま渡す。severityはerror/warning/noteへ写す。
#[test]
fn sarif_format_emits_valid_minimal_sarif() {
    let (_dir, path) = draft("重要なのは、この点です。\n");

    let output = cargo_bin_cmd!("suiko")
        .args([
            "lint",
            path.to_str().expect("UTF-8 path"),
            "--format",
            "sarif",
        ])
        .output()
        .expect("run suiko lint");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid SARIF JSON");
    assert_eq!(json["version"], "2.1.0");
    assert_eq!(json["runs"][0]["columnKind"], "unicodeCodePoints");
    assert_eq!(json["runs"][0]["tool"]["driver"]["name"], "suiko");
    let result = &json["runs"][0]["results"][0];
    assert_eq!(result["ruleId"], "forbidden_phrase");
    assert_eq!(result["level"], "note");
    let region = &result["locations"][0]["physicalLocation"]["region"];
    assert_eq!(region["startLine"], 1);
    assert_eq!(region["startColumn"], 1);
    assert_eq!(region["endColumn"], 6);
    let rules = json["runs"][0]["tool"]["driver"]["rules"]
        .as_array()
        .expect("rules array");
    assert!(rules.iter().any(|rule| rule["id"] == "forbidden_phrase"));
}

// suggestion契約: 機械的に安全なallowlist(「する+こと+が+できる」の縮約)だけが
// 対象で、preimageがspan位置の原文と一致する場合のみ付与される。適用側は
// preimage不一致の変更を適用してはならない。Suiko自身はファイルを書き換えない。
// 「は」型(ことはできない)と使役型は、2026-08-18の実コーパスラベルに基づき
// 検出自体の対象外(自然な用例)。
#[test]
fn safe_suggestions_carry_matching_preimages_and_never_modify_files() {
    let contents = "この設定を変更することができます。ただし削除することはできません。回答を最新の情報に基づかせることができます。\n";
    let (_dir, path) = draft(contents);

    let output = cargo_bin_cmd!("suiko")
        .args(["lint", path.to_str().expect("UTF-8 path"), "--json"])
        .output()
        .expect("run suiko lint");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    let morph_findings = json["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .filter(|finding| finding["category"] == "translationese_morph")
        .cloned()
        .collect::<Vec<_>>();
    // 「は」型と使役型は発火せず、「が」型の1件だけが確認候補になる
    assert_eq!(morph_findings.len(), 1);

    // 「することができます」は削除候補を持ち、preimageがファイルの実バイトと一致する
    let suggestion = &morph_findings[0]["suggestion"];
    assert_eq!(suggestion["preimage"], "することが");
    assert_eq!(suggestion["replacement"], "");
    let line = contents
        .lines()
        .nth(suggestion["span"]["start_line"].as_u64().expect("line") as usize - 1)
        .expect("line text");
    let start = suggestion["span"]["start_byte"].as_u64().expect("start") as usize;
    let end = suggestion["span"]["end_byte"].as_u64().expect("end") as usize;
    assert_eq!(&line[start..end], "することが");

    // 読み取り専用: 入力ファイルは変更されない
    let unchanged = std::fs::read_to_string(&path).expect("read draft");
    assert_eq!(unchanged, contents);
}

// redundant_light_verb契約: サ変名詞+を+行う(隣接)だけが対象で、受身・使役・
// 非隣接・非サ変名詞は発火しない。suggestionは終止/連用/促音便の3活用のみで、
// preimageがファイルの実バイトと一致する場合だけ付与される。
#[test]
fn redundant_light_verb_fires_on_adjacent_sahen_and_carries_a_safe_suggestion() {
    let contents = "リリース前に検証を行い、結果を記録した。式典が行われる日程は未定だ。祭りを行う地区も多い。担当者に集計を行わせる案は見送った。\n";
    let (_dir, path) = draft(contents);

    let output = cargo_bin_cmd!("suiko")
        .args(["lint", path.to_str().expect("UTF-8 path"), "--json"])
        .output()
        .expect("run suiko lint");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    let findings = json["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .filter(|finding| finding["category"] == "redundant_light_verb")
        .cloned()
        .collect::<Vec<_>>();
    // 受身(行われる)・非サ変(祭り)・使役(行わせる)は発火せず、隣接1件のみ
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["severity"], "info");

    let suggestion = &findings[0]["suggestion"];
    assert_eq!(suggestion["preimage"], "を行い");
    assert_eq!(suggestion["replacement"], "し");
    let line = contents
        .lines()
        .nth(suggestion["span"]["start_line"].as_u64().expect("line") as usize - 1)
        .expect("line text");
    let start = suggestion["span"]["start_byte"].as_u64().expect("start") as usize;
    let end = suggestion["span"]["end_byte"].as_u64().expect("end") as usize;
    assert_eq!(&line[start..end], "を行い");

    let unchanged = std::fs::read_to_string(&path).expect("read draft");
    assert_eq!(unchanged, contents);
}

// terms --audit の契約: 複数ファイルの用語を集計し、SudachiDictの正規化表記で
// 表記揺れをクラスタする。ファイルは書き換えない。
#[test]
fn terms_audit_aggregates_files_and_clusters_spelling_variants() {
    let dir = tempdir().expect("create temporary directory");
    let a = dir.path().join("a.md");
    let b = dir.path().join("b.md");
    fs::write(
        &a,
        "サーバーの設定を確認した。サーバーは再起動が必要だった。\n",
    )
    .expect("write a");
    fs::write(
        &b,
        "サーバの増設を検討している。Kubernetesとは、コンテナ基盤のことである。\n",
    )
    .expect("write b");

    let output = cargo_bin_cmd!("suiko")
        .args([
            "terms",
            "--audit",
            a.to_str().expect("UTF-8 path"),
            b.to_str().expect("UTF-8 path"),
            "--json",
        ])
        .output()
        .expect("run suiko terms --audit");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    assert_eq!(json["suiko_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(json["files"].as_array().expect("files").len(), 2);

    let variants = json["variants"].as_array().expect("variants array");
    assert_eq!(variants.len(), 1);
    assert_eq!(variants[0]["normalized"], "サーバー");
    assert_eq!(variants[0]["spellings"][0]["term"], "サーバー");
    assert_eq!(variants[0]["spellings"][0]["total_count"], 2);
    assert_eq!(variants[0]["spellings"][1]["term"], "サーバ");
    assert_eq!(variants[0]["spellings"][1]["total_count"], 1);

    let terms = json["terms"].as_array().expect("terms array");
    let kubernetes = terms
        .iter()
        .find(|term| term["term"] == "Kubernetes")
        .expect("Kubernetes term");
    assert_eq!(kubernetes["files"][0]["has_gloss_hint"], true);

    // 監査は読み取り専用: 入力ファイルは変更されない
    let unchanged = fs::read_to_string(&b).expect("read b");
    assert!(unchanged.contains("サーバの増設"));
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
fn baseline_compares_multiple_files_and_flags_added_and_removed_ones() {
    let dir = tempdir().expect("create temporary directory");
    let a = dir.path().join("a.md");
    let b = dir.path().join("b.md");
    let c = dir.path().join("c.md");
    fs::write(&a, "と言えるでしょう。\n").expect("write a");
    fs::write(&b, "まとめると、要点です。\n").expect("write b");
    let baseline = dir.path().join("baseline.json");
    let first = cargo_bin_cmd!("suiko")
        .args([
            "lint",
            a.to_str().expect("UTF-8 path"),
            b.to_str().expect("UTF-8 path"),
            "--json",
        ])
        .output()
        .expect("create baseline");
    assert!(first.status.success());
    fs::write(&baseline, first.stdout).expect("write baseline");

    fs::write(&a, "何も指摘されない文です。\n").expect("resolve a");
    fs::write(&c, "いかがでしたか。\n").expect("write c");
    let output = cargo_bin_cmd!("suiko")
        .args([
            "lint",
            a.to_str().expect("UTF-8 path"),
            b.to_str().expect("UTF-8 path"),
            c.to_str().expect("UTF-8 path"),
            "--json",
            "--baseline",
            baseline.to_str().expect("UTF-8 path"),
        ])
        .output()
        .expect("compare baseline");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    let records = json.as_array().expect("array of records");
    assert_eq!(records.len(), 3);
    assert_eq!(records[0]["baseline"]["file_status"], "matched");
    assert_eq!(records[0]["baseline"]["summary"]["resolved"], 1);
    assert_eq!(records[1]["baseline"]["file_status"], "matched");
    assert_eq!(records[1]["baseline"]["summary"]["persisting"], 1);
    assert_eq!(records[2]["baseline"]["file_status"], "added");
    assert_eq!(records[2]["findings"][0]["status"], "new");
    assert_eq!(records[0]["suiko_version"], env!("CARGO_PKG_VERSION"));

    let removed = cargo_bin_cmd!("suiko")
        .args([
            "lint",
            b.to_str().expect("UTF-8 path"),
            "--json",
            "--baseline",
            baseline.to_str().expect("UTF-8 path"),
        ])
        .output()
        .expect("compare with removed file");
    assert!(removed.status.success());
    let stderr = String::from_utf8_lossy(&removed.stderr);
    assert!(stderr.contains("今回の対象にないファイル"));
    assert!(stderr.contains("a.md"));
}

#[test]
fn baseline_keeps_document_level_findings_persisting_when_wording_changes() {
    let (dir, path) = draft("Aではなく、BだけでなくCもあります。\nDではなく、Eです。\n");
    let baseline = dir.path().join("baseline.json");
    let first = cargo_bin_cmd!("suiko")
        .args(["lint", path.to_str().expect("UTF-8 path"), "--json"])
        .output()
        .expect("create baseline");
    assert!(first.status.success());
    fs::write(&baseline, first.stdout).expect("write baseline");

    fs::write(
        &path,
        "犬ではなく、猫だけでなく鳥もいます。\n夏ではなく、冬です。\n",
    )
    .expect("rewrite draft");
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
    assert_eq!(json["baseline"]["summary"]["resolved"], 0);
    assert_eq!(json["baseline"]["summary"]["new"], 0);
    assert_eq!(json["baseline"]["summary"]["persisting"], 1);
    assert_eq!(json["findings"][0]["category"], "antithesis_repetition");
    assert_eq!(json["findings"][0]["status"], "persisting");
}

#[test]
fn baseline_rejects_mismatched_genre_and_version() {
    let (dir, path) = draft("と言えるでしょう。\n");
    let baseline = dir.path().join("baseline.json");
    let first = cargo_bin_cmd!("suiko")
        .args([
            "lint",
            path.to_str().expect("UTF-8 path"),
            "--genre",
            "tech",
            "--json",
        ])
        .output()
        .expect("create baseline");
    assert!(first.status.success());
    fs::write(&baseline, &first.stdout).expect("write baseline");

    cargo_bin_cmd!("suiko")
        .args([
            "lint",
            path.to_str().expect("UTF-8 path"),
            "--json",
            "--baseline",
            baseline.to_str().expect("UTF-8 path"),
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("genreが現在と一致しません"));

    let forged = format!(
        r#"{{"file": {}, "suiko_version": "0.0.0", "stats": {{"genre": null, "experimental": false}}, "findings": []}}"#,
        serde_json::to_string(path.to_str().expect("UTF-8 path")).expect("encode path"),
    );
    fs::write(&baseline, forged).expect("write forged baseline");
    cargo_bin_cmd!("suiko")
        .args([
            "lint",
            path.to_str().expect("UTF-8 path"),
            "--json",
            "--baseline",
            baseline.to_str().expect("UTF-8 path"),
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "Suikoバージョンが現在と一致しません",
        ));
}

#[test]
fn reading_load_is_reported_in_a_separate_json_lane() {
    let long_sentence = format!(
        "{}。\n",
        "この文には、分割すべき情報が含まれています".repeat(8)
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

// no_comma_sentence契約: 読点ゼロの60字以上の日本語散文だけが対象で、読点
// （、・，・,）を1つでも含む文、60字未満、Latin優勢の引用行・URL行は発火しない。
#[test]
fn no_comma_sentence_fires_only_on_long_japanese_prose_without_touten() {
    let contents = "システムはリクエストを受信すると内部キューへ登録して即座に仮応答を返す非同期処理方式を採用しているため利用者の体感応答時間は常に一定です。読点を、1つ含む同じ長さの文はこの検出の対象にならず読解負荷レーンにも現れない仕組みになっています。\nhttps://medium.com/airbnb-engineering/listing-embeddings-for-similar-listing-recommendations-and-real-time-personalization\n";
    let (_dir, path) = draft(contents);

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
    let findings = json["reading_load"]["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .filter(|finding| finding["category"] == "no_comma_sentence")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["severity"], "info");
    assert_eq!(findings[0]["line"], 1);
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
        "この文には、分割すべき情報が含まれています".repeat(8)
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
