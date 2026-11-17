//! 形態素の並びで判定する言い回し: 翻訳調、あいまいな範囲、述語のない断片、判断の方向、
//! 冗長な「行う」、無生物主語。置換候補を出せる形は`Suggestion`を付ける。

use crate::morphology::Morpheme;

use super::morph::{TokenizedSentence, significant_tokens, token_positions};
use super::{Finding, Suggestion};

const TRANSITIVE_SMELL_VERBS: &[&str] = &[
    "もたらす",
    "示す",
    "意味する",
    "証明する",
    "生み出す",
    "反映する",
    "示唆する",
    "物語る",
    "浮き彫りにする",
    "後押しする",
];

pub(super) fn translationese_morph_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (sentence, index, token) in token_positions(tokenized) {
        if let Some((byte_start, byte_end, pattern)) =
            abstract_motsu_candidate(&sentence.tokens, index)
        {
            findings.push(sentence.info_finding(
                raw_lines,
                byte_start..byte_end,
                "translationese_morph",
                format!(
                    "「{pattern}」という形で抽象的な内容を表しています。何を決める・考えるのかが分かるか確認してください。「意味を持つ」だけで不自然とは限りません。"
                ),
            ));
        }

        let Some(particle) = sentence.tokens.get(index + 1) else {
            continue;
        };
        let Some(verb) = sentence.tokens.get(index + 2) else {
            continue;
        };
        // 2026-08-18の技術書翻訳21件の正解ラベルに基づき対象を絞る:
        // 「は」型(ことはできない)は否定の対比として自然、使役型
        // (させることができる)は縮約すると受身と紛れるため対象外。
        let causative = index > 0
            && matches!(
                sentence.tokens[index - 1].dictionary_form(),
                "せる" | "させる"
            );
        if token.surface == "こと"
            && token.pos(0) == "名詞"
            && particle.pos(0) == "助詞"
            && particle.surface == "が"
            && !causative
            && verb.pos(0) == "動詞"
            && verb.surface.starts_with("でき")
        {
            let start = sentence.tokens[index.saturating_sub(4)].byte_start;
            let mut finding = sentence.info_finding(
                raw_lines,
                start..verb.byte_end,
                "translationese_morph",
                "「ことができる」を短くできる候補です。可能・能力の意味と文体を保ち、「できます」を「します」や「して構いません」へ変えないでください。",
            );
            finding.suggestion = suru_koto_ga_suggestion(sentence, raw_lines, index, particle);
            if finding.suggestion.is_none() {
                finding.review_suggestion = potential_suggestion(sentence, raw_lines, index, verb);
            }
            let past = sentence.tokens[index + 3..]
                .iter()
                .any(|t| t.pos(0) == "助動詞" && t.dictionary_form() == "た");
            let personal = [
                "楽しく",
                "楽しい",
                "充実",
                "嬉し",
                "うれし",
                "幸い",
                "私",
                "読書",
                "学び",
            ]
            .iter()
            .any(|cue| sentence.text.contains(cue));
            let specification = [
                "旧版",
                "バージョン",
                "仕様",
                "API",
                "エラー",
                "設定",
                "サーバー",
                "権限",
            ]
            .iter()
            .any(|cue| sentence.text.contains(cue));
            if past && personal && !specification {
                finding.category = "evaluative_ability_past".to_owned();
                finding.detail = "過去の体験や実感を表す可能表現です。達成感や機会を得た意味があるなら、そのまま残してください。".to_owned();
                finding.suggestion = None;
                finding.review_suggestion = None;
            }
            findings.push(finding);
        }
    }
    findings
}

fn abstract_motsu_candidate(
    tokens: &[Morpheme],
    index: usize,
) -> Option<(usize, usize, &'static str)> {
    let first = tokens.get(index)?;
    let second = tokens.get(index + 1)?;
    let third = tokens.get(index + 2);

    if second.pos(0) == "助詞" && second.surface == "を" {
        let third = third?;
        if third.pos(0) == "動詞" && third.dictionary_form() == "持つ" {
            if first.pos(0) == "名詞" && first.dictionary_form() == "意味" {
                return Some((first.byte_start, third.byte_end, "意味+を+持つ"));
            }
            if first.pos(0) == "助詞" && first.surface == "か" {
                return Some((first.byte_start, third.byte_end, "疑問節末のか+を+持つ"));
            }
        }
    }

    if first.surface == "持てる" && second.pos(0) == "名詞" && second.dictionary_form() == "未決"
    {
        return Some((first.byte_start, second.byte_end, "持てる+未決"));
    }

    if first.surface == "持て" && second.surface == "る" {
        let third = third?;
        if third.pos(0) == "名詞" && third.dictionary_form() == "未決" {
            return Some((first.byte_start, third.byte_end, "持てる+未決"));
        }
    }

    None
}

pub(super) fn technical_ambiguity_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for sentence in tokenized {
        for (index, token) in sentence.tokens.iter().enumerate() {
            if matches!(token.surface.as_str(), "この" | "その" | "あの")
                && sentence
                    .tokens
                    .get(index + 1)
                    .is_some_and(|next| next.pos(0) == "名詞" && next.dictionary_form() == "こと")
            {
                let predicate_count = sentence.tokens[..index]
                    .iter()
                    .filter(|candidate| candidate.pos(0) == "動詞")
                    .count();
                if predicate_count >= 2 {
                    let end = sentence.tokens[index + 1].byte_end;
                    findings.push(sentence.info_finding(
                        raw_lines,
                        token.byte_start..end,
                        "demonstrative_reference",
                        format!(
                            "品詞列マッチ: 同じ文の前方に動詞が{predicate_count}個あり、その後に「{}こと」がある。指示先を読み直す候補",
                            token.surface
                        ),
                    ));
                }
            }

            if token.surface != "それぞれ" {
                continue;
            }
            let preceding_separator = sentence.tokens[..index]
                .iter()
                .rposition(is_enumeration_separator);
            if preceding_separator.is_none()
                || sentence.tokens[index + 1..]
                    .iter()
                    .any(is_enumeration_separator)
            {
                continue;
            }
            let separator = preceding_separator.expect("separator exists");
            let start = sentence.tokens[separator.saturating_sub(1)].byte_start;
            let end = sentence.tokens[index + 1..]
                .iter()
                .find(|candidate| candidate.pos(0) == "動詞")
                .map_or(token.byte_end, |candidate| candidate.byte_end);
            findings.push(sentence.info_finding(
                raw_lines,
                start..end,
                "respectively_scope",
                "品詞列マッチ: 列挙の後に「それぞれ」があり、後方に対応する列挙がない。どの要素を一つずつ扱うか読み直す候補",
            ));
        }
    }
    findings
}

fn is_enumeration_separator(token: &Morpheme) -> bool {
    (token.pos(0) == "助詞" && matches!(token.surface.as_str(), "と" | "や"))
        || matches!(token.surface.as_str(), "、" | "，" | ",")
}

/// 「資料を、全員へ。」「作業を、もっと確かに。」のように、短い名詞句＋格助詞の後で読点の間を置き、
/// 格助詞または「〜に／〜で」で述語を書かずに終える文。広告の惹句に近い型を、散文の文単位で確認する。
/// 応答の「ええ、東京へ。」は読点の前が格助詞でないため除く。
pub(super) fn copy_fragment_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for sentence in tokenized {
        // 句点のない惹句は、1行だけで書かれた標語の行に限って見る。
        let standalone = sentence.end_mark.is_none()
            && raw_lines
                .get(sentence.line - 1)
                .is_some_and(|line| line.trim() == sentence.text.trim());
        if !(matches!(sentence.end_mark, Some('。')) || standalone)
            || sentence.text.chars().count() > 30
        {
            continue;
        }
        let tokens = significant_tokens(&sentence.tokens);
        let content = tokens
            .iter()
            .rposition(|token| !matches!(token.pos(0), "補助記号" | "記号" | "空白"))
            .map(|index| &tokens[..=index]);
        let Some(content @ [first, .., last]) = content else {
            continue;
        };
        let Some(comma) = content.iter().position(|token| token.surface == "、") else {
            continue;
        };
        let lead = &content[..comma];
        let ends_with_case = |token: &Morpheme| token.pos(1) == "格助詞";
        // 「誰にでも」「ここからも」の末尾の「も」は外し、その前の語で終わり方を判定する。
        let body = match content {
            [rest @ .., particle] if particle.surface == "も" && particle.pos(1) == "係助詞" => {
                rest
            }
            _ => content,
        };
        let Some((end, before_end)) = body.split_last() else {
            continue;
        };
        let fragment_end = (ends_with_case(end)
            && matches!(
                end.surface.as_str(),
                "へ" | "に" | "を" | "と" | "で" | "から" | "まで"
            ))
            || (end.pos(0) == "助動詞"
                && matches!(end.surface.as_str(), "に" | "で")
                && before_end.last().is_some_and(|token| {
                    matches!(token.pos(0), "名詞" | "代名詞" | "形状詞") || token.pos(1) == "格助詞"
                }));
        if comma == 0
            || lead.iter().map(|token| token.surface.chars().count()).sum::<usize>() > 8
            || !lead.last().is_some_and(|token| ends_with_case(token) || token.surface == "は")
            // 連体修飾の形容詞（「新しい資料を」）は句の一部として許し、動詞節は除く。
            || !lead[..lead.len() - 1].iter().all(|token| {
                matches!(
                    token.pos(0),
                    "名詞" | "代名詞" | "接頭辞" | "接尾辞" | "連体詞" | "形容詞"
                ) || token.surface == "の"
            })
            || !fragment_end
            || content[comma..].iter().any(|token| {
                matches!(token.pos(0), "動詞" | "形容詞")
                    || (token.pos(0) == "助動詞" && !std::ptr::eq(token, end))
            })
            || lead.iter().any(|token| matches!(token.pos(0), "動詞" | "助動詞"))
        {
            continue;
        }
        findings.push(sentence.info_finding(
            raw_lines,
            first.byte_start..last.byte_end,
            "copy_fragment",
            "短い句の後に読点で間を置き、述語を書かずに格助詞などで終える惹句の型です。誰が何をするのか、文の働きに合う述語まで書けるか確認してください。見出しや意図した標語なら残せます。",
        ));
    }
    findings
}

/// 選択の説明は技術文書に限らないため、ジャンルを問わず確認候補にする。
pub(super) fn decision_direction_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (sentence, index, token) in token_positions(tokenized) {
        if token.pos(0) == "動詞"
            && token.dictionary_form() == "倒す"
            && let Some(start) = decision_side_start(&sentence.tokens, index)
        {
            findings.push(sentence.info_finding(
                raw_lines,
                start..token.byte_end,
                "decision_direction_metaphor",
                "採否や処理の選択を「側に倒す」で表しています。何を残す・除外する・許可するのか、条件と動作を直接書けるか確認してください。選択内容が前後で明らかなら残せます。",
            ));
        }
    }
    findings
}

/// 選択する動作＋（否定）＋側に倒す、または方針を表す名詞＋側に倒す。
/// 物理的な方向や場所を指す節は拾わない。
fn decision_side_start(tokens: &[Morpheme], index: usize) -> Option<usize> {
    let side = index.checked_sub(2)?;
    if tokens[side].surface != "側"
        || tokens[side].pos(0) != "名詞"
        || tokens[side + 1].surface != "に"
        || tokens[side + 1].pos(1) != "格助詞"
    {
        return None;
    }
    // 「安全側／保守側」は技術文書の`technical_jargon_metaphor`が扱うため、ここでは重ねない。
    if let Some(policy) = side.checked_sub(1)
        && matches!(tokens[policy].pos(0), "名詞" | "形状詞")
        && matches!(
            tokens[policy].dictionary_form(),
            "共通"
                | "厳格"
                | "寛容"
                | "慎重"
                | "標準"
                | "既定"
                | "互換"
                | "許可"
                | "拒否"
                | "失敗"
        )
    {
        return Some(tokens[policy].byte_start);
    }
    let mut action = side.checked_sub(1)?;
    if tokens[action].pos(0) == "助動詞" && tokens[action].dictionary_form() == "ない" {
        action = action.checked_sub(1)?;
    }
    let verb = &tokens[action];
    if verb.pos(0) != "動詞" {
        return None;
    }
    let start = match verb.dictionary_form() {
        "残す" | "捨てる" | "除く" | "省く" | "見送る" => action,
        "する" => {
            let noun = action.checked_sub(1)?;
            if tokens[noun].pos(0) != "名詞"
                || !matches!(
                    tokens[noun].dictionary_form(),
                    "採用" | "許可" | "保存" | "実行" | "削除" | "追加"
                )
            {
                return None;
            }
            noun
        }
        _ => return None,
    };
    Some(tokens[start].byte_start)
}

/// 機械的に安全な唯一の縮約: 「〜することができる」→「〜できる」。
/// 直前が動詞「する」で助詞が「が」の場合だけ、「することが」の削除候補を出す。
/// raw行のpreimageが一致しないときは出さない。
fn suru_koto_ga_suggestion(
    sentence: &TokenizedSentence,
    raw_lines: &[&str],
    koto_index: usize,
    particle: &Morpheme,
) -> Option<Suggestion> {
    if koto_index == 0 || particle.surface != "が" {
        return None;
    }
    let suru = sentence.tokens.get(koto_index - 1)?;
    if suru.pos(0) != "動詞" || suru.dictionary_form() != "する" {
        return None;
    }
    let koto = &sentence.tokens[koto_index];
    let expected = format!("{}{}{}", suru.surface, koto.surface, particle.surface);
    let line_start = sentence.line_byte_start + suru.byte_start;
    let line_end = sentence.line_byte_start + particle.byte_end;
    let matches_raw = raw_lines
        .get(sentence.line - 1)
        .and_then(|raw_line| raw_line.get(line_start..line_end))
        .is_some_and(|slice| slice == expected);
    if !matches_raw {
        return None;
    }
    Some(Suggestion {
        span: sentence.span(raw_lines, suru.byte_start, particle.byte_end)?,
        preimage: expected,
        replacement: String::new(),
    })
}

fn potential_suggestion(
    sentence: &TokenizedSentence,
    raw_lines: &[&str],
    koto_index: usize,
    ability: &Morpheme,
) -> Option<Suggestion> {
    let verb = sentence.tokens.get(koto_index.checked_sub(1)?)?;
    let potential = verb.potential_stem()?;
    let span = sentence.span(raw_lines, verb.byte_start, ability.byte_end)?;
    let preimage = raw_lines
        .get(sentence.line - 1)?
        .get(span.start_byte..span.end_byte)?
        .to_owned();
    Some(Suggestion {
        span,
        preimage,
        replacement: format!("{potential}{}", ability.surface.strip_prefix("でき")?),
    })
}

/// サ変名詞+を+行う型の冗長（「検証を行う」→「検証する」）。
/// 名詞とを、をと行うが隣接する場合だけ対象にし、受身（行われる）と
/// 名詞がサ変可能でない場合（「祭りを行う」）は対象外。textlintの
/// ai-tech-writing-guidelineが指摘する冗長クラスのうち、形態素で
/// 決定的に判定できる部分だけを扱う。
pub(super) fn redundant_light_verb_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (sentence, index, noun) in token_positions(tokenized) {
        let Some(particle) = sentence.tokens.get(index + 1) else {
            continue;
        };
        let Some(verb) = sentence.tokens.get(index + 2) else {
            continue;
        };
        let verbal_noun =
            noun.pos(0) == "名詞" && (noun.pos(2) == "サ変可能" || noun.pos(2) == "サ変形状詞可能");
        if !verbal_noun
            || particle.pos(0) != "助詞"
            || particle.surface != "を"
            || verb.pos(0) != "動詞"
            || !matches!(verb.dictionary_form(), "行う" | "行なう")
        {
            continue;
        }
        // 受身・使役（行われる、行わせる）は言い換えの意味が変わるため対象外
        let passive_or_causative = sentence.tokens.get(index + 3).is_some_and(|next| {
            matches!(
                next.dictionary_form(),
                "れる" | "られる" | "せる" | "させる"
            )
        });
        if passive_or_causative {
            continue;
        }
        let mut finding = sentence.info_finding(
            raw_lines,
            noun.byte_start..verb.byte_end,
            "redundant_light_verb",
            format!(
                "サ変名詞+を+行う型の冗長候補: 「{}を{}」は「{}する」へ畳める。名詞の動作性を活かす方が簡潔（意図的な文体なら維持する）",
                noun.surface, verb.surface, noun.surface
            ),
        );
        finding.suggestion = light_verb_suggestion(sentence, raw_lines, particle, verb);
        findings.push(finding);
    }
    findings
}

/// 「を+行う」の活用形ごとに機械的に安全な置換だけを提案する。
/// 行う→する、行い→し、行っ→し。それ以外の活用（行わ、行え等）は
/// 提案しない。preimageがraw行と一致しない場合も提案しない。
fn light_verb_suggestion(
    sentence: &TokenizedSentence,
    raw_lines: &[&str],
    particle: &Morpheme,
    verb: &Morpheme,
) -> Option<Suggestion> {
    let replacement = match verb.surface.as_str() {
        "行う" | "行なう" => "する",
        "行い" | "行ない" => "し",
        "行っ" | "行なっ" => "し",
        _ => return None,
    };
    let expected = format!("{}{}", particle.surface, verb.surface);
    let line_start = sentence.line_byte_start + particle.byte_start;
    let line_end = sentence.line_byte_start + verb.byte_end;
    let matches_raw = raw_lines
        .get(sentence.line - 1)
        .and_then(|raw_line| raw_line.get(line_start..line_end))
        .is_some_and(|slice| slice == expected);
    if !matches_raw {
        return None;
    }
    Some(Suggestion {
        span: sentence.span(raw_lines, particle.byte_start, verb.byte_end)?,
        preimage: expected,
        replacement: replacement.to_owned(),
    })
}

pub(super) fn inanimate_morph_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for sentence in tokenized {
        let mut skip_until = None;
        for index in 0..sentence.tokens.len() {
            if skip_until.is_some_and(|skip| index <= skip) {
                continue;
            }
            let token = &sentence.tokens[index];
            let mut subject_end = index;
            let mut abstract_subject =
                matches!(token.surface.as_str(), "これ" | "それ" | "あれ" | "それら")
                    || (token.pos(0) == "名詞"
                        && matches!(token.surface.as_str(), "こと" | "事実"));
            if !abstract_subject && let Some(next) = sentence.tokens.get(index + 1) {
                let phrase = format!("{}{}", token.surface, next.surface);
                if matches!(phrase.as_str(), "この事実" | "そのこと") {
                    abstract_subject = true;
                    subject_end = index + 1;
                }
            }
            if !abstract_subject {
                continue;
            }
            skip_until = Some(subject_end);
            let Some(particle) = sentence.tokens.get(subject_end + 1) else {
                continue;
            };
            if particle.pos(0) != "助詞" || !matches!(particle.surface.as_str(), "が" | "は") {
                continue;
            }
            let verb = sentence.tokens[subject_end + 2..].iter().find(|candidate| {
                candidate.pos(0) == "動詞"
                    && TRANSITIVE_SMELL_VERBS.contains(&candidate.dictionary_form())
            });
            let Some(verb) = verb else {
                continue;
            };
            let byte_start = sentence.tokens[index.saturating_sub(3)].byte_start;
            let subject = sentence.tokens[index..=subject_end]
                .iter()
                .map(|token| token.surface.as_str())
                .collect::<String>();
            findings.push(sentence.info_finding(
                raw_lines,
                byte_start..verb.byte_end,
                "inanimate_subject_morph",
                format!(
                    "「{subject}」が何を指し、「{}」とどうつながるか確認してください。データや事実が結果を示す客観的な説明は、そのまま残せます。人の主語を推測で足さないでください。",
                    verb.dictionary_form()
                ),
            ));
        }
    }
    findings
}
