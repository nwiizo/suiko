//! 物・感覚・時間・性質の動作主、圧縮した標語、場面付きの直喩を自作の例で固定する。

use suiko::{lint, morphology::Morphology};

const CATEGORIES: &[&str] = &[
    "sensory_role_mismatch",
    "explanatory_metaphor",
    "compressed_relation",
    "copy_fragment",
    "elaborate_simile",
];

fn findings(body: &str, morphology: &Morphology, experimental: bool) -> Vec<lint::Finding> {
    lint::analyze(body, morphology, None, experimental)
        .unwrap()
        .findings
        .into_iter()
        .filter(|finding| CATEGORIES.contains(&finding.category.as_str()))
        .collect()
}

fn assert_single(
    body: &str,
    morphology: &Morphology,
    category: &str,
    excerpt: &str,
    experimental: bool,
) {
    let found = findings(body, morphology, experimental);
    assert_eq!(
        found.len(),
        1,
        "{body}: {found:?} {:?}",
        morphology.tokenize(body).unwrap()
    );
    assert_eq!(found[0].category, category, "{body}");
    assert_eq!(found[0].excerpt, excerpt, "{body}");
    assert_eq!(found[0].severity, "info");
    assert!(found[0].suggestion.is_none());
    let span = found[0].span.unwrap();
    let line = body.lines().nth(span.start_line - 1).unwrap();
    assert_eq!(&line[span.start_byte..span.end_byte], excerpt, "{body}");
}

#[test]
fn agents_point_at_the_subject_and_its_action() {
    let morphology = Morphology::new().unwrap();
    for (body, category, excerpt) in [
        (
            "台所の鏡が、その声を受け止めた。",
            "explanatory_metaphor",
            "鏡が、その声を受け止め",
        ),
        (
            "古い棚のほうが、私の手を待っていた。",
            "explanatory_metaphor",
            "棚のほうが、私の手を待っ",
        ),
        (
            "扉が隣の鈴を鳴らしていた。",
            "explanatory_metaphor",
            "扉が隣の鈴を鳴らし",
        ),
        (
            "換気扇が首を振っている。",
            "explanatory_metaphor",
            "換気扇が首を振っ",
        ),
        (
            "ベンチが、沈んでいた背中の形を戻す。",
            "explanatory_metaphor",
            "ベンチが、沈んでいた背中の形を戻す",
        ),
        (
            "廊下の空気が私を見送る。",
            "sensory_role_mismatch",
            "空気が私を見送る",
        ),
        (
            "通りの明かりが一つだけ起きている。",
            "sensory_role_mismatch",
            "明かりが一つだけ起き",
        ),
        (
            "石鹸の香りが、昨日の会話を拭き取っていた。",
            "sensory_role_mismatch",
            "香りが、昨日の会話を拭き取っ",
        ),
        (
            "棚にたまった午後がゆっくり揺れた。",
            "sensory_role_mismatch",
            "午後がゆっくり揺れ",
        ),
        (
            "床の冷たさが靴下を通して膝へ這い上がる。",
            "sensory_role_mismatch",
            "冷たさが靴下を通して膝へ這い上がる",
        ),
        (
            "湯の温もりが、まだ廊下に漂っている。",
            "sensory_role_mismatch",
            "温もりが、まだ廊下に漂っ",
        ),
        (
            "紙の凹凸が、爪の間に入り込んだ。",
            "sensory_role_mismatch",
            "凹凸が、爪の間に入り込ん",
        ),
        (
            "ハンガーだけが袖の形を覚えている。",
            "explanatory_metaphor",
            "ハンガーだけが袖の形を覚え",
        ),
        // 家具・台所用品・文具の語。副詞の「〜と」を越えて述語を探す。
        (
            "古いソファが、帰ってくる主人を待っている。",
            "explanatory_metaphor",
            "ソファが、帰ってくる主人を待っ",
        ),
        (
            "台所のやかんが、冷めた会話を抱えていた。",
            "explanatory_metaphor",
            "やかんが、冷めた会話を抱え",
        ),
        (
            "本棚がぼんやりと首を振った。",
            "explanatory_metaphor",
            "本棚がぼんやりと首を振っ",
        ),
        (
            "白いコップが、しっかりと私の話を受け止めた。",
            "explanatory_metaphor",
            "コップが、しっかりと私の話を受け止め",
        ),
        (
            "使い古した万年筆が、旅立つ友を見送った。",
            "explanatory_metaphor",
            "万年筆が、旅立つ友を見送っ",
        ),
    ] {
        assert_single(body, &morphology, category, excerpt, false);
    }
}

#[test]
fn agents_keep_literal_motion_people_and_other_clauses() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "窓が風で揺れた。",
        "鏡が割れた。",
        "母が私を見送った。",
        "扇風機が首を振っている。",
        "風が風鈴を鳴らしていた。",
        "光が部屋を照らした。",
        "明かりが消えた。",
        "香りが部屋に広がった。",
        "夜が明けた。",
        "冷たさが残った。",
        "温もりが伝わった。",
        "寒さが身にしみる。",
        "鏡を見ながら、彼が声を受け止めた。",
        "窓が開くと、母が鈴を鳴らした。",
        "棚は私の手を待っていない。",
        "「鏡が声を受け止めた」と書いた。",
        "鏡の前で彼は客を待っていた。",
        "ソファで、父が主人を待っている。",
        "やかんを抱えて台所を出た。",
        "彼はぼんやりと首を振った。",
        "ソファが届くのを待っている。",
        "コップが割れたので、新しいものを待っている。",
        "本棚はゆっくりと倒れた。",
    ] {
        assert!(
            findings(body, &morphology, true).is_empty(),
            "{body}: {:?}",
            findings(body, &morphology, true)
        );
    }
}

#[test]
fn compressed_slogans_are_flagged_only_without_objects_or_reasons() {
    let morphology = Morphology::new().unwrap();
    for (body, excerpt) in [
        ("迷ったら設計書に戻して決める", "迷ったら設計書に戻し"),
        (
            "判断に迷ったら、チームの担当者に戻る",
            "判断に迷ったら、チームの担当者に戻る",
        ),
        ("迷ったら最初の30日で考える", "迷ったら最初の30日で考える"),
        ("運用は仕組みで守る", "仕組みで守る"),
        ("優先度はデータで決める。", "データで決める"),
        ("費用の上限を現実で測る", "現実で測る"),
        ("触らず、待って、戻す", "触らず、待って、戻す"),
        ("早く測り、続けて改善する", "早く測り、続けて改善する"),
        ("毎月確認して返す", "毎月確認して返す"),
        ("決めた人が、持つ", "決めた人が、持つ"),
    ] {
        assert_single(body, &morphology, "compressed_relation", excerpt, false);
    }
    assert_single(
        "同じ扉から、同じ部屋へ",
        &morphology,
        "copy_fragment",
        "同じ扉から、同じ部屋へ",
        false,
    );
    for body in [
        "迷ったら、人を守る",
        "迷ったら、やめる前に問い直す",
        "判断は感覚ではなく、同じ指標で行う",
        "仕組みで守るべき範囲を、障害の記録から決める。",
        "本を借りたら、来週までに図書館へ返す。",
        "鍵は私が持つ",
        "借りた本を返す",
        "腑抜けとなり、何も感じなくなった状態である。",
        "走り、跳び、ゴールに着く。",
        "同じ扉から、同じ部屋へ入った",
        "## 同じ扉から、同じ部屋へ",
    ] {
        assert!(
            findings(body, &morphology, true).is_empty(),
            "{body}: {:?}",
            findings(body, &morphology, true)
        );
    }
}

#[test]
fn elaborate_similes_are_experimental_and_skip_references() {
    let morphology = Morphology::new().unwrap();
    for (body, excerpt) in [
        (
            "夜の廊下は、閉め忘れた冷蔵庫のように明るかった。",
            "閉め忘れた冷蔵庫のよう",
        ),
        (
            "その声は、割れた皿の縁のように細かった。",
            "割れた皿の縁のよう",
        ),
        (
            "灯りは、池に沈んだ最後の一枚のように揺れた。",
            "沈んだ最後の一枚のよう",
        ),
        (
            "湯気が、眠れない犬の鼻先みたいに震えた。",
            "眠れない犬の鼻先みたい",
        ),
    ] {
        assert!(findings(body, &morphology, false).is_empty(), "{body}");
        assert_single(body, &morphology, "elaborate_simile", excerpt, true);
    }
    for body in [
        "前に示した例のように設定する。",
        "上で説明した手順のように進める。",
        "失敗しないように確認する。",
        "薄い硝子のように揺れた。",
        "冷蔵庫の心臓のように脈を打つ。",
        "答えが出ないまま教科書のように読んだ。",
        "魔法という言葉のように響いた。",
        "「割れた皿の縁のように細い」と書いた。",
    ] {
        assert!(
            findings(body, &morphology, true).is_empty(),
            "{body}: {:?}",
            findings(body, &morphology, true)
        );
    }
}
