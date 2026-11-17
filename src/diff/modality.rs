//! 確度・頻度・条件を表す語の増減。

use super::units::Unit;
use super::{DiffFinding, finding, item_change, join_items, multiset_diff};
use crate::morphology::Morpheme;

// 確度と範囲を示す語。弱める語が消える、強める語が増えると主張が強まる。
const WEAKENERS: &[&str] = &[
    "未確認",
    "未定",
    "未決",
    "未検証",
    "検討中",
    "調査中",
    "確認中",
    "対応中",
    "作業中",
    "予定",
    "暫定",
    "仮に",
    "仮の",
    "想定",
    "推定",
    "概算",
    "目安",
    "一部",
    "原則として",
    "基本的に",
    "おおむね",
    "概ね",
    "場合がある",
    "場合があります",
    "ことがある",
    "ことがあります",
    "傾向がある",
    "傾向があります",
];
const STRENGTHENERS: &[&str] = &[
    "確定",
    "必須",
    "確認済み",
    "対応済み",
    "検証済み",
    "必ず",
    "常に",
    "全て",
    "すべて",
    "絶対に",
    "確実に",
    "完全に",
];
// 推量の言い方。同じ強さの留保として1つにまとめ、互いの言い換え（かもしれない→恐れがある）は示さない。
const HEDGES: &[&str] = &[
    "かもしれ",
    "かも知れ",
    "と思われ",
    "と考えられ",
    "と見られ",
    "見込み",
    "恐れがあ",
    "おそれがあ",
    "可能性があ",
    "だろう",
    "でしょう",
    "おそらく",
    "恐らく",
    "たぶん",
    "多分",
];
const HEDGE: &str = "推量の言い方";

/// `start`から始まる連続した形態素の表層が`word`と一致するか。「正常に」の中の「常に」のように
/// 語の途中から一致する形を除くため、文字列検索ではなく形態素の境界で照合する。
pub(super) fn phrase_at(tokens: &[Morpheme], start: usize, word: &str) -> bool {
    let mut joined = String::new();
    for token in &tokens[start..] {
        joined.push_str(&token.surface);
        if joined.len() >= word.len() {
            return joined == word;
        }
        if !word.starts_with(joined.as_str()) {
            return false;
        }
    }
    false
}

/// 形態素の境界で一致した語を、出現ごとに並べる。
fn phrases(units: &[&Unit], words: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    for unit in units {
        for start in 0..unit.tokens.len() {
            for word in words {
                if phrase_at(&unit.tokens, start, word) {
                    found.push((*word).to_owned());
                }
            }
        }
    }
    found
}

fn certainty_words(units: &[&Unit]) -> Vec<String> {
    let mut found = phrases(units, WEAKENERS);
    found.extend(phrases(units, STRENGTHENERS));
    // 「おそらく〜だろう」のように重ねても、1文の推量は1つと数える。
    found.extend(
        units
            .iter()
            .filter(|unit| HEDGES.iter().any(|hedge| unit.text.contains(hedge)))
            .map(|_| HEDGE.to_owned()),
    );
    found
}

pub(super) fn certainty_findings(
    before: &[&Unit],
    after: &[&Unit],
    findings: &mut Vec<DiffFinding>,
) {
    let (lost, added) = multiset_diff(&certainty_words(before), &certainty_words(after));
    if lost.is_empty() && added.is_empty() {
        return;
    }
    let weakener = |word: &&String| WEAKENERS.contains(&word.as_str()) || *word == HEDGE;
    let stronger = lost
        .iter()
        .filter(weakener)
        .chain(
            added
                .iter()
                .filter(|word| STRENGTHENERS.contains(&word.as_str())),
        )
        .cloned()
        .collect::<Vec<_>>();
    let weaker = added
        .iter()
        .filter(weakener)
        .chain(
            lost.iter()
                .filter(|word| STRENGTHENERS.contains(&word.as_str())),
        )
        .cloned()
        .collect::<Vec<_>>();
    findings.push(finding(
        "certainty_changed",
        "warn",
        before,
        after,
        format!(
            "確度や範囲を示す語が変わりました。主張が強まる変化: {} / 弱まる変化: {}。予定・暫定・一部などの留保を落としていないか、根拠なく断定を加えていないか確認してください。",
            join_items(&stronger),
            join_items(&weaker)
        ),
    ));
}

// 頻度と周期。「常に」は確度の語として別に比べる。
const FREQUENCY_WORDS: &[&str] = &[
    "毎回",
    "毎時",
    "毎日",
    "毎朝",
    "毎晩",
    "毎週",
    "毎月",
    "毎年",
    "隔日",
    "隔週",
    "隔月",
    "常時",
    "随時",
    "都度",
    "いつも",
    "ときどき",
    "時々",
    "時折",
    "たまに",
    "まれに",
    "稀に",
    "しばしば",
    "頻繁に",
    "定期的に",
    "一度だけ",
];
// 同じ頻度の言い換え。比べる前に左の語を右の語にそろえる。
const FREQUENCY_SYNONYMS: &[(&str, &str)] = &[
    ("都度", "毎回"),
    ("時々", "ときどき"),
    ("稀に", "まれに"),
    ("常時", "いつも"),
];

/// 頻度の語が替わった・消えた書き換えは、実行や発生の回数が変わるので warn、増えただけなら info。
pub(super) fn frequency_findings(
    before: &[&Unit],
    after: &[&Unit],
    findings: &mut Vec<DiffFinding>,
) {
    let frequencies = |units: &[&Unit]| {
        phrases(units, FREQUENCY_WORDS)
            .into_iter()
            .map(|word| {
                FREQUENCY_SYNONYMS
                    .iter()
                    .find(|(synonym, _)| *synonym == word)
                    .map_or(word, |(_, canonical)| (*canonical).to_owned())
            })
            .collect::<Vec<_>>()
    };
    let (mut lost, mut added) = multiset_diff(&frequencies(before), &frequencies(after));
    // 「毎回確認する」→「確認するたびに」は同じ頻度。「たびに」だけを足した文は条件の言い方として数えない。
    let every = |units: &[&Unit]| !phrases(units, &["たびに", "度に"]).is_empty();
    if every(after) {
        lost.retain(|word| word != "毎回");
    }
    if every(before) {
        added.retain(|word| word != "毎回");
    }
    findings.extend(item_change(
        "frequency_changed",
        if lost.is_empty() { "info" } else { "warn" },
        before,
        after,
        &lost,
        &added,
        |lost, added| {
            format!(
                "頻度や周期を表す語が変わりました。消えた語: {lost} / 増えた語: {added}。実行や発生の回数が原文と同じか確認してください。"
            )
        },
    ));
}

// 条件・限定・例示。言い換え（場合→とき、など→といった）は同じ種類として数える。
const CONDITION_KINDS: [&str; 3] = ["条件を表す語", "限定・例外を表す語", "例示を表す語"];
const EXAMPLE: usize = 2;

/// 条件・限定・例示の語を、`CONDITION_KINDS`の順に集める。
fn condition_markers(tokens: &[Morpheme]) -> [Vec<String>; 3] {
    let [mut conditional, mut restrictive, mut example] = [Vec::new(), Vec::new(), Vec::new()];
    for (index, token) in tokens.iter().enumerate() {
        let surface = token.surface.as_str();
        let previous = index.checked_sub(1).map(|index| &tokens[index]);
        let after_predicate =
            previous.is_some_and(|token| matches!(token.pos(0), "動詞" | "形容詞" | "助動詞"));
        let next = tokens.get(index + 1);
        // 「〜するとよい」の「と」は勧めの型として文末の働きで見る。
        let advice = surface == "と"
            && next
                .is_some_and(|token| matches!(token.dictionary_form(), "よい" | "いい" | "良い"));
        let is_conditional = match token.pos(0) {
            "助詞" => {
                // 「なければならない」「なくてはいけない」の「ば」は義務の型で、条件ではない。
                let obligation = surface == "ば"
                    && next.is_some_and(|token| {
                        matches!(token.dictionary_form(), "なる" | "いける" | "駄目" | "だめ")
                    });
                token.pos(1) == "接続助詞"
                    && matches!(surface, "ば" | "と")
                    && after_predicate
                    && !advice
                    && !obligation
            }
            "助動詞" => matches!(surface, "たら" | "だら" | "なら" | "ならば" | "れば"),
            "名詞" => {
                (matches!(surface, "場合" | "とき" | "際" | "限り" | "かぎり") && after_predicate)
                    || (surface == "時" && after_predicate)
            }
            _ => false,
        };
        if is_conditional {
            conditional.push(surface.to_owned());
            continue;
        }
        let is_restrictive = match token.pos(0) {
            "助詞" => matches!(surface, "だけ" | "のみ" | "しか" | "ばかり"),
            "名詞" | "接尾辞" => matches!(surface, "以外" | "例外" | "限定"),
            "動詞" => matches!(token.dictionary_form(), "除く" | "限る"),
            "接続詞" => matches!(surface, "ただし" | "但し"),
            _ => false,
        };
        if is_restrictive {
            restrictive.push(surface.to_owned());
            continue;
        }
        // 「PDFなど」の「など」が消えると、例の一覧が全部の一覧に変わる。
        // 「冪等」の「等」のように語の一部になる字は数えない。
        if token.pos(0) == "助詞" && surface == "など" {
            example.push(surface.to_owned());
        } else if let Some(word) = ["といった", "をはじめ"]
            .iter()
            .find(|word| phrase_at(tokens, index, word))
        {
            example.push((*word).to_owned());
        }
    }
    [conditional, restrictive, example]
}

pub(super) fn condition_findings(
    before: &[&Unit],
    after: &[&Unit],
    findings: &mut Vec<DiffFinding>,
) {
    let collect = |units: &[&Unit]| {
        let mut kinds: [Vec<String>; 3] = Default::default();
        for unit in units {
            for (all, found) in kinds.iter_mut().zip(condition_markers(&unit.tokens)) {
                all.extend(found);
            }
        }
        kinds
    };
    let (before_kinds, after_kinds) = (collect(before), collect(after));
    let mut changes = Vec::new();
    let mut lost = false;
    for (kind, ((label, before_words), after_words)) in CONDITION_KINDS
        .iter()
        .zip(&before_kinds)
        .zip(&after_kinds)
        .enumerate()
    {
        if before_words.len() == after_words.len() {
            continue;
        }
        // 例示は、例の語がすべて消えて一覧が閉じたときだけ範囲が変わったとみなす（など→といった、2つ→1つは数えない）。
        lost |= if kind == EXAMPLE {
            after_words.is_empty()
        } else {
            before_words.len() > after_words.len()
        };
        changes.push(format!(
            "{label} {}→{}（前: {} / 後: {}）",
            before_words.len(),
            after_words.len(),
            join_items(before_words),
            join_items(after_words)
        ));
    }
    if changes.is_empty() {
        return;
    }
    // 条件・限定・例示が消えた変化は適用範囲が変わるので warn、増えただけなら info。
    findings.push(finding(
        "condition_changed",
        if lost { "warn" } else { "info" },
        before,
        after,
        format!(
            "{}。適用される条件や例外が変わっていないか確認してください。",
            changes.join("。")
        ),
    ));
}
