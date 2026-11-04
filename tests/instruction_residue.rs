use suiko::{lint, morphology::Morphology};

const CATEGORY: &str = "instruction_residue";

fn findings(body: &str, morphology: &Morphology, experimental: bool) -> Vec<lint::Finding> {
    lint::analyze(body, morphology, None, experimental)
        .unwrap()
        .findings
        .into_iter()
        .filter(|finding| finding.category == CATEGORY)
        .collect()
}

#[test]
fn writing_instructions_and_editing_reports_left_in_the_body_are_reported() {
    let morphology = Morphology::new().unwrap();
    for (body, line) in [
        (
            "この記事では、専門用語を使わず、飾らない言葉で説明します。\n",
            1,
        ),
        ("本記事では、です・ます調のトーンで解説していきます。\n", 1),
        ("以下では、300字程度でまとめます。\n", 1),
        ("本文。\n\n推敲後の文章は以下のとおりです。\n", 3),
        ("ご要望のとおり、敬体に統一して推敲しました。\n", 1),
        ("原文の意味を保ちつつ、冗長な表現を修正しました。\n", 1),
        ("本文。\n\n【修正版】\n\n本文。\n", 3),
        ("改稿後:\n\n本文。\n", 1),
        ("納期は来月末【要確認】とする。\n", 1),
    ] {
        let found = findings(body, &morphology, true);
        assert_eq!(found.len(), 1, "{body}: {found:?}");
        assert_eq!(found[0].line, line, "{body}");
        assert_eq!(found[0].severity, "info");
        assert!(found[0].suggestion.is_none());
        assert!(found[0].span.is_some(), "{body}");
        assert!(!findings(body, &morphology, false).is_empty(), "{body}");
    }
}

#[test]
fn ordinary_scope_statements_and_descriptions_of_editing_are_left_alone() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "この記事では、Rustの所有権を説明します。\n",
        "本記事では、設定の変更手順は扱いません。別紙を参照してください。\n",
        "推敲の手順を説明します。\n",
        "原文の意味を保つには、数値と条件を先に確かめる。\n",
        "修正後の動作を確認した。\n",
        "敬体に統一した理由は、読者が社外の人だからです。\n",
        "要確認の項目を一覧にした。\n",
        "> ご要望のとおり、敬体に統一して推敲しました。\n",
        "```\n推敲後の文章は以下のとおりです。\n```\n",
    ] {
        let found = findings(body, &morphology, true);
        assert!(found.is_empty(), "{body}: {found:?}");
    }
}
