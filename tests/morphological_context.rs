use suiko::{diff, lint, morphology::Morphology};

#[test]
fn difficulty_spelling_uses_the_preceding_verb_and_its_conjugation() {
    let morphology = Morphology::new().unwrap();
    for (text, expected) in [
        ("この道具は使いずらい。", "この道具は使いづらい。"),
        ("この仕事はやりずらかった。", "この仕事はやりづらかった。"),
        ("変更しずらくなった。", "変更しづらくなった。"),
        ("読者には読みずらい。", "読者には読みづらい。"),
        ("この文は分かりずらい。", "この文は分かりづらい。"),
    ] {
        let report = lint::analyze(text, &morphology, Some("tech"), false).unwrap();
        let finding = report
            .findings
            .iter()
            .find(|f| f.category == "word_usage")
            .unwrap_or_else(|| panic!("{text}: {:?}", morphology.tokenize(text).unwrap()));
        let suggestion = finding.suggestion.as_ref().unwrap();
        let range = suggestion.span.start_byte..suggestion.span.end_byte;
        assert_eq!(&text[range.clone()], suggestion.preimage);
        let mut revised = text.to_owned();
        revised.replace_range(range, &suggestion.replacement);
        assert_eq!(revised, expected);
    }
    for text in [
        "この道具は使いづらい。",
        "位置をずらして保存します。",
        "珍しいずらいという名称です。",
        "「使いずらい」という表記です。",
        "\"使いずらい\"という表記です。",
        "`使いずらい`という入力です。",
        "```\n使いずらい\n```",
    ] {
        let report = lint::analyze(text, &morphology, Some("tech"), false).unwrap();
        assert!(
            !report.findings.iter().any(|f| f.category == "word_usage"),
            "{text}: {:?}",
            report.findings
        );
    }
}

#[test]
fn explicit_ability_and_the_matching_potential_verb_share_content_words() {
    let morphology = Morphology::new().unwrap();
    for (before, after) in [
        ("値を取ることができます。", "値を取れます。"),
        ("ファイルを読むことができます。", "ファイルを読めます。"),
        ("バッファへ書き込むことができる。", "バッファへ書き込める。"),
        (
            "処理を行うことができ、結果を保存します。",
            "処理を行え、結果を保存します。",
        ),
        ("安全性を高めることができます。", "安全性を高められます。"),
    ] {
        let report = diff::compare(before, after, &morphology).unwrap();
        assert!(
            !report.findings.iter().any(|f| matches!(
                f.category.as_str(),
                "content_words_changed" | "sentence_function_changed"
            )),
            "{before} -> {after}: {:?}",
            report.findings
        );
    }
    for (before, after, category) in [
        ("汚れを取る。", "汚れが取れる。", "content_words_changed"),
        (
            "値を取ることができます。",
            "値を取れません。",
            "negation_changed",
        ),
        (
            "値を取ることができます。",
            "値を消せます。",
            "content_words_changed",
        ),
        (
            "値を取ることができました。",
            "値を取れます。",
            "sentence_function_changed",
        ),
    ] {
        let report = diff::compare(before, after, &morphology).unwrap();
        assert!(
            report.findings.iter().any(|f| f.category == category),
            "{before} -> {after}: {:?}",
            report.findings
        );
    }
}
