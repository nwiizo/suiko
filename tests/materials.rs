use std::path::PathBuf;

use suiko::{
    materials::{self, Source},
    morphology::Morphology,
};

fn categories(original: &str, final_text: &str, morphology: &Morphology) -> Vec<String> {
    materials::compare(
        final_text,
        &[Source {
            path: PathBuf::from("source.md"),
            text: original,
        }],
        morphology,
    )
    .unwrap()
    .findings
    .into_iter()
    .map(|finding| finding.category)
    .collect()
}

#[test]
fn same_number_on_a_different_subject_is_not_evidence() {
    let morphology = Morphology::new().unwrap();
    for (source, target) in [
        ("Aの応答時間は300msです。", "Bの応答時間は300msです。"),
        (
            "応答時間の平均値は300msです。",
            "応答時間の中央値は300msです。",
        ),
        ("本番環境の応答時間は300msです。", "応答時間は300msです。"),
    ] {
        let found = categories(source, target, &morphology);
        assert!(
            found.iter().any(|c| c == "material_value_context"),
            "{source} -> {target}: {found:?}"
        );
    }
}

#[test]
fn negated_improvement_is_not_evidence_for_a_positive_claim() {
    let morphology = Morphology::new().unwrap();
    let found = categories(
        "応答時間は改善していません。",
        "応答時間は改善しました。",
        &morphology,
    );
    assert!(
        found.iter().any(|c| c == "material_claim_unmatched"),
        "{found:?}"
    );
}

#[test]
fn values_keep_sign_magnitude_units_and_numeric_formatting() {
    let morphology = Morphology::new().unwrap();
    for (source, target) in [
        ("処理件数は1万件です。", "処理件数は1万人です。"),
        ("設定値は-1です。", "設定値は1です。"),
        ("保存期間は3日間です。", "保存期間は3日です。"),
    ] {
        let found = categories(source, target, &morphology);
        assert!(
            found.iter().any(|c| c == "material_value_unmatched"),
            "{source} -> {target}: {found:?}"
        );
    }
    for (source, target) in [
        ("処理件数は1,000件です。", "処理件数は１０００件です。"),
        ("応答時間は300msです。", "応答時間は300 msです。"),
        ("成功率は95%です。", "成功率は９５％です。"),
    ] {
        assert!(
            categories(source, target, &morphology).is_empty(),
            "{source} -> {target}"
        );
    }
}

#[test]
fn unconfirmed_paraphrases_are_kept_and_irrelevant_material_is_not_required() {
    let morphology = Morphology::new().unwrap();
    for (source, target) in [
        (
            "本番の応答時間は未測定です。",
            "本番の応答時間はまだ測定していません。",
        ),
        ("公開日は未定です。", "公開日はまだ決まっていません。"),
        ("配布機能は未実装です。", "ログを保存します。"),
    ] {
        assert!(
            categories(source, target, &morphology).is_empty(),
            "{source} -> {target}"
        );
    }
}

#[test]
fn markdown_metadata_examples_comments_and_references_are_not_claims() {
    let morphology = Morphology::new().unwrap();
    let target = "---\ncount: 999\n---\n\n# 手順\nログを保存します。\n\n> 性能は必ず99倍です。\n\n`最大999件`\n\n<!-- 最大999件 -->\n\n~~~md\n| 件数 | 999件 |\n~~~\n\n## 参考文献\n- 計測手法 2026年版\n";
    assert!(categories("ログを保存します。", target, &morphology).is_empty());
}

#[test]
fn an_added_effect_word_is_checked_as_a_claim_not_as_a_different_subject() {
    let morphology = Morphology::new().unwrap();
    let found = categories(
        "本番環境の応答時間は平均120msでした。",
        "本番環境の応答時間は平均120msに改善しました。",
        &morphology,
    );
    // 効果の語は material_claim_unmatched で示し、同じ値の対象の違いとしては重ねない。
    assert!(
        found.iter().any(|c| c == "material_claim_unmatched"),
        "{found:?}"
    );
    assert!(
        !found.iter().any(|c| c == "material_value_context"),
        "{found:?}"
    );
    // 効果の語を含む別の名詞（安定性）は、対象の語として比べ続ける。
    let found = categories(
        "応答時間は300msです。",
        "安定性の指標の応答時間は300msです。",
        &morphology,
    );
    assert!(
        found.iter().any(|c| c == "material_value_context"),
        "{found:?}"
    );
}
