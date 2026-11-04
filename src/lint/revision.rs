//! 推敲の入口。意味による言い換えはSkillで判断し、表記だけに置換候補を付ける。

use std::sync::LazyLock;

use regex::Regex;

use super::morph::TokenizedSentence;
use super::morph::affirmative_predicate;
use super::relations::{active_predicate, first_predicate};
use super::revision_lexicon::{RELATIONS, RENYO_FORMS, RelationRule};
use super::{Finding, Suggestion};
use crate::morphology::Morpheme;

struct WordingRule {
    pattern: Regex,
    category: &'static str,
    detail: &'static str,
}

static RULES: LazyLock<Vec<WordingRule>> = LazyLock::new(|| {
    [
        (r"価格帯に収斂", "explanatory_metaphor", "何がどう変わるかと、時間の変化などの含みを確認してください。原文にない効果は作らないでください。"),
        (r"(?:という|との)見立て(?:です|だ|である)", "revision_expression", "誰の判断か、判断の対象と根拠が追えるか確認してください。主語を推測で足したり、見立てを確定した事実へ変えたりしないでください。"),
        (r"が招く(?:混乱|問題|危険|リスク)", "revision_expression", "何がどの条件で起きるかを前後と資料から確認してください。対象が説明済みなら残し、原因・影響・被害を推測で具体化しないでください。"),
        (r"(?:リポジトリ|プロジェクト|ワークスペース|組織|ファイル|パッケージ|モジュール|部署|チーム|ユーザー|アカウント)レベルの", "revision_expression", "設定や処理の適用範囲が読めるか確認してください。資料にある範囲で説明し、配置場所や継承規則を推測で足さないでください。"),
        (r"という挙動を(?:排除|防止|阻止)|欠かせない存在(?:です|だ|とな)", "revision_expression", "名詞のまとめ方だけで動作や理由を済ませていないか確認してください。何をする・しないのかを原文の確度と範囲で書いてください。"),
        // 「安全側に倒す」は方針の慣用句。相談・問い合わせの相手にすると、人か方針かが定まらない。
        (r"(?:安全|慎重|保守的?な?)側に(?:相談|問い合わせ|問い合せ|聞[かきくけこい]|尋ね|頼[まみむめも])", "compressed_relation", "判断の向きを表す「〜側」を相談の相手にしています。相談する人・窓口と、迷ったときに選ぶ方針のどちらを言いたいのか確認してください。原文や資料にない相談先を補わないでください。"),
        // 責める対象は人。出来事を責める形では、誰を責めないのかが省かれる。
        (r"(?:障害|事故|インシデント|不具合|バグ|エラー)(?:は|を)責め", "compressed_relation", "出来事を「責める」対象にしており、責めない相手（担当者・個人など）が省かれています。誰を責めず、何から学ぶのかが前後で分かるか確認してください。原文にない方針を補わないでください。"),
        (r"(?:書類不備発見時|設定変更後|障害発生時|申請承認後)", "revision_expression", "助詞を省いた名詞の連続です。「設定を変更したあと」など、動作と時の関係を保って開けるか確認してください。項目名・定義語・正式な語は残してください。"),
    ].into_iter().map(|(pattern, category, detail)| WordingRule {
        pattern: Regex::new(pattern).expect("valid revision regex"), category, detail,
    }).collect()
});

pub(super) fn findings(
    sentences: &[TokenizedSentence],
    raw_lines: &[&str],
    technical: bool,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut quote_depth = 0;
    for sentence in sentences {
        let plain = sentence
            .tokens
            .iter()
            .filter(|token| !token.surface.chars().all(|ch| matches!(ch, '*' | '_')))
            .collect::<Vec<_>>();
        if quote_depth == 0
            && crate::markup::quotation_ranges(&sentence.text).is_empty()
            && isolated_unit(sentence, sentences, raw_lines)
            && let Some(end) = elided_condition(&plain)
        {
            findings.push(sentence.info_finding(raw_lines, plain[0].byte_start..plain[end].byte_end,
                "compressed_relation", "条件の後に判断や移動の動詞があり、対象や戻る先が省かれています。見出しの本文や前後で分かるか確認してください。原文や資料にない基準・対象を推測で足さないでください。"));
        }
        let unquoted =
            quote_depth == 0 && crate::markup::quotation_ranges(&sentence.text).is_empty();
        if unquoted && let Some(start) = means_only(&plain, &sentence.text) {
            let end = plain
                .iter()
                .rposition(|t| t.pos(0) == "動詞")
                .unwrap_or(start);
            findings.push(sentence.info_finding(raw_lines, plain[start].byte_start..plain[end].byte_end,
                "compressed_relation", "手段や担い手だけを示して短く言い切っており、何を・どの基準で・誰に対して行うのかが省かれています。前後や見出しの本文で分かるか確認してください。原文や資料にない手順や基準を推測で足さないでください。"));
        }
        if unquoted && bare_verb_chain(&plain, &sentence.text) {
            let end = plain.iter().rposition(|t| t.pos(0) == "動詞").unwrap_or(0);
            findings.push(sentence.info_finding(raw_lines, plain[0].byte_start..plain[end].byte_end,
                "compressed_relation", "主語も目的語もない動詞だけを読点で並べています。何に対して、誰が行う手順なのかが前後で分かるか確認してください。原文や資料にない対象を推測で足さないでください。"));
        }
        for (index, token) in plain.iter().enumerate() {
            if quote_depth_at(&sentence.text, token.byte_start, quote_depth) != 0 {
                continue;
            }
            for rule in RELATIONS {
                if let Some(end) = relation_predicate(&plain, index, rule)
                    && active_predicate(&plain, end)
                {
                    findings.push(sentence.info_finding(
                        raw_lines,
                        token.byte_start..plain[end].byte_end,
                        rule.category,
                        rule.detail,
                    ));
                }
            }
            if renyo_noun(&plain, index, technical) {
                findings.push(sentence.info_finding(raw_lines, token.byte_start..token.byte_end, "renyo_noun",
                    "連用形を単独の名詞として使っています。対象と動作を保ち、動詞や自然な名詞で書けるか確認してください。定着語や正式名称は残してください。"));
            }
            if technical && let Some(end) = operation_condition(&plain, index) {
                findings.push(sentence.info_finding(raw_lines, token.byte_start..plain[end].byte_end, "operation_condition",
                    "操作の名詞と必要条件の主述関係を確認してください。「ログを削除するには、権限を変える必要がある」のように、操作と条件を保って開けます。担当者・許可・義務・実施済みの事実を推測で足さないでください。"));
            }
        }
        for (index, token) in sentence.tokens.iter().enumerate() {
            if quote_depth_at(&sentence.text, token.byte_start, quote_depth) != 0 {
                continue;
            }
            let Some(replacement) = kana_replacement(&sentence.tokens, index) else {
                continue;
            };
            let mut finding = sentence.info_finding(raw_lines, token.byte_start..token.byte_end,
                "auxiliary_kana", format!("補助動詞・形式名詞の「{}」は「{replacement}」と書けます。引用や指定された表記は原文を優先してください。", token.surface));
            if let Some(span) = finding.span
                && raw_lines
                    .get(span.start_line - 1)
                    .and_then(|line| line.get(span.start_byte..span.end_byte))
                    == Some(&token.surface)
            {
                finding.suggestion = Some(Suggestion {
                    span,
                    preimage: token.surface.clone(),
                    replacement,
                });
            }
            findings.push(finding);
        }
        for rule in RULES.iter() {
            for found in rule.pattern.find_iter(&sentence.text) {
                if quote_depth_at(&sentence.text, found.start(), quote_depth) == 0 {
                    findings.push(sentence.info_finding(
                        raw_lines,
                        found.range(),
                        rule.category,
                        rule.detail,
                    ));
                }
            }
        }
        quote_depth = quote_depth_at(&sentence.text, sentence.text.len(), quote_depth);
    }
    findings
}

fn isolated_unit(
    sentence: &TokenizedSentence,
    sentences: &[TokenizedSentence],
    raw: &[&str],
) -> bool {
    if sentences
        .iter()
        .filter(|unit| unit.line == sentence.line)
        .count()
        != 1
    {
        return false;
    }
    let line = raw[sentence.line - 1];
    crate::text::is_heading(line)
        || crate::text::is_list_item(line)
        || sentence.line == 1
        || raw[sentence.line - 2].trim().is_empty()
}

fn elided_condition(tokens: &[&Morpheme]) -> Option<usize> {
    // 「担当に迷ったら」のように、迷う対象を一つだけ前に置ける。
    let lead = if tokens.len() > 2 && tokens[0].pos(0) == "名詞" && tokens[1].surface == "に" {
        2
    } else {
        0
    };
    let first = tokens.get(lead)?;
    if first.pos(0) != "動詞" || !matches!(first.normalized(), "迷う" | "悩む" | "困る") {
        return None;
    }
    let condition = tokens.get(lead + 1)?;
    if !(condition.pos(0) == "助動詞" && condition.pos(5).starts_with("仮定形")
        || first.pos(5).starts_with("仮定形") && condition.surface == "ば")
    {
        return None;
    }
    let mut index = lead + 2;
    if tokens.get(index).is_some_and(|t| t.pos(1) == "読点") {
        index += 1;
    }
    // 「ドキュメントに戻して」「最初の90日で考える」のように、名詞句＋に／でを一つ許す。
    // 動詞を含む句（やめる前に）や目的語（人を守る）は対象外。
    let phrase_start = index;
    while tokens.get(index).is_some_and(|t| {
        matches!(t.pos(0), "名詞" | "接尾辞" | "接頭辞")
            || (t.pos(0) == "助詞" && t.surface == "の")
    }) {
        index += 1;
    }
    let phrase = index > phrase_start;
    if phrase {
        if !tokens
            .get(index)
            .is_some_and(|t| t.pos(1) == "格助詞" && matches!(t.surface.as_str(), "に" | "で"))
        {
            return None;
        }
        index += 1;
    }
    while tokens.get(index).is_some_and(|t| t.pos(0) == "副詞") {
        index += 1;
    }
    let verb = tokens.get(index)?;
    (verb.pos(0) == "動詞"
        // 行き先を書いた「最初の手順に立ち返る」は省略ではない。名詞句があるときは、
        // 目的語のない他動詞（戻して）や、基準が日数だけの「考える」等に限る。
        && if phrase {
            matches!(verb.normalized(), "戻す" | "戻る" | "考える")
        } else {
            matches!(
                verb.normalized(),
                "立ち返る" | "戻す" | "返す" | "分ける" | "合わせる" | "決める"
            )
        }
        && affirmative_predicate(tokens, index))
    .then_some(index)
}

/// 「仕組みで守る」「データで決める」のように、手段だけを示して短く言い切る文。
fn means_only(tokens: &[&Morpheme], text: &str) -> Option<usize> {
    if text.trim_end_matches('。').chars().count() > 20 {
        return None;
    }
    let last = tokens
        .iter()
        .rposition(|t| !matches!(t.pos(0), "補助記号" | "空白"))?;
    let verb = tokens[last];
    // 句点のない行末の動詞は、終止形と連体形のどちらにも解析される。
    let plain_form = verb.pos(5).starts_with("終止形") || verb.pos(5).starts_with("連体形");
    if verb.pos(0) != "動詞" || !plain_form || last < 2 {
        return None;
    }
    // 「棚卸しして返す」「直す者が、持つ」のように、受け渡す物を書かずに言い切る短文。
    if text.trim_end_matches('。').chars().count() <= 16
        && matches!(verb.normalized(), "返す" | "持つ")
        // 「鍵は私が持つ」のように主題が受け渡す物を示す文は除く。
        && !tokens.iter().any(|t| {
            t.surface == "を" && t.pos(1) == "格助詞" || t.surface == "は" && t.pos(1) == "係助詞"
        })
    {
        return Some(0);
    }
    let mut index = last - 1;
    while index > 0 && tokens[index].pos(0) == "副詞" {
        index -= 1;
    }
    let particle = tokens[index];
    let means = tokens[index.checked_sub(1)?];
    (particle.surface == "で"
        && particle.pos(1) == "格助詞"
        && means.pos(0) == "名詞"
        && matches!(
            means.normalized(),
            "仕組み" | "データ" | "現実" | "ドキュメント"
        ))
    .then_some(index - 1)
}

/// 「触らず、止めて、待つ」のように、主語・目的語のない動詞だけを読点で並べた文。
fn bare_verb_chain(tokens: &[&Morpheme], text: &str) -> bool {
    if text.trim_end_matches('。').chars().count() > 30
        || tokens.iter().any(|t| {
            t.pos(1) == "係助詞" && t.surface == "は"
                || t.pos(1) == "格助詞" && matches!(t.surface.as_str(), "が" | "を" | "に" | "で")
        })
    {
        return false;
    }
    let content = tokens
        .iter()
        .filter(|t| t.pos(1) != "句点" && t.pos(0) != "空白")
        .collect::<Vec<_>>();
    let parts = content.split(|t| t.pos(1) == "読点").collect::<Vec<_>>();
    parts.len() >= 2
        && parts.iter().enumerate().all(|(index, part)| {
            let has_verb = part.iter().any(|t| t.pos(0) == "動詞");
            // 名詞は「改善する」のサ変語幹だけを許す。名詞述語（〜であり）や「〜となり」は手順の列挙ではない。
            let verbs_only = part.iter().enumerate().all(|(i, t)| {
                t.pos(0) != "名詞"
                    || part
                        .get(i + 1)
                        .is_some_and(|next| next.dictionary_form() == "する")
            }) && !part
                .iter()
                .any(|t| matches!(t.normalized(), "有る" | "居る" | "成る" | "言う"));
            let ends = part.last().is_some_and(|t| {
                if index + 1 == parts.len() {
                    t.pos(0) == "動詞"
                        && (t.pos(5).starts_with("終止形") || t.pos(5).starts_with("連体形"))
                } else {
                    t.pos(0) == "動詞" && t.pos(5).starts_with("連用形")
                        || t.pos(1) == "接続助詞" && matches!(t.surface.as_str(), "て" | "で")
                        || t.pos(0) == "助動詞" && t.surface == "ず"
                }
            });
            has_verb && verbs_only && ends
        })
}

// nwiizo-coding-style: 係り受け解析ではなく、直近の述語へ続く限定した型だけを扱う;
// 新しい型を広げるときは、別主語・連体修飾・助詞の反例も追加する。
fn relation_predicate(tokens: &[&Morpheme], index: usize, rule: &RelationRule) -> Option<usize> {
    let noun = tokens[index];
    // 「ハンガーだけが」のように、限定の「だけ」を一つ挟める。
    let marker_index =
        index + 1 + usize::from(tokens.get(index + 1).is_some_and(|t| t.surface == "だけ"));
    let marker = tokens.get(marker_index)?;
    if noun.pos(0) != "名詞"
        || !rule.nouns.contains(&noun.normalized())
        || marker.pos(0) != "助詞"
        || !rule.particles.contains(&marker.surface.as_str())
    {
        return None;
    }
    let predicate = first_predicate(tokens, marker_index + 1, |object| {
        rule.objects.contains(&object.normalized())
    })?;
    rule.predicates
        .contains(&tokens[predicate.head].normalized())
        .then_some(predicate.end)
}

fn renyo_noun(tokens: &[&Morpheme], index: usize, technical: bool) -> bool {
    let token = tokens[index];
    if !RENYO_FORMS.contains(&token.surface.as_str())
        || !(token.pos(0) == "名詞"
            || (token.pos(0) == "動詞" && token.pos(5).starts_with("連用形")))
    {
        return false;
    }
    let modified = index >= 2
        && tokens[index - 1].surface == "の"
        && tokens[index - 1].pos(0) == "助詞"
        && tokens[index - 2].pos(0) == "名詞";
    let demonstrative = technical
        && index >= 1
        && tokens[index - 1].pos(0) == "連体詞"
        && matches!(
            tokens[index - 1].surface.as_str(),
            "その" | "この" | "あの" | "どの"
        );
    if !modified && !demonstrative {
        return false;
    }
    tokens.get(index + 1).is_none_or(|next| {
        (next.pos(0) == "助詞" && matches!(next.surface.as_str(), "が" | "を" | "は" | "に" | "と"))
            || (next.pos(0) == "補助記号" && matches!(next.pos(1), "読点" | "句点"))
    })
}

// 操作名を主題にした文のうち、別の対象や動作を必要条件として続ける型。
// 「変更は必要です」など、操作そのものの必要性を述べる文は含めない。
fn operation_condition(tokens: &[&Morpheme], index: usize) -> Option<usize> {
    let head = tokens[index];
    if head.pos(0) != "名詞"
        || !super::revision_lexicon::OPERATIONS.contains(&head.normalized())
        || tokens
            .get(index + 1)
            .is_none_or(|t| t.surface != "は" || t.pos(0) != "助詞")
    {
        return None;
    }
    let topic = tokens[..index]
        .iter()
        .rev()
        .take_while(|t| t.pos(0) != "補助記号" && t.pos(0) != "動詞")
        .take(10);
    if !topic
        .into_iter()
        .any(|t| t.surface == "の" && t.pos(0) == "助詞")
    {
        return None;
    }
    let mut required_content = false;
    for (end, token) in tokens.iter().enumerate().skip(index + 2).take(24) {
        if token.normalized() == "必要" {
            return required_content.then_some(end);
        }
        if (token.pos(0) == "補助記号" && token.pos(1) != "読点")
            || (token.pos(0) == "助詞" && token.surface == "は")
        {
            return None;
        }
        required_content |= matches!(token.pos(0), "名詞" | "動詞");
    }
    None
}

fn kana_replacement(tokens: &[Morpheme], index: usize) -> Option<String> {
    let token = &tokens[index];
    let previous = index.checked_sub(1).map(|i| &tokens[i]);
    let after_te = previous
        .is_some_and(|t| t.pos(1) == "接続助詞" && matches!(t.surface.as_str(), "て" | "で"));
    if after_te
        && token.pos(0) == "動詞"
        && matches!(
            token.dictionary_form(),
            "下さる" | "頂く" | "見る" | "置く" | "居る"
        )
    {
        for (kanji, kana) in [
            ("下さ", "くださ"),
            ("頂", "いただ"),
            ("見", "み"),
            ("置", "お"),
            ("居", "い"),
        ] {
            if let Some(tail) = token.surface.strip_prefix(kanji) {
                return Some(format!("{kana}{tail}"));
            }
        }
    }
    if token.pos(0) == "動詞"
        && token.surface.starts_with("出来")
        && previous.is_some_and(|t| {
            t.pos(2).starts_with("サ変")
                || (t.surface == "が" && index >= 2 && tokens[index - 2].surface == "こと")
        })
    {
        return Some(token.surface.replacen("出来", "でき", 1));
    }
    if token.pos(0) == "名詞" && previous.is_some_and(|t| t.pos(5).starts_with("連体形")) {
        return match token.surface.as_str() {
            "事" => Some("こと".to_owned()),
            "為" => Some("ため".to_owned()),
            _ => None,
        };
    }
    None
}

fn quote_depth_at(text: &str, end: usize, mut depth: usize) -> usize {
    for ch in text[..end].chars() {
        match ch {
            '「' | '『' | '“' => depth += 1,
            '」' | '』' | '”' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    depth
}
