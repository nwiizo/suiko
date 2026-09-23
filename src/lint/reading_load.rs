//! 読解負荷レーン。自然度スコアとは分離した推敲用の指さしを返す。

use std::collections::BTreeMap;

use regex::Regex;

use crate::Error;
use crate::morphology::Morphology;
use crate::text::{mask_markdown_structure, sentences_with_raw};

use super::morph::{
    buried_list, buried_question_list, long_attributive_span, punctuation_between, tokenize,
};
use super::{Finding, ReadingLoadReport, ReadingLoadStats, ReadingLoadThresholds};

// Issue #34 で著者が読みづらいと判断した文は句点を除いて49〜70字（2節・55字を含む）。
// 30字まで下げても実文書99件で10件しか出ず、45字で3文をすべて残す(eval/calibration.md)。
// 節の下限は「はいか、」のような選択肢を外し、「〜へ戻るか」(11字)を残す。
const QUESTION_LIST_SENTENCE_MIN: usize = 45;
const QUESTION_CLAUSE_MIN: usize = 6;

fn reading_length(text: &str) -> usize {
    let whitespace = Regex::new(r"\s{2,}").expect("valid whitespace regex");
    whitespace.replace_all(text, " ").trim().chars().count()
}

fn first_negation_modifies_noun(
    tokens: &[crate::morphology::Morpheme],
    first: usize,
    second: usize,
) -> bool {
    let Some(_) = tokens.get(first + 1).filter(|token| {
        matches!(token.pos(0), "名詞" | "代名詞")
            && !matches!(token.surface.as_str(), "こと" | "わけ" | "はず")
    }) else {
        return false;
    };
    let Some(particle) = tokens.get(first + 2) else {
        return false;
    };
    // 「必要のないデータはない」のような存在の否定は候補に残す。
    // 別の修飾句にある「ある」まで、この例外へ含めない。
    if matches!(particle.surface.as_str(), "は" | "が" | "も")
        && tokens[first + 1..second]
            .iter()
            .rev()
            .find(|token| token.pos(0) == "動詞")
            .is_some_and(|token| token.dictionary_form() == "ある")
    {
        return false;
    }
    first + 2 < second
        && particle.pos(0) == "助詞"
        && matches!(
            particle.surface.as_str(),
            "は" | "が" | "を" | "も" | "や" | "に"
        )
}

pub fn analyze_reading_load(
    raw: &str,
    morphology: &Morphology,
    genre: Option<&str>,
) -> Result<ReadingLoadReport, Error> {
    analyze_reading_load_with_thresholds(raw, morphology, genre, ReadingLoadThresholds::default())
}

pub fn analyze_reading_load_with_thresholds(
    raw: &str,
    morphology: &Morphology,
    genre: Option<&str>,
    thresholds: ReadingLoadThresholds,
) -> Result<ReadingLoadReport, Error> {
    let masked = mask_markdown_structure(raw);
    let raw_lines = raw.split('\n').collect::<Vec<_>>();
    let split = sentences_with_raw(&masked, raw);
    let tokenized = tokenize(&split, morphology)?;
    let sentence_max = thresholds
        .sentence_max
        .unwrap_or(if genre == Some("essay") { 110 } else { 90 });
    // 校正時、原稿で目視修正した文の修飾節は31〜76字に分布し、35字では31〜34字の
    // 文を取り逃した。随筆(青空文庫)でも30字で文の1.5%程度に収まる(eval/calibration.md)。
    let attributive_span_min = thresholds.attributive_span_min.unwrap_or(30);
    let kanji = Regex::new(r"[一-龿々]{7,}").expect("valid kanji-run regex");
    let conditional_negative =
        Regex::new(r"^(ない|なけれ|なく)(と|ば|ければ)").expect("valid conditional-negation regex");
    let mut findings = Vec::new();

    for sentence in &tokenized {
        let length = reading_length(&sentence.text);
        let excerpt = sentence.raw_text.chars().take(40).collect::<String>();
        // 読点ゼロの長文(カタログ B4の下位事例)。読点密度の検出はNO-GO済みだが、
        // 60字以上で読点が1つもない文は統語的な切れ目が示されず、実測でも
        // 人間文書の誤検知ゼロだった(eval/calibration.md)。日本語の読点診断なので、
        // URL・引用文献・コード断片のようなLatin優勢の行は対象にしない。
        let japanese_chars = sentence
            .text
            .chars()
            .filter(|c| matches!(c, 'ぁ'..='ん' | 'ァ'..='ヶ' | 'ー' | '一'..='鿿' | '々'))
            .count();
        if length >= 60
            && japanese_chars * 2 >= length
            && !sentence.text.contains(['、', '，', ','])
        {
            let mut finding = Finding::new(
                sentence.line,
                "no_comma_sentence",
                excerpt.clone(),
                "info",
                format!(
                    "一文{length}字に読点がない（目安60字）。カタログ B4。統語的な切れ目に読点を打つか、文を分ける"
                ),
            );
            finding.span = sentence.span(&raw_lines, 0, sentence.text.len());
            findings.push(finding);
        }
        if length > sentence_max {
            let mut finding = Finding::new(
                sentence.line,
                "sentence_too_long",
                excerpt,
                "info",
                format!(
                    "一文が{length}字（目安{sentence_max}字）。カタログ B1。一文一義になっているか確認する（分割の結果、字数が増えるのは正しい）"
                ),
            );
            finding.span = sentence.span(&raw_lines, 0, sentence.text.len());
            findings.push(finding);
        }

        for found in kanji.find_iter(&sentence.text) {
            let includes_proper_noun = sentence.tokens.iter().any(|token| {
                token.byte_end > found.start()
                    && token.byte_start < found.end()
                    && token.pos(1) == "固有名詞"
            });
            if includes_proper_noun {
                continue;
            }
            let mut finding = Finding::new(
                sentence.line,
                "kanji_run",
                found.as_str(),
                "info",
                format!(
                    "漢字が{}字連続（目安6字）。カタログ C1。語の切れ目が読み取れるか確認する",
                    found.as_str().chars().count()
                ),
            );
            finding.span = sentence.span(&raw_lines, found.start(), found.end());
            findings.push(finding);
        }

        if let Some((start, end, items)) = buried_list(&sentence.tokens) {
            let min_chars = if items <= 3 { 80 } else { 50 };
            if length >= min_chars {
                let phrase = sentence.tokens[start..end]
                    .iter()
                    .map(|token| token.surface.as_str())
                    .collect::<String>();
                let mut finding = Finding::new(
                    sentence.line,
                    "buried_list",
                    phrase.chars().take(40).collect::<String>(),
                    "info",
                    format!(
                        "同格の名詞句が読点で{items}個並んでいる（一文{length}字）。カタログ F1。箇条書きに開くと並列関係を読み手が再構成せずに済む"
                    ),
                );
                finding.span = sentence.span(
                    &raw_lines,
                    sentence.tokens[start].byte_start,
                    sentence.tokens[end - 1].byte_end,
                );
                findings.push(finding);
            }
        }

        // 疑問節「〜か、」を読点で並べ、末尾の「〜かを」で一つの述語へ係らせる文
        // (カタログ F1の節版)。短い選択肢の列挙は節の字数と述語の有無で外す。
        if length >= QUESTION_LIST_SENTENCE_MIN
            && let Some(list) = buried_question_list(&sentence.tokens, QUESTION_CLAUSE_MIN)
        {
            let phrase = sentence.tokens[list.start..list.end]
                .iter()
                .map(|token| token.surface.as_str())
                .collect::<String>();
            let particle = &sentence.tokens[list.end - 1].surface;
            let mut finding = Finding::new(
                sentence.line,
                "buried_question_list",
                phrase.chars().take(40).collect::<String>(),
                "info",
                format!(
                    "疑問節「〜か」が読点で{}個並び、末尾の「か{particle}」で一つの述語に係っている（一文{length}字）。カタログ F1。係り先の述語を先に出すか、節ごとに文を切る",
                    list.clauses
                ),
            );
            finding.span = sentence.span(
                &raw_lines,
                sentence.tokens[list.start].byte_start,
                sentence.tokens[list.end - 1].byte_end,
            );
            findings.push(finding);
        }

        // 一つの実質名詞に、述語を2つ以上含む長い修飾節が前置され、その名詞句が主節の
        // 項になる文(カタログ B2/B5)。読み手は名詞が出るまで修飾節全体を保留する。
        // 削除済みの nested_attributive は連体形の個数で全発火したため、ここでは
        // 個数ではなく一つの名詞が背負う修飾節の長さと述語数を測る。
        if let Some(span) =
            long_attributive_span(&sentence.text, &sentence.tokens, attributive_span_min)
        {
            let surface = |range: std::ops::Range<usize>| {
                sentence.tokens[range]
                    .iter()
                    .map(|token| token.surface.as_str())
                    .collect::<String>()
            };
            let head = surface(span.head..span.head_end);
            let phrase = surface(span.start..span.head_end);
            let phrase_chars = phrase.chars().count();
            let excerpt = if phrase_chars > 40 {
                format!(
                    "…{}",
                    phrase.chars().skip(phrase_chars - 39).collect::<String>()
                )
            } else {
                phrase
            };
            let mut finding = Finding::new(
                sentence.line,
                "long_attributive_span",
                excerpt,
                "info",
                format!(
                    "名詞「{head}」に{}字・述語{}個の修飾節が前置されている（目安{attributive_span_min}字）。カタログ B2/B5。被修飾名詞か述語を先に出すか、修飾節を独立した文に分ける",
                    span.chars, span.predicates
                ),
            );
            finding.span = sentence.span(
                &raw_lines,
                sentence.tokens[span.start].byte_start,
                sentence.tokens[span.head_end - 1].byte_end,
            );
            findings.push(finding);
        }

        let negative_indices = sentence
            .tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| {
                matches!(token.pos(0), "助動詞" | "形容詞")
                    && matches!(
                        token.dictionary_form(),
                        "ない" | "無い" | "ぬ" | "ず" | "ん"
                    )
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        for pair in negative_indices.windows(2) {
            let first = pair[0];
            let second = pair[1];
            let phrase = sentence.tokens[first..=second]
                .iter()
                .map(|token| token.surface.as_str())
                .collect::<String>();
            let obligation = [
                "といけ",
                "とだめ",
                "とダメ",
                "ばならな",
                "ばなりま",
                "ばいけな",
                "てはならな",
                "てはなりま",
                "てはいけな",
                "ざるを得",
                "ざるをえ",
            ]
            .iter()
            .any(|pattern| phrase.contains(pattern));
            if second - first <= 6
                && !punctuation_between(&sentence.tokens, first, second)
                && !obligation
                && !conditional_negative.is_match(&phrase)
                && !first_negation_modifies_noun(&sentence.tokens, first, second)
                // 「聞こえず話せない」は別述語。「見ずにはいられない」は残す。
                && !(sentence.tokens[first].surface == "ず"
                    && sentence.tokens.get(first + 1).is_some_and(|token| {
                        token.pos(0) == "動詞" && token.dictionary_form() != "いる"
                    }))
            {
                let mut finding = Finding::new(
                    sentence.line,
                    "double_negative",
                    phrase,
                    "info",
                    "否定が二重に掛かっている可能性。カタログ A1/A2。肯定に畳むなら真偽が反転していないか必ず確認する。控えめな肯定が本質的な箇所は触らない",
                );
                finding.span = sentence.span(
                    &raw_lines,
                    sentence.tokens[first].byte_start,
                    sentence.tokens[second].byte_end,
                );
                findings.push(finding);
                break;
            }
        }

        let no_indices = sentence
            .tokens
            .iter()
            .enumerate()
            // 「〜するのは」「〜なの」の準体助詞は名詞句の連鎖ではないので数えない。
            .filter(|(_, token)| {
                token.surface == "の" && token.pos(0) == "助詞" && token.pos(1) == "格助詞"
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        for window in no_indices.windows(3) {
            if window.windows(2).all(|pair| pair[1] - pair[0] <= 3)
                && !punctuation_between(&sentence.tokens, window[0], window[2])
            {
                let phrase = sentence.tokens[window[0]..=window[2]]
                    .iter()
                    .map(|token| token.surface.as_str())
                    .collect::<String>();
                let mut finding = Finding::new(
                    sentence.line,
                    "no_chain",
                    phrase,
                    "info",
                    "格助詞「の」が3連以上。カタログ C2。どこかを動詞・連用に開く",
                );
                finding.span = sentence.span(
                    &raw_lines,
                    sentence.tokens[window[0]].byte_start,
                    sentence.tokens[window[2]].byte_end,
                );
                findings.push(finding);
                break;
            }
        }
    }
    findings.sort_by_key(|finding| finding.line);
    let mut by_category = BTreeMap::new();
    for finding in &findings {
        *by_category.entry(finding.category.clone()).or_default() += 1;
    }
    Ok(ReadingLoadReport {
        stats: ReadingLoadStats {
            total: findings.len(),
            sentences: tokenized.len(),
            genre: genre.map(str::to_owned),
            by_category,
        },
        findings,
    })
}
