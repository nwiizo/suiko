//! 形態素と局所文脈に基づく確認候補。構文の正誤を断定しない。

use std::collections::BTreeMap;

use super::{Finding, morph::TokenizedSentence};

pub(super) fn findings(
    sentences: &[TokenizedSentence],
    raw_lines: &[&str],
    experimental: bool,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut paragraph = Vec::new();
    let mut previous_line = 0;
    for sentence in sentences {
        if previous_line > 0
            && sentence.line > previous_line
            && raw_lines[previous_line..sentence.line.saturating_sub(1)]
                .iter()
                .any(|line| {
                    line.trim().is_empty()
                        || crate::text::is_heading(line)
                        || crate::text::is_list_item(line)
                })
        {
            if experimental {
                register_findings(&paragraph, raw_lines, &mut findings);
            }
            paragraph.clear();
        }
        previous_line = sentence.line;
        if sentence.text.contains(['「', '」', '『', '』', '“', '”'])
            || !crate::markup::quotation_ranges(&sentence.text).is_empty()
        {
            continue;
        }
        paragraph.push(sentence);
        for pair in sentence.tokens.windows(2) {
            let (verb, suffix) = (&pair[0], &pair[1]);
            if verb.pos(0) != "動詞"
                || !verb.pos(5).starts_with("連用形")
                || suffix.pos(0) != "接尾辞"
                || suffix.dictionary_form() != "ずらい"
                || verb.byte_end != suffix.byte_start
            {
                continue;
            }
            let mut finding = sentence.info_finding(raw_lines, suffix.byte_start..suffix.byte_end,
                "word_usage", "動詞の連用形に続き、動作の難しさを表す接尾辞です。「ず」を「づ」にして、元の活用を保ってください。固有名や引用の表記は残します。");
            if let Some(span) = finding.span {
                finding.suggestion = Some(super::Suggestion {
                    span,
                    preimage: suffix.surface.clone(),
                    replacement: suffix.surface.replacen('ず', "づ", 1),
                });
            }
            findings.push(finding);
        }
        for (index, verb) in sentence.tokens.iter().enumerate() {
            if verb.dictionary_form() == "上げる"
                && sentence.tokens[..index]
                    .iter()
                    .rev()
                    .take(6)
                    .any(|t| matches!(t.normalized(), "選択肢" | "例" | "候補"))
                && sentence.text[..verb.byte_start].contains("として")
            {
                findings.push(sentence.info_finding(raw_lines, verb.byte_start..verb.byte_end, "word_usage", "例や選択肢を列挙する意味なら「挙げる」を使います。値を上昇させる意味との区別を確認してください。"));
            }
        }
        if !experimental {
            continue;
        }
        topic_findings(sentence, raw_lines, &mut findings);
        repetition_findings(sentence, raw_lines, &mut findings);
    }
    if experimental {
        register_findings(&paragraph, raw_lines, &mut findings);
    }
    findings
}

fn topic_findings(sentence: &TokenizedSentence, raw_lines: &[&str], findings: &mut Vec<Finding>) {
    let tokens = &sentence.tokens;
    let Some(topic) = tokens
        .iter()
        .position(|t| t.surface == "は" && t.pos(0) == "助詞")
    else {
        return;
    };
    let Some(subject) = tokens[..topic].iter().rev().find(|t| t.pos(0) == "名詞") else {
        return;
    };
    let text = &sentence.text[tokens[topic].byte_end..];
    let request = matches!(subject.normalized(), "ブログ" | "記事")
        && text.contains("ために")
        && text.contains("ください");
    let judgment = matches!(subject.normalized(), "フィールド" | "設定" | "ファイル")
        && tokens[topic + 1..]
            .iter()
            .any(|t| matches!(t.normalized(), "評価" | "判断"))
        && (text.contains("行い") || text.contains("します"))
        && !text.contains("自動");
    if request || judgment {
        findings.push(sentence.info_finding(raw_lines, subject.byte_start..sentence.text.len(), "topic_predicate_mismatch", "主題の物と、利用・評価する人や処理の関係を確認してください。対象を主題にした自然な文なら残し、動作主が曖昧なら原文の根拠から書き分けてください。"));
    }
}

fn repetition_findings(
    sentence: &TokenizedSentence,
    raw_lines: &[&str],
    findings: &mut Vec<Finding>,
) {
    let mut seen = BTreeMap::new();
    for (index, verb) in sentence.tokens.iter().enumerate() {
        if !matches!(verb.pos(0), "動詞" | "形容詞")
            || matches!(verb.dictionary_form(), "する" | "ある" | "いる")
        {
            continue;
        }
        let Some(noun) = sentence.tokens[..index]
            .iter()
            .rev()
            .take(4)
            .find(|t| t.pos(0) == "名詞")
        else {
            continue;
        };
        let tail = sentence.tokens[index + 1..]
            .iter()
            .take_while(|t| matches!(t.pos(0), "助動詞" | "形容詞" | "接尾辞"))
            .map(|t| t.dictionary_form())
            .collect::<Vec<_>>()
            .join("/");
        let key = (noun.normalized(), verb.dictionary_form(), tail);
        if let Some(start) = seen.insert(key, noun.byte_start) {
            findings.push(sentence.info_finding(raw_lines, start..verb.byte_end, "repeated_predicate_phrase", "同じ内容語と述語が一文の中で繰り返されています。別の条件や対象を説明している場合、意図的に強調している場合は残してください。"));
        }
    }
}

fn register_findings(
    sentences: &[&TokenizedSentence],
    raw_lines: &[&str],
    findings: &mut Vec<Finding>,
) {
    if sentences.len() < 3 {
        return;
    }
    let modes = sentences
        .iter()
        .filter_map(|sentence| {
            let tail = sentence
                .tokens
                .iter()
                .rev()
                .filter(|t| !matches!(t.pos(0), "記号" | "補助記号" | "空白"))
                .take(4)
                .collect::<Vec<_>>();
            if tail.is_empty() || !matches!(tail[0].pos(0), "動詞" | "助動詞" | "形容詞") {
                return None;
            }
            let polite = tail
                .iter()
                .any(|t| matches!(t.dictionary_form(), "です" | "ます"));
            Some((*sentence, polite))
        })
        .collect::<Vec<_>>();
    let polite = modes.iter().filter(|(_, mode)| *mode).count();
    let plain = modes.len() - polite;
    if polite.min(plain) != 1 || polite.max(plain) < 2 {
        return;
    }
    let majority = polite > plain;
    for (sentence, mode) in modes {
        if mode == majority {
            continue;
        }
        findings.push(sentence.info_finding(raw_lines, 0..sentence.text.len(), "paragraph_register_mixture", "同じ段落の他の文と敬体・常体が異なります。引用や意図した語り分けなら残し、技術説明の語尾が意図せず変わった場合に揃えてください。"));
    }
}
