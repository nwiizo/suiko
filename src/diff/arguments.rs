//! 主語・目的語・通知先・動作主と、内容語・固有名詞の増減。

use std::collections::{BTreeSet, HashSet};
use std::sync::LazyLock;

use regex::Regex;

use super::units::Unit;
use super::{DiffFinding, DocumentValues, finding, item_change, join_items};
use crate::lint::FORMAL_NOUNS;
use crate::morphology::Morpheme;

// 主語・目的語。助詞の直前の名詞を正規化表記で比べる。
/// 助詞の直前の名詞。`form`は照合用の正規化表記、`surface`は表示用。
pub(super) struct Argument {
    pub(super) form: String,
    pub(super) surface: String,
}

fn case_head(tokens: &[Morpheme], particle: usize) -> Option<Argument> {
    let head = tokens[..particle]
        .iter()
        .rev()
        .find(|token| !matches!(token.pos(0), "接尾辞" | "補助記号"))?;
    (head.pos(0) == "名詞"
        && !matches!(head.pos(1), "数詞")
        && !FORMAL_NOUNS.contains(&head.surface.as_str()))
    .then(|| Argument {
        form: head.normalized().to_owned(),
        surface: head.surface.clone(),
    })
}

pub(super) fn case_arguments(tokens: &[Morpheme]) -> (Vec<Argument>, Vec<Argument>) {
    let mut subjects = Vec::new();
    let mut objects = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.pos(0) != "助詞" {
            continue;
        }
        let role = match (token.surface.as_str(), token.pos(1)) {
            ("が", "格助詞") | ("は", "係助詞") => &mut subjects,
            ("を", "格助詞") => &mut objects,
            _ => continue,
        };
        if let Some(head) = case_head(tokens, index) {
            role.push(head);
        }
    }
    (subjects, objects)
}

fn noun_forms(units: &[&Unit]) -> HashSet<String> {
    units
        .iter()
        .flat_map(|unit| unit.tokens.iter())
        .filter(|token| matches!(token.pos(0), "名詞" | "代名詞"))
        .map(|token| token.normalized().to_owned())
        .collect()
}

// 動作主になりやすい名詞の語尾。人・組織・システムを表す主語の追加だけを見る。
const AGENT_ENDINGS: &[&str] = &[
    "者",
    "員",
    "人",
    "手",
    "側",
    "チーム",
    "部",
    "課",
    "係",
    "社",
    "庁",
    "省",
    "局",
    "会",
    "班",
    "先",
    "主",
    "長",
    "師",
    "家",
    "士",
    "官",
    "客",
    "様",
    "氏",
    "ベンダー",
    "スタッフ",
    "メンバー",
    "ユーザー",
    "エージェント",
    "システム",
    "サーバー",
    "サービス",
    "ツール",
    "アプリ",
    "プログラム",
    "プロセス",
    "ジョブ",
    "バッチ",
    "モデル",
    "ボット",
    "AI",
    "CLI",
    "API",
    "SDK",
    "OS",
    "DB",
];
const NON_AGENT_SUBJECTS: &[&str] = &[
    "前者", "後者", "両者", "三者", "各者", "一部", "大部", "全部",
];

fn is_agent_like(tokens: &[Morpheme], subject: &Argument) -> bool {
    if NON_AGENT_SUBJECTS.contains(&subject.surface.as_str()) {
        return false;
    }
    let proper_noun = tokens
        .iter()
        .any(|token| token.surface == subject.surface && token.pos(1) == "固有名詞");
    proper_noun
        || AGENT_ENDINGS
            .iter()
            .any(|ending| subject.surface.ends_with(ending) || subject.form.ends_with(ending))
}

pub(super) fn subject_added_findings(
    before: &[&Unit],
    after: &[&Unit],
    findings: &mut Vec<DiffFinding>,
) {
    let known = noun_forms(before);
    let mut added = Vec::<String>::new();
    for unit in after {
        for subject in case_arguments(&unit.tokens).0 {
            if !known.contains(&subject.form)
                && is_agent_like(&unit.tokens, &subject)
                && !added.contains(&subject.surface)
            {
                added.push(subject.surface);
            }
        }
    }
    if added.is_empty() {
        return;
    }
    findings.push(finding(
        "subject_added",
        "warn",
        before,
        after,
        format!(
            "変更前にない主語が増えました: {}。担当者や動作主を原文や資料の裏付けなしに補っていないか確認してください。",
            join_items(&added)
        ),
    ));
}

/// 通知・報告・依頼の相手を改稿で補った場合の候補。
/// nwiizo-coding-style: 同じ節の明示的な「に／へ」と最初の動詞に限定する;
/// 省略や遠い係り受けの実例が集まったら反例とともに見直す。
pub(super) fn recipient_added_findings(
    before: &[&Unit],
    after: &[&Unit],
    findings: &mut Vec<DiffFinding>,
) {
    let known = noun_forms(before);
    for unit in after {
        let mut added = Vec::new();
        for (index, token) in unit.tokens.iter().enumerate() {
            if token.pos(1) != "格助詞" || !matches!(token.surface.as_str(), "に" | "へ") {
                continue;
            }
            let Some(actor) = case_head(&unit.tokens, index) else {
                continue;
            };
            if known.contains(&actor.form)
                || !is_agent_like(&unit.tokens, &actor)
                || !recipient_predicate(&unit.tokens[index + 1..])
            {
                continue;
            }
            // 照合は既存の名詞主要部、表示は「運用チーム」のような連続する名詞句。
            let mut start = index;
            while start > 0 && matches!(unit.tokens[start - 1].pos(0), "名詞" | "接頭辞" | "接尾辞")
            {
                start -= 1;
            }
            let surface = unit.tokens[start..index]
                .iter()
                .map(|token| token.surface.as_str())
                .collect::<String>();
            if !added.contains(&surface) {
                added.push(surface);
            }
        }
        if !added.is_empty() {
            findings.push(finding(
                "recipient_added",
                "warn",
                before,
                &[unit],
                format!(
                    "変更前にない通知・報告・依頼の相手が増えました: {}。原文や資料に相手の指定があるか確認してください。",
                    join_items(&added)
                ),
            ));
        }
    }
}

fn recipient_predicate(tokens: &[Morpheme]) -> bool {
    let end = tokens
        .iter()
        .position(|token| {
            matches!(token.pos(0), "補助記号" | "記号")
                || (token.pos(0) == "助詞"
                    && matches!(
                        (token.surface.as_str(), token.pos(1)),
                        ("が", "格助詞") | ("は", "係助詞")
                    ))
        })
        .unwrap_or(tokens.len());
    let clause = &tokens[..end];
    let Some(index) = clause.iter().position(|token| token.pos(0) == "動詞") else {
        return false;
    };
    let verb = &clause[index];
    // Sudachiは「知らせる」を「知る」＋使役の「せる」に分ける。
    let shiraseru = verb.dictionary_form() == "知る"
        && clause
            .get(index + 1)
            .is_some_and(|token| token.dictionary_form() == "せる");
    let predicate_end = index + usize::from(shiraseru);
    if clause.get(predicate_end + 1).is_some_and(|token| {
        token.pos(0) == "助動詞"
            && matches!(
                token.dictionary_form(),
                "れる" | "られる" | "せる" | "させる"
            )
    }) {
        return false;
    }
    shiraseru
        || matches!(
            verb.dictionary_form(),
            "知らせる" | "伝える" | "送る" | "渡す" | "届ける"
        )
        || (verb.dictionary_form() == "する"
            && index.checked_sub(1).is_some_and(|previous| {
                matches!(
                    clause[previous].dictionary_form(),
                    "連絡" | "通知" | "報告" | "依頼" | "相談" | "共有" | "説明"
                )
            }))
}

pub(super) fn nominalization_added_findings(
    before: &[&Unit],
    after: &[&Unit],
    findings: &mut Vec<DiffFinding>,
) {
    static PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
        r"することが可能|が可能|の実施|を実施[しすさ]|を担保|を図[るりっられ]|に資(?:する|し|さ)|の観点から|に関して|を行[ういっわえ]"
    ).expect("valid nominalization regex")
    });
    let count = |units: &[&Unit]| {
        units
            .iter()
            .map(|unit| PATTERN.find_iter(&unit.text).count())
            .sum::<usize>()
    };
    let before_count = count(before);
    let after_count = count(after);
    if after_count > before_count {
        findings.push(finding("nominalization_added", "info", before, after, format!(
            "名詞化した言い回しが{before_count}件から{after_count}件へ増えました。短い動詞を「削減が可能」「実施」などへ替えて読みにくくしていないか確認してください。規程・論文の正式な語と、対象を保つために必要な名詞は残せます。"
        )));
    }
}

/// 人名と地名。「固有名詞・一般」にはDB・フロントエンドのような一般の語も入るので数えない。
fn proper_nouns(tokens: &[Morpheme]) -> impl Iterator<Item = String> + '_ {
    tokens
        .iter()
        .filter(|token| {
            token.pos(0) == "名詞"
                && token.pos(1) == "固有名詞"
                && matches!(token.pos(2), "人名" | "地名")
        })
        .map(|token| token.surface.clone())
}

/// 書き換えた文に、変更前の文書のどこにもない人名・地名が現れたもの。
/// 別の文から移しただけの名前は数えない。coji/natural-japaneseの`newfacts.py`（MIT）と同じ考え方。
pub(super) fn proper_noun_findings(
    before: &[&Unit],
    after: &[&Unit],
    document_values: &DocumentValues,
    findings: &mut Vec<DiffFinding>,
) {
    let mut added = after
        .iter()
        .flat_map(|unit| proper_nouns(&unit.tokens))
        .filter(|name| !document_values.text_before.contains(name.as_str()))
        .collect::<Vec<_>>();
    added.sort();
    added.dedup();
    findings.extend(item_change("proper_noun_added", "warn", before, after, &[], &added, |_, added| {
        format!(
            "変更前の文書にない固有名詞が増えています: {added}。原文や資料にある名前か、書き手が言っていない人・場所・組織を足していないか確認してください。"
        )
    }));
}

pub(super) fn content_word_findings(
    before: &[&Unit],
    after: &[&Unit],
    findings: &mut Vec<DiffFinding>,
) {
    let before_lemmas = before
        .iter()
        .flat_map(|unit| unit.ability_lemmas.iter().cloned())
        .collect::<HashSet<_>>();
    let after_lemmas = after
        .iter()
        .flat_map(|unit| unit.ability_lemmas.iter().cloned())
        .collect::<HashSet<_>>();
    let mut lost = before_lemmas
        .difference(&after_lemmas)
        .cloned()
        .collect::<Vec<_>>();
    let mut added = after_lemmas
        .difference(&before_lemmas)
        .cloned()
        .collect::<Vec<_>>();
    lost.sort();
    added.sort();
    findings.extend(item_change("content_words_changed", "info", before, after, &lost, &added, |lost, added| {
        format!(
            "書き換えで消えた語: {lost} / 増えた語: {added}。消えた語が条件や対象を担っていないか、増えた語に原文や資料の裏付けがあるか確認してください。"
        )
    }));
}

static ACTOR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[一-龥々ァ-ヴー]*(?:担当者|責任者|管理者|利用者|担当|チーム)")
        .expect("valid actor regex")
});

/// 「担当者」が「運用担当者」になったように、人や担当の語が狭まった箇所。
pub(super) fn actor_narrowed_findings(
    before: &[&Unit],
    after: &[&Unit],
    findings: &mut Vec<DiffFinding>,
) {
    let words = |units: &[&Unit]| {
        units
            .iter()
            .flat_map(|unit| {
                ACTOR
                    .find_iter(&unit.text)
                    .map(|found| found.as_str().to_owned())
            })
            .collect::<BTreeSet<_>>()
    };
    let (before_words, after_words) = (words(before), words(after));
    fn containing<'a>(units: &[&'a Unit], word: &str) -> Vec<&'a Unit> {
        units
            .iter()
            .find(|unit| unit.text.contains(word))
            .copied()
            .into_iter()
            .collect()
    }
    for old in before_words.difference(&after_words) {
        let Some(new) = after_words
            .difference(&before_words)
            .find(|new| new.ends_with(old.as_str()) && new.len() > old.len())
        else {
            continue;
        };
        findings.push(finding(
            "actor_narrowed",
            "warn",
            &containing(before, old),
            &containing(after, new),
            format!(
                "人や担当を指す語が「{old}」から「{new}」に狭まりました。原文や資料で担当の範囲が決まっているか確認してください。推測で特定の担当に絞らないでください。"
            ),
        ));
    }
}
