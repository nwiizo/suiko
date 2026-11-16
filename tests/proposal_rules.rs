use suiko::{lint, morphology::Morphology};

#[test]
fn ignore_regions_mask_all_lint_paths_and_keep_original_locations() {
    let morphology = Morphology::new().unwrap();
    let text = "<!-- suiko:ignore-start -->\n# 不可欠な鍵\n**「題名」**です。\n設定を、保存することができます。\n<!-- suiko:ignore-end -->\n設定を、保存してください。\n";
    let report = lint::analyze(text, &morphology, Some("tech"), true).unwrap();
    assert!(
        report.findings.iter().all(|f| f.line >= 6),
        "{:?}",
        report.findings
    );
    assert_eq!(report.stats.measurements["masking"]["ignored_regions"], 1);
    assert_eq!(report.stats.measurements["masking"]["ignored_lines"], 3);
    assert!(report.findings.iter().any(|f| f.line == 6));
    for invalid in [
        "<!-- suiko:ignore-start -->\n設定を、保存してください。",
        "<!-- suiko:ignore-end -->\n設定を、保存してください。",
        "<!-- suiko:ignore-start -->\n<!-- suiko:ignore-start -->\n設定を、保存してください。\n<!-- suiko:ignore-end -->\n<!-- suiko:ignore-end -->",
    ] {
        let report = lint::analyze(invalid, &morphology, Some("tech"), false).unwrap();
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.category == "invalid_ignore_directive"),
            "{invalid}"
        );
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.category == "short_object_comma"),
            "{invalid}"
        );
    }
    let code = "```html\n<!-- suiko:ignore-start -->\n```\n設定を、保存してください。\n";
    let report = lint::analyze(code, &morphology, Some("tech"), false).unwrap();
    assert!(
        !report
            .findings
            .iter()
            .any(|f| f.category == "invalid_ignore_directive")
    );
    assert!(report.findings.iter().any(|f| f.line == 4));
}

#[test]
fn hatena_markup_is_masked_inline_and_in_blocks_without_losing_captions() {
    let morphology = Morphology::new().unwrap();
    let text = "[asin:不可欠:title]\n[:contents]\n[https://example.com/不可欠:embed]\n[f:id:不可欠:plain]\n<figure><img src=\"不可欠.png\"><figcaption>設定を、保存してください。</figcaption></figure>\n[asin:123:title]設定を、保存してください。\n";
    let report = lint::analyze(text, &morphology, Some("tech"), false).unwrap();
    assert!(
        !report
            .findings
            .iter()
            .any(|f| f.category == "forbidden_phrase")
    );
    assert_eq!(report.stats.measurements["masking"]["hatena_spans"], 5);
    assert!(
        report.findings.iter().any(|f| f.line == 5),
        "{:?}",
        report.findings
    );
    assert!(
        report.findings.iter().any(|f| f.line == 6),
        "{:?}",
        report.findings
    );
}

#[test]
fn long_sentences_keep_their_only_comma_and_exclude_english_quotations() {
    let morphology = Morphology::new().unwrap();
    let text = "設定は、アプリケーションを実行するために必要な接続先の情報と処理の開始時に読み込む複数のパラメーターを記録したファイルとして保存されます。";
    let report = lint::analyze(text, &morphology, Some("tech"), false).unwrap();
    assert!(
        !report
            .findings
            .iter()
            .any(|f| f.category == "short_topic_comma")
    );
    let text = "\"This is a long English quotation used to explain a technical concept and show why a configuration file needs to be validated before it is loaded.\" 設定を読み込む前に検証します。";
    let report = lint::analyze_reading_load(text, &morphology, Some("tech")).unwrap();
    assert!(
        !report
            .findings
            .iter()
            .any(|f| f.category == "sentence_too_long")
    );
}

#[test]
fn technical_labels_heading_emoji_and_invisible_characters_have_context() {
    let morphology = Morphology::new().unwrap();
    let text = "# 🐶 導入\n## 「🐶の話」という登壇\n- **/describe**: PRの説明を表示する\n- **`--json`**: JSONを返す\n設定を保存します。\u{200b}\u{200b}\n絵文字は👩\u{200d}💻です。\n```text\n見本\u{200b}\n```\n";
    let report = lint::analyze(text, &morphology, Some("tech"), false).unwrap();
    assert!(
        !report
            .findings
            .iter()
            .any(|f| f.category == "bullet_bold_label")
    );
    assert_eq!(
        report
            .findings
            .iter()
            .filter(|f| f.category == "heading_emoji")
            .count(),
        1
    );
    let invisible = report
        .findings
        .iter()
        .filter(|f| f.category == "invisible_character")
        .collect::<Vec<_>>();
    assert_eq!(invisible.len(), 2);
    for finding in invisible {
        assert_eq!(finding.line, 5);
        assert!(finding.detail.contains("U+200B"));
    }
    let report = lint::analyze(text, &morphology, Some("essay"), false).unwrap();
    assert!(
        !report
            .findings
            .iter()
            .any(|f| f.category == "heading_emoji")
    );
}

#[test]
fn experiential_past_ability_preserves_voice_but_technical_ability_is_still_checked() {
    let morphology = Morphology::new().unwrap();
    for text in [
        "楽しく読書を進めることができました。",
        "AIOpsについても触れることができ、非常に充実した時間でした。",
        "私は多くのことを学習することができました。",
    ] {
        let report = lint::analyze(text, &morphology, None, false).unwrap();
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.category == "evaluative_ability_past"),
            "{text}: {:?}",
            report.findings
        );
        assert!(
            !report.findings.iter().any(|f| matches!(
                f.category.as_str(),
                "translationese" | "translationese_morph"
            )),
            "{text}"
        );
        let report = lint::analyze(text, &morphology, Some("essay"), false).unwrap();
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.category == "evaluative_ability_past")
        );
    }
    for text in [
        "旧版では取得することができました。",
        "このAPIは設定を保存することができます。",
    ] {
        let report = lint::analyze(text, &morphology, Some("tech"), false).unwrap();
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.category == "translationese_morph"),
            "{text}"
        );
    }
}

#[test]
fn formatting_only_quote_edits_preserve_words_and_multiple_broken_bold_spans_are_reported() {
    let morphology = Morphology::new().unwrap();
    let report = suiko::diff::compare(
        "**「用語」**を使います。",
        "「**用語**」を使います。",
        &morphology,
    )
    .unwrap();
    assert!(
        !report
            .findings
            .iter()
            .any(|f| f.category == "quotation_changed")
    );
    let report = suiko::diff::compare(
        "**「許可する」**と説明した。",
        "「**許可しない**」と説明した。",
        &morphology,
    )
    .unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.category == "quotation_changed")
    );
    let report = lint::analyze("**[『書名A』](https://example.com/a)**が、**[『書名B』](https://example.com/b)**もあります。", &morphology, Some("tech"), false).unwrap();
    assert_eq!(
        report
            .findings
            .iter()
            .filter(|f| f.category == "bold_not_rendered")
            .count(),
        2
    );
    for text in ["****よりよい文書****", "**外側 **内側** 外側**"] {
        let report = lint::analyze(text, &morphology, Some("tech"), false).unwrap();
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.category == "bold_not_rendered"),
            "{text}: {:?}",
            report.findings
        );
    }
}

#[test]
fn potential_forms_are_review_candidates_separate_from_safe_edits() {
    let morphology = Morphology::new().unwrap();
    for (text, replacement) in [
        ("本を読むことができます。", "読め"),
        ("理解を深めることができました。", "深められ"),
        ("ここで話すことができる。", "話せる"),
    ] {
        let report = lint::analyze(text, &morphology, Some("tech"), false).unwrap();
        let finding = report
            .findings
            .iter()
            .find(|f| f.category == "translationese_morph")
            .unwrap();
        let json = serde_json::to_value(finding).unwrap();
        assert!(json["suggestion"].is_null());
        assert_eq!(
            json["review_suggestion"]["replacement"], replacement,
            "{text}: {json}"
        );
        let span = &json["review_suggestion"]["span"];
        assert_eq!(
            &text[span["start_byte"].as_u64().unwrap() as usize
                ..span["end_byte"].as_u64().unwrap() as usize],
            json["review_suggestion"]["preimage"].as_str().unwrap()
        );
    }
}

#[test]
fn morphological_context_candidates_are_scoped_and_do_not_flatten_essays() {
    let morphology = Morphology::new().unwrap();
    for (text, category) in [
        (
            "このブログは、参考資料を探すために利用してください。",
            "topic_predicate_mismatch",
        ),
        (
            "これらのフィールドは、入力の評価を行います。",
            "topic_predicate_mismatch",
        ),
        (
            "価値が見えにくく、その場での価値が見えにくいので迷います。",
            "repeated_predicate_phrase",
        ),
        (
            "設定を読み込みます。値を検証した。結果を保存します。",
            "paragraph_register_mixture",
        ),
        ("設定が変更しずらい。選択肢として上げられる。", "word_usage"),
    ] {
        let report = lint::analyze(text, &morphology, Some("tech"), true).unwrap();
        assert!(
            report.findings.iter().any(|f| f.category == category),
            "{text}: {:?}",
            report.findings
        );
        let essay = lint::analyze(text, &morphology, Some("essay"), false).unwrap();
        assert!(!essay.findings.iter().any(|f| f.category == category));
    }
    for text in [
        "この設定は、変更しないでください。",
        "システムは入力を自動で評価します。",
        "値を取得してログを保存します。",
        "設定を読み込みます。\n\n値を検証した。",
    ] {
        let report = lint::analyze(text, &morphology, Some("tech"), true).unwrap();
        assert!(
            !report.findings.iter().any(|f| matches!(
                f.category.as_str(),
                "topic_predicate_mismatch"
                    | "repeated_predicate_phrase"
                    | "paragraph_register_mixture"
                    | "word_usage"
            )),
            "{text}: {:?}",
            report.findings
        );
    }
}
