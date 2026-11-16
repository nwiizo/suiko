use suiko::{lint, morphology::Morphology};

fn semantic_findings(text: &str, morphology: &Morphology) -> Vec<lint::Finding> {
    lint::analyze(text, morphology, Some("tech"), false)
        .unwrap()
        .findings
        .into_iter()
        .filter(|finding| {
            matches!(
                finding.category.as_str(),
                "sensory_role_mismatch" | "explanatory_metaphor" | "compressed_relation"
            )
        })
        .collect()
}

#[test]
fn semantic_relations_follow_inflected_predicates_and_case_markers() {
    let morphology = Morphology::new().unwrap();
    let mut missing = Vec::new();
    for (text, category) in [
        ("鐘の響きが扉を押した。", "sensory_role_mismatch"),
        ("足音が棚の香りを運びます。", "sensory_role_mismatch"),
        ("コップの熱さが照明を反射した。", "sensory_role_mismatch"),
        ("茶の渋みが光を返す。", "sensory_role_mismatch"),
        ("廊下で友人の横顔を聞いた。", "sensory_role_mismatch"),
        ("聞こえた父の横顔が遠ざかった。", "sensory_role_mismatch"),
        ("古い額縁が昨日を思い出した。", "explanatory_metaphor"),
        ("椅子は持ち主の願いを覚えている。", "explanatory_metaphor"),
        ("窓の外の夜を畳みます。", "explanatory_metaphor"),
        ("新人への出社を厚くします。", "compressed_relation"),
        ("設計の判断を丸くした。", "compressed_relation"),
        ("迷ったら立ち返る。", "compressed_relation"),
        ("迷えば戻す。", "compressed_relation"),
    ] {
        let found = semantic_findings(text, &morphology);
        let matching = found
            .iter()
            .filter(|f| f.category == category)
            .collect::<Vec<_>>();
        if matching.is_empty() {
            missing.push(format!("{text}: {:?}", morphology.tokenize(text).unwrap()));
            continue;
        }
        for finding in matching {
            assert_eq!(finding.severity, "info", "{text}");
            assert!(finding.suggestion.is_none(), "{text}");
            let span = finding.span.unwrap();
            assert_eq!(&text[span.start_byte..span.end_byte], finding.excerpt);
            assert!(!finding.detail.is_empty());
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}

#[test]
fn sensory_subjects_follow_only_local_coordinated_predicates() {
    let morphology = Morphology::new().unwrap();
    let mut missing = Vec::new();
    for text in [
        "羽音が室内に流れ込み、すぐに閉じた。",
        "電子音が響いて、ゆっくり割れた。",
        "靴音が窓を押します。",
        "羽音は扉を押した。",
    ] {
        if !semantic_findings(text, &morphology)
            .iter()
            .any(|f| f.category == "sensory_role_mismatch")
        {
            missing.push(format!("{text}: {:?}", morphology.tokenize(text).unwrap()));
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
    for text in [
        "羽音が室内に流れ込み、窓が閉じた。",
        "羽音が室内に流れ込み、窓を閉じた。",
        "羽音が響いたので、窓を閉じた。",
        "羽音が響く部屋で、窓を閉じた。",
        "羽音が響いて、消えた。",
        "電子音が響いて、閉じない。",
        "本音が扉を押した。",
        "観音が扉を押した。",
        "心音計が窓を叩いた。",
    ] {
        assert!(
            semantic_findings(text, &morphology).is_empty(),
            "{text}: {:?}",
            semantic_findings(text, &morphology)
        );
    }
}

#[test]
fn semantic_relations_preserve_arguments_negation_voice_and_clause_boundaries() {
    let morphology = Morphology::new().unwrap();
    for text in [
        "鐘の響きが聞こえ、係員が扉を押した。",
        "足音を聞きながら、風が花の香りを運ぶ。",
        "コップの熱さが残るガラスは照明を反射した。",
        "コップの表面が照明を反射した。",
        "友人の声を聞いた。",
        "聞こえた父の声が遠ざかった。",
        "父から聞いた祖母の背中の痛みについて説明した。",
        "椅子の持ち主は昔を思い出した。",
        "椅子は持ち主が思い出した場所にある。",
        "椅子が記憶を持つとは思わない。",
        "額縁が昨日を思い出さない。",
        "額縁が昨日を思い出させた。",
        "額縁が持ち主に思い出された。",
        "夜に毛布を畳みます。",
        "新人への支援を厚くします。",
        "設計図の角を丸くした。",
        "迷ったら最初の手順に立ち返る。",
        "迷えば変更を元に戻す。",
        "迷ったら立ち返らない。",
        "誰に相談するか迷ったら、担当者に聞く。",
        "小説では『椅子が昨日を思い出した』と描かれる。",
        "「静かな部屋。\n額縁が昨日を思い出す」と書かれている。",
    ] {
        assert!(
            semantic_findings(text, &morphology).is_empty(),
            "{text}: {:?}",
            semantic_findings(text, &morphology)
        );
    }
}

#[test]
fn compressed_relations_reach_headings_lists_and_keep_source_offsets() {
    let morphology = Morphology::new().unwrap();
    let text = "# 迷ったら立ち返る\n\n- 新人への出社を厚くする\n\n1. 設計の判断を丸くする\n";
    let found = semantic_findings(text, &morphology);
    assert_eq!(found.len(), 3, "{found:?}");
    assert_eq!(found.iter().map(|f| f.line).collect::<Vec<_>>(), [1, 3, 5]);
    for finding in found {
        assert_eq!(finding.category, "compressed_relation");
        let span = finding.span.unwrap();
        let line = text.lines().nth(finding.line - 1).unwrap();
        assert_eq!(&line[span.start_byte..span.end_byte], finding.excerpt);
    }
    for text in [
        "> 迷ったら立ち返る\n",
        "```text\n迷ったら立ち返る\n```\n",
        "`新人への出社を厚くする`\n",
        "<!-- suiko:ignore-start -->\n迷ったら立ち返る\n<!-- suiko:ignore-end -->\n",
        "## 迷ったら最初の手順に立ち返る\n",
    ] {
        assert!(semantic_findings(text, &morphology).is_empty(), "{text}");
    }
}
