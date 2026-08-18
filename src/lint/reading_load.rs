//! 読解負荷レーン。自然度スコアとは分離した推敲用の指さしを返す。

use std::collections::BTreeMap;

use regex::Regex;

use crate::Error;
use crate::morphology::Morphology;
use crate::text::{mask_markdown_structure, sentences_with_raw};

use super::morph::{buried_list, punctuation_between, tokenize};
use super::{Finding, ReadingLoadReport, ReadingLoadStats, ReadingLoadThresholds};

fn reading_length(text: &str) -> usize {
    let whitespace = Regex::new(r"\s{2,}").expect("valid whitespace regex");
    whitespace.replace_all(text, " ").trim().chars().count()
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
            .filter(|(_, token)| token.surface == "の" && token.pos(0) == "助詞")
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
