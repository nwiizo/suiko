//! 文の構造と、否定対比・因果・選択の範囲の変化。

use std::sync::LazyLock;

use regex::Regex;

use super::units::{Unit, UnitKind};
use super::{DiffFinding, finding, join_items};
use crate::morphology::Morpheme;

/// 箇条書きと地の文の入れ替わり、見出しの階層の変化。
pub(super) fn structure_findings(before: &Unit, after: &Unit, findings: &mut Vec<DiffFinding>) {
    let detail = match (before.kind, after.kind) {
        (UnitKind::List, UnitKind::Prose) => {
            "箇条書きの項目が地の文になりました。並列の走査性が要る一覧なら箇条書きのまま残してください。".to_owned()
        }
        (UnitKind::Prose, UnitKind::List) => {
            "地の文が箇条書きになりました。因果や経緯が項目の間に隠れていないか確認してください。".to_owned()
        }
        (UnitKind::Heading, UnitKind::Heading) if before.level != after.level => format!(
            "見出しの階層が{}から{}に変わりました。節の包含関係が意図どおりか確認してください。",
            "#".repeat(before.level),
            "#".repeat(after.level)
        ),
        _ => return,
    };
    findings.push(finding(
        "structure_changed",
        "info",
        &[before],
        &[after],
        detail,
    ));
}

// 「XではなくY」の否定対比。
static CONTRAST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"([^、。「」（）\s]{1,20}?)(?:では|じゃ|で)な(?:く|くて)、?\s*([^、。\s]{1,20})")
        .expect("valid contrast regex")
});

fn contrasts(text: &str) -> Vec<(String, String)> {
    CONTRAST
        .captures_iter(text)
        .map(|captures| (captures[1].to_owned(), captures[2].to_owned()))
        .collect()
}

fn overlaps(left: &str, right: &str) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

/// 否定対比の逆転（warn）と、否定した側の消失（info）。
pub(super) fn contrast_findings(before: &Unit, after: &Unit, findings: &mut Vec<DiffFinding>) {
    let content = |text: &str| {
        text.chars()
            .filter(|ch| !ch.is_whitespace() && *ch != '、')
            .collect::<String>()
    };
    if content(&before.text) == content(&after.text) {
        return;
    }
    let before_pairs = contrasts(&before.text);
    if before_pairs.is_empty() {
        return;
    }
    let after_pairs = contrasts(&after.text);
    for (denied, chosen) in &before_pairs {
        let reversed = after_pairs.iter().find(|(denied_after, chosen_after)| {
            overlaps(denied_after, chosen) && overlaps(chosen_after, denied)
        });
        if let Some((denied_after, chosen_after)) = reversed {
            findings.push(finding(
                "contrast_changed",
                "warn",
                &[before],
                &[after],
                format!(
                    "否定対比の向きが逆になっています。前は「{denied}」を否定して「{chosen}」を選び、後は「{denied_after}」を否定して「{chosen_after}」を選んでいます。"
                ),
            ));
            return;
        }
        if after_pairs.is_empty() && !after.text.contains(denied.as_str()) {
            findings.push(finding(
                "contrast_changed",
                "info",
                &[before],
                &[after],
                format!(
                    "「{denied}ではなく{chosen}」の否定した側「{denied}」が消えました。誤解の訂正や比較として必要な対比なら残してください。"
                ),
            ));
            return;
        }
    }
}

// 因果の接続。
fn causal_markers(tokens: &[Morpheme]) -> Vec<String> {
    let mut marks = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let surface = token.surface.as_str();
        let previous = index.checked_sub(1).map(|index| &tokens[index]);
        let next = tokens.get(index + 1);
        let after_predicate =
            previous.is_some_and(|token| matches!(token.pos(0), "動詞" | "形容詞" | "助動詞"));
        let causal = match token.pos(0) {
            "助詞" => {
                token.pos(1) == "接続助詞" && matches!(surface, "ので" | "から") && after_predicate
            }
            "名詞" => {
                (matches!(surface, "ため" | "為")
                    && (after_predicate || previous.is_some_and(|token| token.surface == "の")))
                    || matches!(surface, "おかげ" | "せい" | "原因" | "要因")
            }
            "接続詞" => matches!(
                surface,
                "そのため"
                    | "このため"
                    | "したがって"
                    | "従って"
                    | "だから"
                    | "なぜなら"
                    | "よって"
                    | "ゆえに"
                    | "その結果"
            ),
            "動詞" => {
                token.dictionary_form() == "よる"
                    && previous.is_some_and(|token| token.surface == "に")
                    && !next.is_some_and(|token| token.surface == "と")
            }
            _ => false,
        };
        if causal {
            marks.push(if token.dictionary_form() == "よる" {
                format!("に{surface}")
            } else {
                surface.to_owned()
            });
        }
    }
    marks
}

// 選ぶ範囲。どれか一つを選ぶ言い方と、全部を求める言い方。
const ONE_OF_WORDS: &[&str] = &[
    "または",
    "もしくは",
    "あるいは",
    "いずれか",
    "どちらか",
    "どれか",
];
const ALL_OF_WORDS: &[&str] = &["両方", "すべて", "全て", "いずれも", "どちらも", "全部"];

fn choice_scope(unit: &Unit) -> (bool, bool) {
    let has = |words: &[&str]| words.iter().any(|word| unit.text.contains(word));
    (has(ONE_OF_WORDS), has(ALL_OF_WORDS))
}

/// 「AまたはBのどちらか」が「AとBの両方」になった（逆も）文を示す。片側に両方の言い方がある文は比べない。
pub(super) fn choice_findings(before: &Unit, after: &Unit, findings: &mut Vec<DiffFinding>) {
    let direction = match (choice_scope(before), choice_scope(after)) {
        ((true, false), (false, true)) => {
            "どれか一つを選ぶ言い方が、全部を求める言い方になりました"
        }
        ((false, true), (true, false)) => {
            "全部を求める言い方が、どれか一つを選ぶ言い方になりました"
        }
        _ => return,
    };
    findings.push(finding(
        "choice_changed",
        "warn",
        &[before],
        &[after],
        format!("{direction}。求める範囲が原文どおりか確認してください。"),
    ));
}

pub(super) fn causal_findings(before: &Unit, after: &Unit, findings: &mut Vec<DiffFinding>) {
    let before_marks = causal_markers(&before.tokens);
    let after_marks = causal_markers(&after.tokens);
    if before_marks.len() == after_marks.len() {
        return;
    }
    // 関連・相関だけを述べた文を因果にした書き換えは、主張が強まるので warn にする。
    let strengthened = after_marks.len() > before_marks.len()
        && ["関連", "相関", "関係"]
            .iter()
            .any(|word| before.text.contains(word) && !after.text.contains(word));
    let direction = if strengthened {
        "関連・相関を述べた文が、因果を述べる文になりました"
    } else if after_marks.len() > before_marks.len() {
        "原文にない因果のつながりが増えました"
    } else {
        "因果のつながりが減りました"
    };
    findings.push(finding(
        "causal_changed",
        if strengthened { "warn" } else { "info" },
        &[before],
        &[after],
        format!(
            "{direction}（前: {} / 後: {}）。原因と結果の関係に原文や資料の裏付けがあるか確認してください。",
            join_items(&before_marks),
            join_items(&after_marks)
        ),
    ));
}
