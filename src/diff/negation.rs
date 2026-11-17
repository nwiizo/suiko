//! 否定の増減と、文末の述語の肯定・否定の入れ替わり。

use std::sync::LazyLock;

use regex::Regex;

use super::units::{LIGHT_VERBS, Unit, content_lemmas, similarity};
use super::{DiffFinding, finding, join_items};
use crate::morphology::Morpheme;

const NEGATION_SIMILARITY: f64 = 0.6;

// 否定。義務・推量・限定などの慣用句は否定の数に入れない。
static NEGATION_IDIOMS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"かもしれ(?:ない|ません|ぬ)|に違い(?:ない|ありません)|とは限(?:らない|りません)|わけでは(?:ない|ありません)|",
        r"に(?:すぎ|過ぎ)(?:ない|ません)|しか(?:ない|ありません)|ほか(?:ない|ありません)|",
        r"なければ(?:ならない|なりません|いけない|いけません|ならず)|なくては(?:ならない|なりません|いけない|いけません)|",
        r"ないと(?:いけない|いけません|ならない|なりません)|ざるを得(?:ない|ません)|ねばなら(?:ない|ぬ|ず)|",
        r"(?:では|じゃ)な(?:く|くて)|だけでなく|のみならず|問題(?:は|が|も)?(?:ない|なく|ありません)|",
        r"差し支え(?:ない|ありません)|構(?:わない|いません)|かまいません|仕方(?:が|の)?(?:ない|ありません)|",
        r"少なく(?:ない|ありません)|なくない|有無|否か",
    ))
    .expect("valid negation idiom regex")
});
static NEGATION_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"不(?:要|可|明|正|在|備|適|十分|一致|安定|許可|採用|使用|参加|可能|完全)|",
        r"未[一-龥]|非(?:公開|対応|表示|推奨|同期|互換|同意|活性)|無(?:効|料|視|関係|制限|断)|禁止|厳禁",
    ))
    .expect("valid negation prefix regex")
});

fn negation_marks(unit: &Unit) -> Vec<String> {
    let idioms = NEGATION_IDIOMS
        .find_iter(&unit.text)
        .map(|found| found.range())
        .collect::<Vec<_>>();
    let inside_idiom = |byte: usize| idioms.iter().any(|range| range.contains(&byte));
    let mut marks = Vec::new();
    for (index, token) in unit.tokens.iter().enumerate() {
        if inside_idiom(token.byte_start) {
            continue;
        }
        let negative = match token.pos(0) {
            "助動詞" => matches!(token.dictionary_form(), "ない" | "ぬ" | "ず"),
            "形容詞" => matches!(token.dictionary_form(), "ない" | "無い"),
            _ => false,
        };
        if negative {
            // 「ません」は「ませ」＋「ん」に分かれるので、表示は語のまとまりに戻す。
            let previous = index
                .checked_sub(1)
                .map(|index| unit.tokens[index].surface.as_str());
            marks.push(match (previous, token.surface.as_str()) {
                (Some("ませ"), "ん") => "ません".to_owned(),
                _ => token.surface.clone(),
            });
        }
    }
    for found in NEGATION_PREFIX.find_iter(&unit.text) {
        if inside_idiom(found.start()) || matches!(found.as_str(), "未来" | "未明" | "未満") {
            continue;
        }
        marks.push(found.as_str().to_owned());
    }
    marks
}

/// 文末の節（最後の読点より後ろ）に否定の語があるか。
fn tail_negated(unit: &Unit) -> bool {
    let tail_start = unit
        .text
        .rfind('、')
        .map_or(0, |index| index + '、'.len_utf8());
    let idioms = NEGATION_IDIOMS
        .find_iter(&unit.text)
        .map(|found| found.range())
        .collect::<Vec<_>>();
    unit.tokens.iter().any(|token| {
        token.byte_start >= tail_start
            && !idioms.iter().any(|range| range.contains(&token.byte_start))
            && match token.pos(0) {
                "助動詞" => matches!(token.dictionary_form(), "ない" | "ぬ" | "ず"),
                "形容詞" => matches!(token.dictionary_form(), "ない" | "無い"),
                _ => false,
            }
    }) || NEGATION_PREFIX
        .find_iter(&unit.text[tail_start..])
        .any(|found| !matches!(found.as_str(), "未来" | "未明" | "未満"))
}

// 起きることを打ち消す向きの語（回避・消失・停止）。「防ぐ」を「しないようにする」と言い直しても文末の向きは同じとみなす。
const AVOIDANCE_WORDS: &[&str] = &[
    "防ぐ",
    "防げる",
    "避ける",
    "妨げる",
    "免れる",
    "防止",
    "回避",
    "阻止",
    "抑止",
    "無くなる",
    "なくなる",
    "止まる",
    "止める",
    "途絶える",
];

fn is_negative_token(token: &Morpheme) -> bool {
    match token.pos(0) {
        "助動詞" => matches!(token.dictionary_form(), "ない" | "ぬ" | "ず"),
        "形容詞" => matches!(token.dictionary_form(), "ない" | "無い"),
        _ => false,
    }
}

/// 文末の述語（最後の動詞・形容詞と、続く助動詞・接続助詞・補助の語）が打ち消しの向きか。
/// 否定の語と回避の語を合わせた数が奇数なら打ち消しの向きとする（防がない は肯定の向き）。
/// 文末の述語の向き。`avoidance` は回避・消失・停止の語を含むか。
struct PredicatePolarity {
    negated: bool,
    avoidance: bool,
}

fn predicate_polarity(unit: &Unit) -> PredicatePolarity {
    let idioms = NEGATION_IDIOMS
        .find_iter(&unit.text)
        .map(|found| found.range())
        .collect::<Vec<_>>();
    let mut predicate: Vec<&Morpheme> = Vec::new();
    let mut rest = unit.tokens.iter().rev().peekable();
    while let Some(token) = rest.next() {
        // 「未実装です」「不要だ」のように、名詞に「だ・です」が付いただけの述語。
        let copula_only = !predicate.is_empty()
            && predicate.iter().all(|token| {
                matches!(token.pos(0), "補助記号" | "空白")
                    || (token.pos(0) == "助詞" && token.pos(1) == "終助詞")
                    || (token.pos(0) == "助動詞"
                        && matches!(token.dictionary_form(), "だ" | "です" | "である"))
            });
        let keep = match token.pos(0) {
            "補助記号" | "空白" => predicate.is_empty(),
            "動詞" | "形容詞" | "助動詞" | "形状詞" | "接尾辞" => true,
            "助詞" => matches!(token.pos(1), "接続助詞" | "終助詞"),
            // 「確認した」「防止する」のように、サ変の名詞は続く「する」と1つの述語にする。
            "名詞" => {
                copula_only
                    || predicate.last().is_some_and(|next| {
                        matches!(next.dictionary_form(), "する" | "できる" | "出来る")
                    })
            }
            _ => false,
        };
        if !keep {
            break;
        }
        predicate.push(token);
        if token.pos(0) == "名詞" {
            // 「未」「不」が別の形態素に分かれた場合も、名詞と1つの述語にする。
            if let Some(prefix) = rest.next_if(|previous| previous.pos(0) == "接頭辞") {
                predicate.push(prefix);
            }
            break;
        }
    }
    let avoidance = |token: &&Morpheme| AVOIDANCE_WORDS.contains(&token.dictionary_form());
    // 名詞と、前に付いた「未」「不」などの形態素をつないだ字面で、打ち消しの接頭辞を探す。
    let nominal = predicate
        .iter()
        .rev()
        .filter(|token| matches!(token.pos(0), "名詞" | "接頭辞"))
        .map(|token| token.surface.as_str())
        .collect::<String>();
    let prefixed = NEGATION_PREFIX
        .find_iter(&nominal)
        .filter(|found| !matches!(found.as_str(), "未来" | "未明" | "未満"))
        .count();
    let turns = prefixed
        + predicate
            .iter()
            .filter(|token| {
                !idioms.iter().any(|range| range.contains(&token.byte_start))
                    && (is_negative_token(token)
                        || avoidance(token)
                        || (!matches!(token.pos(0), "名詞" | "接頭辞")
                            && NEGATION_PREFIX.is_match(&token.surface)))
            })
            .count();
    PredicatePolarity {
        negated: turns % 2 == 1,
        avoidance: predicate.iter().any(avoidance),
    }
}

/// 打ち消された語（否定の語の前の内容語）の正規化表記。「仕様がない」の「ない」のように
/// 直前が助詞なら、ある・ないを述べる「ない」として語を持たない（None）。
fn negated_words(unit: &Unit) -> Vec<Option<String>> {
    let idioms = NEGATION_IDIOMS
        .find_iter(&unit.text)
        .map(|found| found.range())
        .collect::<Vec<_>>();
    let mut words = Vec::new();
    for (index, token) in unit.tokens.iter().enumerate() {
        if !is_negative_token(token) || idioms.iter().any(|range| range.contains(&token.byte_start))
        {
            continue;
        }
        let word = unit.tokens[..index]
            .iter()
            .rev()
            .find(|previous| {
                // 「送られ」「処理し」「でき」のような助動詞・軽い動詞は飛ばして、その前の語を見る。
                !(previous.pos(0) == "助動詞"
                    || (previous.pos(0) == "動詞"
                        && (previous.pos(1) == "非自立可能"
                            || LIGHT_VERBS.contains(&previous.dictionary_form()))))
            })
            .filter(|previous| content_lemmas(std::slice::from_ref(previous)).len() == 1)
            .map(|previous| previous.normalized().to_owned());
        words.push(word);
    }
    words
}

fn prefix_marks(unit: &Unit) -> usize {
    negation_marks(unit)
        .iter()
        .filter(|mark| NEGATION_PREFIX.is_match(mark))
        .count()
}

/// 前後の両方にある語の打ち消しが増減したか。「通知が消える→通知が送られない」のように、
/// 原文にない語を打ち消して言い直しただけなら false。
fn shared_word_negation_changed(before: &Unit, after: &Unit) -> bool {
    let (before_words, after_words) = (negated_words(before), negated_words(after));
    let count = |words: &[Option<String>], word: &str| {
        words.iter().filter(|w| w.as_deref() == Some(word)).count()
    };
    before_words
        .iter()
        .chain(&after_words)
        .flatten()
        .any(|word| {
            before.lemmas.contains(word)
                && after.lemmas.contains(word)
                && count(&before_words, word) != count(&after_words, word)
        })
        || prefix_marks(before) != prefix_marks(after)
}

/// 対になった文で否定の数が違い、文末の述語の肯定・否定が入れ替わった場合と、字面がほぼ同じで
/// 両方にある語の打ち消しが増減した場合に示す。文全体を書き直して否定の数が変わっただけの文と、
/// 回避の語を「〜ない（ようにする）」で言い直した文は、内容語の変化として読む。
pub(super) fn negation_findings(before: &Unit, after: &Unit, findings: &mut Vec<DiffFinding>) {
    let before_marks = negation_marks(before);
    let after_marks = negation_marks(after);
    if before_marks.len() == after_marks.len() {
        return;
    }
    let (before_predicate, after_predicate) =
        (predicate_polarity(before), predicate_polarity(after));
    let predicate_flipped = before_predicate.negated != after_predicate.negated;
    // 「重複登録を防止する→重複して登録されないようにする」は、回避の語が否定に置き換わっただけ。
    let avoidance_rephrased =
        !predicate_flipped && (before_predicate.avoidance || after_predicate.avoidance);
    let reported = if similarity(before, after) < NEGATION_SIMILARITY {
        predicate_flipped && tail_negated(before) != tail_negated(after)
    } else {
        predicate_flipped || (!avoidance_rephrased && shared_word_negation_changed(before, after))
    };
    if !reported {
        return;
    }
    findings.push(finding(
        "negation_changed",
        "warn",
        &[before],
        &[after],
        format!(
            "否定の語が{}個から{}個になりました（前: {} / 後: {}）。肯定と否定、禁止と許可が入れ替わっていないか確認してください。",
            before_marks.len(),
            after_marks.len(),
            join_items(&before_marks),
            join_items(&after_marks)
        ),
    ));
}
