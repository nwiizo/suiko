//! 中身を言わない語・評価・型（yomiyasu との機能差から抽象化した規則）を自作の例で固定する。

use suiko::{lint, morphology::Morphology};

const CATEGORIES: &[&str] = &[
    "vague_loanword",
    "thin_evaluation",
    "calque_phrase",
    "short_concession",
    "persona_ending",
    "te_nominal_stop",
    "cleft_list_ending",
    "evaluation_fragment_run",
    "heading_colon",
    "redundant_heading_bracket",
    "metaphor_label",
    "dialogue_period",
    "essay_report_density",
];

fn findings(
    body: &str,
    morphology: &Morphology,
    genre: Option<&str>,
) -> Vec<(String, String, usize)> {
    lint::analyze(body, morphology, genre, true)
        .unwrap()
        .findings
        .into_iter()
        .filter(|finding| CATEGORIES.contains(&finding.category.as_str()))
        .map(|finding| (finding.category, finding.excerpt, finding.line))
        .collect()
}

#[test]
fn hollow_sentences_are_reported_once_with_their_excerpt() {
    let morphology = Morphology::new().unwrap();
    for (body, category, excerpt) in [
        (
            "この施策はチームのシナジーを生みます。",
            "vague_loanword",
            "シナジー",
        ),
        (
            "本体の質感も上質です。",
            "thin_evaluation",
            "本体の質感も上質です",
        ),
        (
            "座り心地はとても快適です。",
            "thin_evaluation",
            "座り心地はとても快適です",
        ),
        (
            "その変更は大きな違いを作ります。",
            "calque_phrase",
            "違いを作",
        ),
        (
            "その案はまだテーブルにあります。",
            "calque_phrase",
            "案はまだテーブルにあり",
        ),
        (
            "私たちは同じページにいます。",
            "calque_phrase",
            "同じページにい",
        ),
        ("まずは水を試そう。", "calque_phrase", "水を試そ"),
        (
            "もちろん、速いです。",
            "short_concession",
            "もちろん、速いです",
        ),
        ("これは新しい機能なんよ。", "persona_ending", "なんよ"),
        (
            "原因を調べて、手順を改善。",
            "te_nominal_stop",
            "原因を調べて、手順を改善",
        ),
        (
            "今回変わったのは、速度、価格、重さ。",
            "cleft_list_ending",
            "今回変わったのは、速度、価格、重さ",
        ),
        (
            "効く。重い。安い。",
            "evaluation_fragment_run",
            "効く。重い。安い",
        ),
        ("# 認証とは：仕組み", "heading_colon", "# 認証とは：仕組み"),
        (
            "## 設定（概要）",
            "redundant_heading_bracket",
            "## 設定（概要）",
        ),
        (
            "# 鍵の番\n\n鍵の管理を説明します。",
            "metaphor_label",
            "# 鍵の番",
        ),
    ] {
        let found = findings(body, &morphology, None);
        assert_eq!(found.len(), 1, "{body}: {found:?}");
        assert_eq!(found[0].0, category, "{body}");
        assert_eq!(found[0].1, excerpt, "{body}");
    }
}

#[test]
fn natural_or_grounded_sentences_stay_quiet() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "プラットフォームエンジニアリングの章を読む。",
        "「シナジー」という語は使わない。",
        "本体の質感も上質ですが、角の仕上げは粗い。",
        "質感が上質になりました。",
        "意味をなさない。",
        // 「試しに」は試行の副詞で、水温を確かめる英語の句ではない。
        "コップの水を試しに飲んでみた。",
        "大きな絵を描いて、額に飾った。",
        "ドアの番を頼んだ。",
        "もちろん、大丈夫です。",
        "使えますか？もちろん、使えます。",
        "変わったのは何か。変わったのは、速度、価格、重さ。",
        "目指したのは、速さ、軽さ。",
        "原因を調べて、手順を改善しました。",
        "- 原因を調べて、手順を改善。",
        "効くし、重いし、安い。",
        "はい。そうです。",
        "ヤッホー！ヤッホー！ヤッホー！",
        "もちろん、筆者も参加予定です。",
        "デジタル・トランスフォーメーションを進める。",
        "# 使い方\n\n手順を説明します。",
        "# 当番\n\n当番を決めます。",
        "# 鍵の番\n\n鍵の番を交代します。",
        "## 設定（2024年版）",
        "```\n本体の質感も上質です。\n```",
        "> 効く。重い。安い。",
    ] {
        let found = findings(body, &morphology, None);
        assert!(found.is_empty(), "{body}: {found:?}");
    }
    // 語の出現だけの規則は人間の文書にも多く一致するため、--experimental でだけ示す。
    assert!(
        lint::analyze(
            "この施策はチームのシナジーを生みます。",
            &morphology,
            None,
            false
        )
        .unwrap()
        .findings
        .iter()
        .all(|finding| finding.category != "vague_loanword")
    );
    // 随筆では「なんよ」を語りとして残す。
    assert!(findings("これは新しい機能なんよ。", &morphology, Some("essay")).is_empty());
}

#[test]
fn essay_document_checks_apply_only_to_the_essay_genre() {
    let morphology = Morphology::new().unwrap();
    let dialogue = (0..10)
        .map(|index| format!("「今日は{index}回目です。」と彼女は言った。\n"))
        .collect::<String>();
    let found = findings(&dialogue, &morphology, Some("essay"));
    assert_eq!(
        found.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(),
        ["dialogue_period"],
        "{found:?}"
    );
    assert!(findings(&dialogue, &morphology, None).is_empty());
    let natural = dialogue.replace("です。」", "です」");
    assert!(findings(&natural, &morphology, Some("essay")).is_empty());

    let report = (0..60)
        .map(|index| {
            format!(
                "- 項目{index}は{index}件で、前月から{}件増えた。\n",
                index + 1
            )
        })
        .collect::<String>();
    let found = findings(&report, &morphology, Some("essay"));
    assert_eq!(
        found.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(),
        ["essay_report_density"],
        "{found:?}"
    );
    let personal = report.replace("増えた。", "増えたと私は思った。");
    assert!(findings(&personal, &morphology, Some("essay")).is_empty());
}
