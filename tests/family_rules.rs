//! 規則の族（docs/rule-families.md）のW2〜W5で新設・拡張した規則の正例と反例。

use suiko::{lint, morphology::Morphology};

/// CLIの既定と同じく、全規則で検査する。
fn findings(morphology: &Morphology, body: &str, category: &str) -> Vec<lint::Finding> {
    lint::analyze(body, morphology, None, true)
        .unwrap()
        .findings
        .into_iter()
        .filter(|finding| finding.category == category)
        .collect()
}

fn excerpt_of_span(body: &str, finding: &lint::Finding) -> String {
    let span = finding.span.expect("span");
    let line = body.split('\n').nth(span.start_line - 1).unwrap();
    line[span.start_byte..span.end_byte].to_owned()
}

// ---- W2 repeated_sentence_ending ----

#[test]
fn repeated_sentence_ending_points_at_the_third_ending_of_a_run() {
    let morphology = Morphology::new().unwrap();
    for (body, ending, lines) in [
        (
            "この設定は安全です。次の手順も簡単です。最後の確認も短時間です。\n",
            "です",
            vec![1],
        ),
        (
            "ファイルを開きます。設定を保存します。画面を閉じます。\n",
            "ます",
            vec![1],
        ),
        (
            "昨日は会議に出ました。\n資料を配りました。\n意見を集めました。\n",
            "ました",
            vec![1, 2, 3],
        ),
        (
            "当時の店は小さな店でした。客も少ない日でした。それでも楽しい毎日でした。\n",
            "でした",
            vec![1],
        ),
        (
            "これは道具である。あれも道具である。それも道具である。\n",
            "である",
            vec![1],
        ),
        (
            "これは道具だ。あれも道具だ。それも同じ道具だ。\n",
            "だ",
            vec![1],
        ),
        (
            "明日は雨でしょう。風も強いでしょう。外出は控えるでしょう。\n",
            "でしょう",
            vec![1],
        ),
        (
            "明日は雨だろう。風も強いだろう。外出は控えるだろう。\n",
            "だろう",
            vec![1],
        ),
        (
            "今日は暑いですね。風も強いですね。空も高いですね。\n",
            "ですね",
            vec![1],
        ),
    ] {
        let found = findings(&morphology, body, "repeated_sentence_ending");
        assert_eq!(found.len(), 1, "{body}: {found:?}");
        assert_eq!(excerpt_of_span(body, &found[0]), ending, "{body}");
        assert_eq!(
            found[0].related_lines.as_deref(),
            Some(&lines[..]),
            "{body}"
        );
        assert_eq!(found[0].line, *lines.last().unwrap(), "{body}");
    }
}

#[test]
fn repeated_sentence_ending_reports_a_long_run_once() {
    let morphology = Morphology::new().unwrap();
    let body =
        "Aの値は十です。Bの値は二十です。Cの値は三十です。Dの値は四十です。Eの値は五十です。\n";
    let found = findings(&morphology, body, "repeated_sentence_ending");
    assert_eq!(found.len(), 1, "{found:?}");
    // 3文目「Cの値は三十です」の「です」
    let third = body.match_indices("です").nth(2).unwrap().0;
    assert_eq!(found[0].span.unwrap().start_byte, third);
}

#[test]
fn repeated_sentence_ending_ignores_other_endings_and_breaks() {
    let morphology = Morphology::new().unwrap();
    for body in [
        // 過去の「た」は「だ」として数えない
        "本を読んだ。手紙を書いた。駅まで歩いた。\n",
        "本を読んだ。手紙を読んだ。雑誌も読んだ。\n",
        "時間がありません。予算もありません。人もいません。\n",
        "とても静かだった。誰もいなかった。風だけが強かった。\n",
        // 形が混ざる
        "この設定は安全です。手順を確認します。最後の確認も短時間です。\n",
        // 空行・見出し・箇条書き・引用・コードで連続を切る
        "この設定は安全です。次の手順も簡単です。\n\n最後の確認も短時間です。\n",
        "この設定は安全です。次の手順も簡単です。\n## 確認\n最後の確認も短時間です。\n",
        "この設定は安全です。次の手順も簡単です。\n- 項目\n最後の確認も短時間です。\n",
        "この設定は安全です。次の手順も簡単です。\n> 引用です。\n最後の確認も短時間です。\n",
        "この設定は安全です。次の手順も簡単です。\n```\ncode\n```\n最後の確認も短時間です。\n",
        // 台詞の中の文末は数えない
        "「それは本当です」\n「私も見ました、本当です」\n「みんな知っています、本当です」\n",
        // 「のだ」は別の形（repeated_sentence_modeが従来どおり扱う）
        "それが理由なのだ。だから続けるのだ。結局そうなるのだ。\n",
    ] {
        let found = findings(&morphology, body, "repeated_sentence_ending");
        assert!(found.is_empty(), "{body}: {found:?}");
    }
}

#[test]
fn repeated_sentence_ending_skips_sentences_shorter_than_four_characters() {
    let morphology = Morphology::new().unwrap();
    // 「そうです」は4字なので数え、「です」だけの文は数えない（連続も切らない）。
    let counted = "この設定は安全です。そうです。最後の確認も短時間です。\n";
    assert_eq!(
        findings(&morphology, counted, "repeated_sentence_ending").len(),
        1
    );
    let skipped = "この設定は安全です。です。最後の確認も短時間です。\n";
    assert!(findings(&morphology, skipped, "repeated_sentence_ending").is_empty());
}

#[test]
fn repeated_sentence_mode_leaves_runs_covered_by_sentence_endings() {
    let morphology = Morphology::new().unwrap();
    let covered = concat!(
        "この仕組みは入力された記録を順番に比較し、差分を一覧へまとめて表示する設計である。\n",
        "この処理は保存された設定を項目ごとに検査し、不正な値を理由とともに報告する設計である。\n",
        "この機能は収集した測定値を期間別に集計し、変化を追跡できる形で提示する設計である。\n",
        "処理結果の確認。\n",
        "設定項目の再点検。\n",
        "変更内容の記録。\n",
    );
    assert!(findings(&morphology, covered, "repeated_sentence_mode").is_empty());
    assert_eq!(
        findings(&morphology, covered, "repeated_sentence_ending").len(),
        1
    );
    // 「だった」は文末の連続に数えないため、従来の規則が示す。
    let past = covered.replace("設計である。", "設計だった。");
    assert_eq!(
        findings(&morphology, &past, "repeated_sentence_mode").len(),
        1
    );
    assert!(findings(&morphology, &past, "repeated_sentence_ending").is_empty());
}

// ---- W3 negated_contrast ----

#[test]
fn negated_contrast_points_at_each_contrast() {
    let morphology = Morphology::new().unwrap();
    for (body, count, first) in [
        ("成功の鍵は才能ではなく、習慣だ。\n", 1, "ではなく、習慣だ"),
        (
            "これは終わりではなく始まりです。\n",
            1,
            "ではなく始まりです",
        ),
        (
            "問題は量ではなく質で、速さではなく正確さだ。\n",
            2,
            "ではなく質で",
        ),
        (
            "これは失敗ではない。次への準備だ。\n",
            1,
            "ではない。次への準備だ",
        ),
        (
            "大事なのは速さではありません。\n続けることです。\n",
            1,
            "ではありません。続けることです",
        ),
    ] {
        let found = findings(&morphology, body, "negated_contrast");
        assert_eq!(found.len(), count, "{body}: {found:?}");
        assert_eq!(found[0].excerpt, first, "{body}");
        assert!(found[0].span.is_some(), "{body}");
    }
    let two_lines = findings(
        &morphology,
        "大事なのは速さではありません。\n続けることです。\n",
        "negated_contrast",
    );
    assert_eq!(two_lines[0].related_lines.as_deref(), Some(&[1, 2][..]));
}

#[test]
fn negated_contrast_skips_state_changes_additions_and_lone_negation() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "この方法は有効ではなくなった。\n",
        "この部屋は会議室ではなくなります。\n",
        "彼は医者ではなく、作家でもある。\n",
        "その案は現実的ではない。\n",
        "その案は現実的ではない。予算もない。\n",
        "これは正しくないのではないか。\n",
        "その案は現実的ではない。\n\n次の案を出す。\n",
        "その案は現実的ではない。だから見送る。\n",
        "設定は`ではなく`と書く。\n",
        // 付け足し・二重否定・部分否定は、否定を外すと意味が変わる
        "自社だけではなく、業界全体が変わる。\n",
        "東京のみではなく大阪でも開く。\n",
        "日本語で作ることも不可能ではない。効果はほとんど同じである。\n",
        "その違いは少なくない。むしろ大きな差だ。\n",
        "それは必ずしも失敗ではない。次の機会だ。\n",
    ] {
        let found = findings(&morphology, body, "negated_contrast");
        assert!(found.is_empty(), "{body}: {found:?}");
    }
}

#[test]
fn antithesis_repetition_summarizes_the_document_without_a_span() {
    let morphology = Morphology::new().unwrap();
    let body = "Aではなく、Bです。\nCではなく、Dです。\nEではなく、Fです。\n";
    let summary = findings(&morphology, body, "antithesis_repetition");
    assert_eq!(summary.len(), 1);
    assert!(summary[0].span.is_none());
    assert_eq!(summary[0].related_lines.as_deref(), Some(&[1, 2, 3][..]));
    assert_eq!(findings(&morphology, body, "negated_contrast").len(), 3);
}

// ---- W4 short_topic_comma / short_subject_comma / short_object_comma ----

fn comma_findings(morphology: &Morphology, body: &str) -> Vec<(String, String)> {
    lint::analyze(body, morphology, None, true)
        .unwrap()
        .findings
        .into_iter()
        .filter(|finding| {
            matches!(
                finding.category.as_str(),
                "short_topic_comma" | "short_subject_comma" | "short_object_comma"
            )
        })
        .map(|finding| {
            assert_eq!(excerpt_of_span(body, &finding), "、");
            (finding.category, finding.excerpt)
        })
        .collect()
}

#[test]
fn short_phrase_commas_cover_every_comma_and_longer_noun_phrases() {
    let morphology = Morphology::new().unwrap();
    for (body, category, excerpt) in [
        (
            "この章の目的は、手順の確認です。",
            "short_topic_comma",
            "この章の目的は、",
        ),
        (
            "東京の店舗では、来月から制度を試す。",
            "short_topic_comma",
            "東京の店舗では、",
        ),
        (
            "大阪からは、明日の便で向かう。",
            "short_topic_comma",
            "大阪からは、",
        ),
        ("社員も、会議に参加する。", "short_topic_comma", "社員も、"),
        (
            "新システムが、処理を引き継ぐ。",
            "short_subject_comma",
            "新システムが、",
        ),
        (
            "対象のファイルを、一つずつ確認する。",
            "short_object_comma",
            "対象のファイルを、",
        ),
        // 文の最初の読点だけでなく、後ろの読点も見る。
        (
            "しかし、この方法は、時間を要する。",
            "short_topic_comma",
            "この方法は、",
        ),
    ] {
        assert_eq!(
            comma_findings(&morphology, body),
            vec![(category.to_owned(), excerpt.to_owned())],
            "{body}: {:?}",
            morphology.tokenize(body).unwrap()
        );
    }
}

#[test]
fn short_phrase_commas_keep_commas_that_carry_structure() {
    let morphology = Morphology::new().unwrap();
    for body in [
        // 副詞にもなる名詞、指示語・人称だけ、接続の句
        "今回は、手順を見直した。",
        "現在は、別の方式を使う。",
        "それは、正しい判断だ。",
        "私は、反対だ。",
        "中でも、この点を確かめたい。",
        "具体的には、手順を変える。",
        // 並べた名詞の最後と、同じ助詞の句の並び
        "東京、大阪、名古屋は、晴れの予報だ。",
        "開発チームも、運用チームも、同じ手順を使う。",
        // 動詞・形容詞を含む句、15字を超える句
        "昨日届いた荷物は、倉庫に置いた。",
        "古い設定は、削除した。",
        "国際標準化機構の情報セキュリティ管理規格の要求事項は、三つある。",
        // 後ろに別の主語が続く、引用の中
        "この方法は、手間がかかる。",
        // 括弧を挟んでも後ろの連体修飾を見る（主題が遠い述語に掛かることを読点が示す）
        "政府は、1994年に閣議決定された「指針」を改めた。",
        "この記事は、検索ノイズを嫌がる人に向けて書いた。",
        "「要点は、三つです」と話した。",
    ] {
        let found = comma_findings(&morphology, body);
        assert!(found.is_empty(), "{body}: {found:?}");
    }
}

// ---- W5 abstract_metaphor / vague_sensory_term / technical_jargon_metaphor ----

#[test]
fn abstract_metaphor_covers_foundations_and_compasses_of_abstract_things() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "計画そのものが対話の土台となり、判断をそろえる。",
        "売上の拡大だけでは、その土台は十分に育ちません。",
        "担当者の判断の土台を揃える。",
        "この領域は、顧客データの活用を土台に、売上を伸ばす。",
        "手を動かし続けた時間が、いまの事業の土台になっています。",
        "中深煎りの豆が、やわらかな苦みを土台に、甘い余韻を重ねます。",
        "お客様の声は、進む方向を確かめる羅針盤になります。",
        "監視は現在地を示す羅針盤であり、計画を支える。",
    ] {
        let found = findings(&morphology, body, "abstract_metaphor");
        assert_eq!(found.len(), 1, "{body}: {found:?}");
    }
    for body in [
        "移行先には、オブジェクトストレージを土台にした構成を選んだ。",
        "土台作りとしてのインプットを続ける。",
        "テストの土台作りから始めた。",
        "絵を土台にしてお話を作った。",
        "地震で家の土台が傾いた。",
        "建物の土台を補強する計画だ。",
        "航海では羅針盤が頼りになる。",
    ] {
        let found = findings(&morphology, body, "abstract_metaphor");
        assert!(found.is_empty(), "{body}: {found:?}");
    }
}

#[test]
fn vague_sensory_term_covers_warmth_and_heat_of_non_bodies() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "言葉の体温が伝わる文章だ。",
        "組織の熱量が下がっている。",
        "熱量のある文章を書きたい。",
        "議論の熱量が高い。",
    ] {
        let found = findings(&morphology, body, "vague_sensory_term");
        assert_eq!(found.len(), 1, "{body}: {found:?}");
    }
    for body in [
        "冷たい水が指先から体温を奪っていく。",
        "朝起きたら体温を測る。",
        "赤ちゃんの体温は大人より高い。",
        "この料理の熱量は五百キロカロリーだ。",
        "体温が三十八度を超えた。",
        "記憶は体温に触れて目を覚ます。",
    ] {
        let found = findings(&morphology, body, "vague_sensory_term");
        assert!(found.is_empty(), "{body}: {found:?}");
    }
}

#[test]
fn technical_jargon_metaphor_covers_effective_rules_and_entry_points() {
    let morphology = Morphology::new().unwrap();
    for (body, excerpt) in [
        ("古いログを移すルールが、効いたためだ。", "ルールが、効い"),
        ("あとで振り返るとき、この区別が効いてくる。", "区別が効い"),
        ("小さな工夫も効く。", "工夫も効く"),
        (
            "改修は、請求計算の入口を一つ用意するところから始めた。",
            "請求計算の入口",
        ),
        (
            "利用者の操作を入口に、呼ばれる処理と外部への通信をたどった。",
            "操作を入口",
        ),
        (
            "外部連携の窓口を、このモジュールに集める。",
            "外部連携の窓口",
        ),
    ] {
        let found = findings(&morphology, body, "technical_jargon_metaphor");
        assert_eq!(found.len(), 1, "{body}: {found:?}");
        assert_eq!(found[0].excerpt, excerpt, "{body}");
    }
    for body in [
        "薬が効いた。",
        "冷房が効いた部屋で作業する。",
        "わさびが効いている。",
        "このルールは効かない。",
        "濡れた傘を入口の袋に収める。",
        "問い合わせ窓口でデータの扱いを確認する。",
        "駅の入口でデータを確認した。",
        "玄関を出ると、空気が冷たい。",
        "上履きのまま玄関へ歩きだした。",
    ] {
        let found = findings(&morphology, body, "technical_jargon_metaphor");
        assert!(found.is_empty(), "{body}: {found:?}");
    }
}
