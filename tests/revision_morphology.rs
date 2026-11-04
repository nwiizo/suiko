use suiko::{lint, morphology::Morphology};

fn candidates(body: &str, morphology: &Morphology) -> Vec<lint::Finding> {
    lint::analyze(body, morphology, None, false)
        .unwrap()
        .findings
        .into_iter()
        .filter(|f| {
            matches!(
                f.category.as_str(),
                "explanatory_metaphor"
                    | "technical_jargon_metaphor"
                    | "revision_expression"
                    | "renyo_noun"
            )
        })
        .collect()
}

#[test]
fn vocabulary_uses_lemmas_particles_and_local_predicates() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "調査に連休を溶かした。",
        "検証に一晩をとかしました。",
        "ジョブを別のキューに逃がした。",
        "リクエストを外へ逃がします。",
        "モジュールは利用者の事情を知っている。",
        "ライブラリが成功を願った。",
        "この関数はデータベースと対話します。",
        "不具合が内部に潜んでいる。",
        "欠陥がひそんでいました。",
        "候補の絞りが必要です。",
        "手順の見極めを行います。",
        "設定が黙って破棄されます。",
        "処理が静かに止まりました。",
    ] {
        let found = candidates(body, &morphology);
        assert_eq!(
            found.len(),
            1,
            "{body}: {:?}",
            morphology.tokenize(body).unwrap()
        );
        let finding = &found[0];
        assert_eq!(finding.severity, "info");
        assert!(finding.suggestion.is_none());
        let span = finding.span.unwrap();
        assert_eq!(&body[span.start_byte..span.end_byte], finding.excerpt);
    }
}

#[test]
fn relations_do_not_cross_clauses_or_assert_negated_and_quoted_actions() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "休日を楽しんで、氷を溶かす。",
        "休日を使って金属を溶かす。",
        "処理を終えて鳥を逃がす。",
        "処理を、担当者が魚を逃がす前に終える。",
        "問題がある場所に猫が潜む。",
        "関数は、担当者が事情を知ってから呼び出す。",
        "関数は正常に動き、利用者が成功を願う。",
        "システムと利用者は対話する人を見守る。",
        "この関数は事情を知らない。",
        "休日を溶かさない。",
        "時間を溶かさない。",
        "データが静かに壊れない。",
        "データが静かに進む処理で壊れる。",
        "処理を逃がしません。",
        "不具合が潜んでいない。",
        "連休を溶かすという表現を避ける。",
        "猫が物陰に潜んでいる。",
        "見積もりの比べ方を説明する。",
        "候補の絞り込みが必要です。",
        "手順の見直しを行います。",
        "彼の確かめた内容を読む。",
        "彼の選びにくい道を進む。",
        "「連休を溶かした。\nジョブを逃がした。」",
        "`ジョブを逃がす`",
        "> 不具合が潜む。",
    ] {
        assert!(
            candidates(body, &morphology).is_empty(),
            "{body}: {:?}",
            candidates(body, &morphology)
        );
    }
}

#[test]
fn expanded_sense_words_still_require_the_matching_grammatical_role() {
    let morphology = Morphology::new().unwrap();
    for (body, expected) in [
        ("甘さをじっと眺めた。", 1),
        ("苦みを見つめています。", 1),
        ("湿り気を眺める。", 1),
        ("甘さを味わった。", 0),
        ("甘さの違う菓子を眺めた。", 0),
        ("苦みを確かめて、カップを眺めた。", 0),
        ("甘さを見つめない。", 0),
    ] {
        let report = lint::analyze(body, &morphology, None, false).unwrap();
        assert_eq!(
            report
                .findings
                .iter()
                .filter(|f| f.category == "sensory_role_mismatch")
                .count(),
            expected,
            "{body}: {:?}",
            morphology.tokenize(body).unwrap()
        );
    }
}
