//! 文書統計（リズム、n-gram、語彙多様性、具体性、段落、読者観測値）。

use std::collections::{BTreeMap, BTreeSet};

use regex::Regex;
use serde_json::{Value, json};

use crate::Error;
use crate::morphology::{Morpheme, Morphology};
use crate::text::{numbered_lines, sentences};

use super::Finding;
use super::morph::{CONTENT_POS, TokenizedSentence, mora_length, significant_tokens};

const ABSTRACT_NOUNS: &[&str] = &[
    "側面",
    "観点",
    "重要性",
    "可能性",
    "あり方",
    "存在",
    "意味",
    "本質",
    "価値",
    "意義",
    "課題",
    "問題",
    "要素",
    "要因",
    "背景",
    "傾向",
    "姿勢",
    "視点",
    "概念",
    "特徴",
    "性質",
    "状況",
    "状態",
    "変化",
];

const EXAMPLE_MARKERS: &[&str] = &[
    "たとえば",
    "例えば",
    "実際に",
    "実際には",
    "具体的には",
    "具体例として",
    "一例として",
    "先日",
    "昨日",
    "現に",
    "実例として",
];

const PARAGRAPH_CONJUNCTIONS: &[&str] = &[
    "しかし",
    "また",
    "そして",
    "そのため",
    "さらに",
    "つまり",
    "一方",
    "一方で",
    "このように",
    "なぜなら",
    "したがって",
    "ただし",
];

const SENTENCE_MODE_RUN_MIN: usize = 3;
const LONG_SENTENCE_MORA_MIN: usize = 30;
const SENTENCE_MODE_RUN_MAX_CV: f64 = 0.15;
const SHORT_NOMINAL_MORA_MAX: usize = 25;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum SentenceMode {
    Assertive,
    Tentative,
    Question,
    Nominal,
    Other,
}

impl SentenceMode {
    const ALL: [Self; 5] = [
        Self::Assertive,
        Self::Tentative,
        Self::Question,
        Self::Nominal,
        Self::Other,
    ];

    fn as_str(self) -> &'static str {
        match self {
            Self::Assertive => "assertive",
            Self::Tentative => "tentative",
            Self::Question => "question",
            Self::Nominal => "nominal",
            Self::Other => "other",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Assertive => "断定",
            Self::Tentative => "推量・保留",
            Self::Question => "疑問",
            Self::Nominal => "体言止め",
            Self::Other => "その他",
        }
    }
}

#[derive(Debug)]
struct SentenceEndingObservation<'a> {
    sentence: &'a TokenizedSentence,
    mode: SentenceMode,
    signature: &'static str,
    mora: usize,
}

fn sentence_ending(sentence: &TokenizedSentence) -> (SentenceMode, &'static str) {
    if matches!(sentence.end_mark, Some('？' | '?')) {
        return (SentenceMode::Question, "question_mark");
    }

    let tokens = sentence
        .tokens
        .iter()
        .filter(|token| !matches!(token.pos(0), "記号" | "補助記号" | "空白"))
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        return (SentenceMode::Other, "other");
    }
    if tokens
        .last()
        .is_some_and(|token| token.pos(0) == "助詞" && token.surface == "か")
    {
        return (SentenceMode::Question, "particle_ka");
    }

    let ending = tokens
        .iter()
        .rev()
        .take(8)
        .rev()
        .map(|token| token.surface.as_str())
        .collect::<String>();
    if let Some(suffix) = [
        "かもしれない",
        "かもしれません",
        "かもしれなかった",
        "に違いない",
        "に違いありません",
        "だろう",
        "でしょう",
        "ようだ",
        "ようです",
        "らしい",
        "らしいです",
        "と思う",
        "と思います",
    ]
    .iter()
    .find(|suffix| ending.ends_with(**suffix))
    {
        return (SentenceMode::Tentative, *suffix);
    }
    if super::morph::noun_ended(&sentence.tokens) {
        return (SentenceMode::Nominal, "nominal");
    }
    if let Some(suffix) = [
        "である",
        "であった",
        "だった",
        "なのだ",
        "のだ",
        "というわけだ",
        "です",
        "でした",
        "だ",
    ]
    .iter()
    .find(|suffix| ending.ends_with(**suffix))
    {
        return (SentenceMode::Assertive, *suffix);
    }
    (SentenceMode::Other, "other")
}

fn same_paragraph(previous: &TokenizedSentence, current: &TokenizedSentence) -> bool {
    current.line <= previous.line + 1
}

fn sentence_ending_observations(
    tokenized: &[TokenizedSentence],
) -> Vec<SentenceEndingObservation<'_>> {
    tokenized
        .iter()
        .map(|sentence| {
            let (mode, signature) = sentence_ending(sentence);
            SentenceEndingObservation {
                sentence,
                mode,
                signature,
                mora: mora_length(&sentence.tokens),
            }
        })
        .collect()
}

fn sentence_ending_stats(observations: &[SentenceEndingObservation<'_>]) -> Value {
    let mut counts = BTreeMap::new();
    let mut longest_runs = BTreeMap::new();
    for mode in SentenceMode::ALL {
        counts.insert(mode.as_str(), 0_usize);
        longest_runs.insert(mode.as_str(), 0_usize);
    }

    let mut previous: Option<&SentenceEndingObservation<'_>> = None;
    let mut current_run = 0;
    for observation in observations {
        *counts
            .get_mut(observation.mode.as_str())
            .expect("all sentence modes initialized") += 1;
        if previous.is_some_and(|previous| {
            previous.mode == observation.mode
                && same_paragraph(previous.sentence, observation.sentence)
        }) {
            current_run += 1;
        } else {
            current_run = 1;
        }
        let longest = longest_runs
            .get_mut(observation.mode.as_str())
            .expect("all sentence modes initialized");
        *longest = (*longest).max(current_run);
        previous = Some(observation);
    }

    json!({"counts": counts, "longest_runs": longest_runs})
}

fn matching_runs<'a>(
    observations: &'a [SentenceEndingObservation<'a>],
    eligible: impl Fn(&SentenceEndingObservation<'_>) -> bool,
) -> Vec<&'a [SentenceEndingObservation<'a>]> {
    let mut runs = Vec::new();
    let mut start = 0;
    while start < observations.len() {
        if !eligible(&observations[start]) {
            start += 1;
            continue;
        }
        let mode = observations[start].mode;
        let mut end = start + 1;
        while end < observations.len()
            && eligible(&observations[end])
            && observations[end].mode == mode
            && observations[end].signature == observations[start].signature
            && same_paragraph(observations[end - 1].sentence, observations[end].sentence)
        {
            end += 1;
        }
        if end - start >= SENTENCE_MODE_RUN_MIN {
            runs.push(&observations[start..end]);
        }
        start = end;
    }
    runs
}

fn run_lines(run: &[SentenceEndingObservation<'_>]) -> Vec<usize> {
    run.iter()
        .map(|observation| observation.sentence.line)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn run_mora_cv(run: &[SentenceEndingObservation<'_>]) -> f64 {
    let values = run
        .iter()
        .map(|observation| observation.mora as f64)
        .collect::<Vec<_>>();
    mean_and_stdev(&values).map_or(
        0.0,
        |(mean, stdev)| {
            if mean == 0.0 { 0.0 } else { stdev / mean }
        },
    )
}

fn sentence_ending_findings(observations: &[SentenceEndingObservation<'_>]) -> Vec<Finding> {
    // 三文だけの例文集や境界fixtureを文書のリズムとして扱わない。
    if observations.len() < 6 {
        return Vec::new();
    }
    let mut findings = Vec::new();
    for run in matching_runs(observations, |observation| {
        matches!(
            observation.mode,
            SentenceMode::Assertive | SentenceMode::Tentative | SentenceMode::Question
        ) && observation.mora >= LONG_SENTENCE_MORA_MIN
    }) {
        let mora_cv = run_mora_cv(run);
        if mora_cv > SENTENCE_MODE_RUN_MAX_CV {
            continue;
        }
        let mode = run[0].mode;
        let signature = &run[0].signature;
        let mut finding = Finding::new(
            run[0].sentence.line,
            "repeated_sentence_mode",
            run[0]
                .sentence
                .raw_text
                .chars()
                .take(40)
                .collect::<String>(),
            "info",
            format!(
                "{}モーラ以上の{}型「{}」が{}文連続し、文長の変動係数={:.3}（上限{:.2}）。同じ文末表現と長さが続き、局所的なリズムが平坦な疑い",
                LONG_SENTENCE_MORA_MIN,
                mode.label(),
                signature,
                run.len(),
                mora_cv,
                SENTENCE_MODE_RUN_MAX_CV
            ),
        );
        finding.related_lines = Some(run_lines(run));
        findings.push(finding);
    }
    for run in matching_runs(observations, |observation| {
        observation.mode == SentenceMode::Nominal && observation.mora <= SHORT_NOMINAL_MORA_MAX
    }) {
        let mut finding = Finding::new(
            run[0].sentence.line,
            "consecutive_nominal_endings",
            run[0]
                .sentence
                .raw_text
                .chars()
                .take(40)
                .collect::<String>(),
            "info",
            format!(
                "{}モーラ以下の体言止めが{}文連続（閾値{}文以上）。短い停止の反復が文章の流れを細切れにしている疑い",
                SHORT_NOMINAL_MORA_MAX,
                run.len(),
                SENTENCE_MODE_RUN_MIN
            ),
        );
        finding.related_lines = Some(run_lines(run));
        findings.push(finding);
    }
    findings
}

pub(super) fn mean_and_stdev(values: &[f64]) -> Option<(f64, f64)> {
    if values.is_empty() {
        return None;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance = values
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / values.len() as f64;
    Some((mean, variance.sqrt()))
}

pub(super) fn rhythm_analysis(tokenized: &[TokenizedSentence]) -> (Vec<Finding>, Value) {
    let ending_observations = sentence_ending_observations(tokenized);
    let ending_stats = sentence_ending_stats(&ending_observations);
    let mut findings = sentence_ending_findings(&ending_observations);
    if tokenized.len() < 6 {
        return (findings, json!({"sentence_endings": ending_stats}));
    }
    let lengths = tokenized
        .iter()
        .map(|sentence| mora_length(&sentence.tokens) as f64)
        .collect::<Vec<_>>();
    let (mean, stdev) = mean_and_stdev(&lengths).expect("non-empty lengths");
    let burstiness = if stdev + mean == 0.0 {
        0.0
    } else {
        (stdev - mean) / (stdev + mean)
    };
    let xs = &lengths[..lengths.len() - 1];
    let ys = &lengths[1..];
    let autocorrelation = if xs.len() >= 4 {
        let (x_mean, x_stdev) = mean_and_stdev(xs).expect("non-empty x series");
        let (y_mean, y_stdev) = mean_and_stdev(ys).expect("non-empty y series");
        if x_stdev > 0.0 && y_stdev > 0.0 {
            let covariance = xs
                .iter()
                .zip(ys)
                .map(|(x, y)| (x - x_mean) * (y - y_mean))
                .sum::<f64>()
                / xs.len() as f64;
            Some(covariance / (x_stdev * y_stdev))
        } else {
            None
        }
    } else {
        None
    };
    if burstiness < -0.24 {
        findings.push(Finding::new(
            tokenized[0].line,
            "low_burstiness",
            format!(
                "burstiness={burstiness:.3} (モーラ近似長 平均={mean:.1}, 標準偏差={stdev:.1})"
            ),
            "warn",
            "burstiness が閾値(-0.24)未満。文の長短のメリハリが乏しく機械的なリズムの疑い",
        ));
    }
    if autocorrelation.is_some_and(|value| value > 0.6) {
        findings.push(Finding::new(
            tokenized[0].line,
            "high_length_autocorrelation",
            format!("lag-1 自己相関={:.3}", autocorrelation.unwrap_or_default()),
            "info",
            "隣接する文の長さが強く相関（閾値0.6超）。文長パターンが単調に繰り返されている疑い",
        ));
    }
    (
        findings,
        json!({
            "mora_mean": mean,
            "mora_stdev": stdev,
            "burstiness": burstiness,
            "length_autocorrelation_lag1": autocorrelation,
            "sentence_endings": ending_stats,
        }),
    )
}

pub(super) fn ngram_analysis(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
    genre: Option<&str>,
    repeated_sentence_lead: Option<usize>,
) -> (Vec<Finding>, Value) {
    let lead_threshold = repeated_sentence_lead.unwrap_or(match genre {
        Some("essay") => 5,
        Some("tech" | "business") => 7,
        _ => 6,
    });
    let mut findings = Vec::new();
    let leads = tokenized
        .iter()
        .filter_map(|sentence| {
            let tokens = significant_tokens(&sentence.tokens);
            if tokens.len() < 2 {
                return None;
            }
            let lead = format!("{}{}", tokens[0].surface, tokens[1].surface);
            let tech_lead = (tokens[0].pos(0) == "名詞" && tokens[0].pos(1) == "固有名詞")
                || (tokens[0]
                    .surface
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_alphabetic())
                    && tokens[0]
                        .surface
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')));
            Some((sentence, lead, tech_lead))
        })
        .collect::<Vec<_>>();
    let mut lead_counts = BTreeMap::<String, usize>::new();
    for (_, lead, _) in &leads {
        *lead_counts.entry(lead.clone()).or_default() += 1;
    }
    // 同じ反復キーは文書単位で1件に集約し、全対象行はrelated_linesで示す。
    let mut reported = BTreeSet::new();
    for (sentence, lead, tech_lead) in &leads {
        let count = lead_counts[lead];
        if count < lead_threshold || !reported.insert(lead.clone()) {
            continue;
        }
        let occurrences = leads
            .iter()
            .filter(|(_, candidate, _)| candidate == lead)
            .collect::<Vec<_>>();
        let lines = occurrences
            .iter()
            .map(|(sentence, _, _)| sentence.line)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let related = lines
            .iter()
            .map(|line| format!("L{line}"))
            .collect::<Vec<_>>()
            .join(", ");
        // 文頭3有意トークン内のコロン、またはQ./A.のような短いマーカー+区切りが
        // 過半数なら、用語集やFAQのような定型フィールドのラベルとみなす。
        let label_like = occurrences
            .iter()
            .filter(|(sentence, _, _)| {
                let tokens = significant_tokens(&sentence.tokens);
                let colon_in_head = tokens
                    .iter()
                    .take(3)
                    .any(|token| matches!(token.surface.as_str(), "：" | ":"));
                let short_marker = tokens.len() >= 2
                    && tokens[0].surface.chars().count() <= 2
                    && tokens[0]
                        .surface
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric())
                    && matches!(tokens[1].surface.as_str(), "." | "．" | ")" | "）");
                colon_in_head || short_marker
            })
            .count()
            * 2
            >= count;
        let qualifier = if label_like {
            "ラベル+コロンの定型フィールドとみられ、意図的な構造なら言い換えの対象にしない"
        } else if *tech_lead {
            "固有名詞/技術用語由来の可能性が高い"
        } else {
            "人間の意図的な反復技法との区別がつかないため参考情報として提示"
        };
        let example = sentence.raw_text.chars().take(20).collect::<String>();
        let mut finding = Finding::new(
            sentence.line,
            "repeated_sentence_lead",
            lead.clone(),
            "info",
            format!(
                "文頭2形態素「{lead}」が{count}回反復（閾値{lead_threshold}回以上）。文書単位の集約finding。{qualifier}。初出の文頭: 「{example}」。対応箇所: {related}"
            ),
        );
        let lead_tokens = significant_tokens(&sentence.tokens);
        if lead_tokens.len() >= 2 {
            finding.span = sentence.span(
                raw_lines,
                lead_tokens[0].byte_start,
                lead_tokens[1].byte_end,
            );
        }
        finding.related_lines = Some(lines);
        findings.push(finding);
    }

    let pos_ngrams = tokenized
        .iter()
        .filter_map(|sentence| {
            let tokens = significant_tokens(&sentence.tokens);
            if tokens.len() < 4 {
                return None;
            }
            Some((
                sentence,
                tokens[..4]
                    .iter()
                    .map(|token| token.pos(0).to_owned())
                    .collect::<Vec<_>>(),
            ))
        })
        .collect::<Vec<_>>();
    let mut ordered_counts = Vec::<(Vec<String>, usize)>::new();
    for (_, sequence) in &pos_ngrams {
        if let Some((_, count)) = ordered_counts
            .iter_mut()
            .find(|(candidate, _)| candidate == sequence)
        {
            *count += 1;
        } else {
            ordered_counts.push((sequence.clone(), 1));
        }
    }
    let top = ordered_counts
        .iter()
        .max_by_key(|(_, count)| *count)
        .cloned();
    let mut top_text: Option<String> = None;
    let mut top_ratio: Option<f64> = None;
    if pos_ngrams.len() >= 6
        && let Some((top_sequence, top_count)) = top
    {
        let ratio = top_count as f64 / pos_ngrams.len() as f64;
        top_text = Some(top_sequence.join("/"));
        top_ratio = Some(ratio);
        if ratio >= 0.4 {
            let lines = pos_ngrams
                .iter()
                .filter(|(_, sequence)| sequence == &top_sequence)
                .map(|(sentence, _)| sentence.line)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let related = lines
                .iter()
                .map(|line| format!("L{line}"))
                .collect::<Vec<_>>()
                .join(", ");
            for (sentence, sequence) in &pos_ngrams {
                if sequence != &top_sequence {
                    continue;
                }
                let mut finding = Finding::new(
                    sentence.line,
                    "repeated_syntax_template",
                    sentence.raw_text.chars().take(20).collect::<String>(),
                    "info",
                    format!(
                        "文頭品詞4-gram「{}」が全文の{:.1}%で一致（閾値40%以上）。構文テンプレートの使い回しの疑い。対応箇所: {related}",
                        top_sequence.join("/"),
                        ratio * 100.0
                    ),
                );
                let template_tokens = significant_tokens(&sentence.tokens);
                if template_tokens.len() >= 4 {
                    finding.span = sentence.span(
                        raw_lines,
                        template_tokens[0].byte_start,
                        template_tokens[3].byte_end,
                    );
                }
                finding.related_lines = Some(lines.clone());
                findings.push(finding);
            }
        }
    }
    (
        findings,
        json!({"lead_pos_4gram_top": top_text, "lead_pos_4gram_ratio": top_ratio}),
    )
}

fn mtld(tokens: &[String], threshold: f64) -> Option<f64> {
    if tokens.len() < 20 {
        return None;
    }
    fn direction(tokens: impl Iterator<Item = String>, length: usize, threshold: f64) -> f64 {
        let mut factor_count = 0.0;
        let mut types = BTreeSet::new();
        let mut token_count = 0;
        for token in tokens {
            types.insert(token);
            token_count += 1;
            if types.len() as f64 / token_count as f64 <= threshold {
                factor_count += 1.0;
                types.clear();
                token_count = 0;
            }
        }
        if token_count > 0 {
            let ttr = types.len() as f64 / token_count as f64;
            if ttr < 1.0 {
                factor_count += ((1.0 - ttr) / (1.0 - threshold)).min(1.0);
            }
        }
        if factor_count > 0.0 {
            length as f64 / factor_count
        } else {
            length as f64
        }
    }
    let forward = direction(tokens.iter().cloned(), tokens.len(), threshold);
    let backward = direction(tokens.iter().rev().cloned(), tokens.len(), threshold);
    Some((forward + backward) / 2.0)
}

pub(super) fn lexical_diversity_analysis(
    tokenized: &[TokenizedSentence],
    ttr_threshold: f64,
    mtld_threshold: f64,
) -> (Vec<Finding>, Value) {
    let content = tokenized
        .iter()
        .flat_map(|sentence| sentence.tokens.iter())
        .filter(|token| CONTENT_POS.contains(&token.pos(0)))
        .map(|token| token.dictionary_form().to_owned())
        .collect::<Vec<_>>();
    let doc_chars = tokenized
        .iter()
        .map(|sentence| sentence.raw_text.chars().count())
        .sum::<usize>();
    if doc_chars < 4000 {
        return (
            Vec::new(),
            json!({
                "ttr": Value::Null,
                "mtld": Value::Null,
                "content_token_count": content.len(),
                "doc_char_count": doc_chars,
                "skipped_too_short": true,
            }),
        );
    }
    let unique = content.iter().collect::<BTreeSet<_>>().len();
    let ttr = if content.is_empty() {
        None
    } else {
        Some(unique as f64 / content.len() as f64)
    };
    let mtld_value = mtld(&content, 0.72);
    let mut findings = Vec::new();
    if ttr.is_some_and(|value| value < ttr_threshold) {
        findings.push(Finding::new(
            tokenized.first().map_or(1, |sentence| sentence.line),
            "low_lexical_diversity_ttr",
            format!(
                "TTR={:.3} (内容語 {} 語中 {unique} 種類)",
                ttr.unwrap_or_default(),
                content.len()
            ),
            "info",
            format!(
                "TTR(Type-Token Ratio)が閾値{ttr_threshold:.2}未満。同じ語彙の使い回しが多い疑い"
            ),
        ));
    }
    if mtld_value.is_some_and(|value| value < mtld_threshold) {
        findings.push(Finding::new(
            tokenized.first().map_or(1, |sentence| sentence.line),
            "low_lexical_diversity_mtld",
            format!("MTLD={:.1}", mtld_value.unwrap_or_default()),
            "info",
            format!("MTLD が閾値{mtld_threshold:.1}未満。文章長で正規化した語彙多様性が低い疑い"),
        ));
    }
    (
        findings,
        json!({
            "ttr": ttr,
            "mtld": mtld_value,
            "content_token_count": content.len(),
            "doc_char_count": doc_chars,
            "skipped_too_short": false,
        }),
    )
}

pub(super) fn paragraphs(text: &str) -> Vec<Vec<(usize, &str)>> {
    let mut output = Vec::new();
    let mut current = Vec::new();
    for (line_no, line) in numbered_lines(text) {
        if line.trim().is_empty() {
            if !current.is_empty() {
                output.push(std::mem::take(&mut current));
            }
        } else {
            current.push((line_no, line));
        }
    }
    if !current.is_empty() {
        output.push(current);
    }
    output
}

pub(super) fn low_specificity_analysis(
    masked: &str,
    raw: &str,
    morphology: &Morphology,
    score_threshold: f64,
) -> Result<(Vec<Finding>, Value), Error> {
    let raw_lines = raw.split('\n').collect::<Vec<_>>();
    let numeric = Regex::new(
        r"[0-9０-９]+(年代|年間|世紀|年|月|日|時間|時|分|秒|人|円|%|％|kg|km|cm|mm|g|m|回|件|個|つ|割|倍|台|社|名|冊|本|杯|軒)?",
    )
    .expect("valid numeric quantity regex");
    let mut findings = Vec::new();
    let mut evaluated = 0;
    let mut fired = 0;
    for paragraph in paragraphs(masked) {
        let first_line = paragraph[0].0;
        let text = paragraph
            .iter()
            .map(|(_, line)| *line)
            .collect::<Vec<_>>()
            .join("\n");
        if text.chars().count() < 80 {
            continue;
        }
        let mut tokens = Vec::new();
        for (_, line) in &paragraph {
            tokens.extend(morphology.tokenize(line)?);
        }
        let content = tokens
            .iter()
            .filter(|token| CONTENT_POS.contains(&token.pos(0)))
            .collect::<Vec<_>>();
        if content.len() < 15 {
            continue;
        }
        evaluated += 1;
        let proper = content
            .iter()
            .filter(|token| token.pos(0) == "名詞" && token.pos(1) == "固有名詞")
            .count();
        let abstract_count = content
            .iter()
            .filter(|token| {
                token.pos(0) == "名詞" && ABSTRACT_NOUNS.contains(&token.dictionary_form())
            })
            .count();
        let numeric_count = numeric.find_iter(&text).count();
        let has_example = EXAMPLE_MARKERS.iter().any(|marker| text.contains(marker));
        let count = content.len() as f64;
        let proper_density = proper as f64 / count;
        let numeric_density = numeric_count as f64 / count;
        let abstract_ratio = abstract_count as f64 / count;
        let score = proper_density + numeric_density + if has_example { 0.1 } else { 0.0 }
            - abstract_ratio * 1.5;
        if score < score_threshold {
            fired += 1;
            let excerpt = raw_lines
                .get(first_line - 1)
                .copied()
                .unwrap_or(paragraph[0].1)
                .trim()
                .chars()
                .take(40)
                .collect::<String>();
            findings.push(Finding::new(
                first_line,
                "low_specificity",
                excerpt,
                "info",
                format!(
                    "段落の具体性スコア={score:.3}（閾値{score_threshold:.2}未満）。固有名詞密度={proper_density:.3}, 数値密度={numeric_density:.3}, 抽象名詞率={abstract_ratio:.3}, 例示マーカー={}。固有名詞・数値・実例が乏しく一般論に留まっている疑い。素材不足のサインであり、文体の修正でなく情報収集を検討する（revision-guide.md の素材不足の分岐を参照）",
                    if has_example { "あり" } else { "なし" }
                ),
            ));
        }
    }
    Ok((
        findings,
        json!({"paragraphs_evaluated": evaluated, "paragraphs_fired": fired}),
    ))
}

pub(super) struct ParagraphAnalysis {
    pub(super) findings: Vec<Finding>,
    pub(super) total: usize,
    pub(super) conjunction_count: usize,
    pub(super) conjunction_ratio: f64,
    pub(super) sentence_counts: Vec<usize>,
    pub(super) sentence_count_cv: Option<f64>,
}

pub(super) fn analyze_paragraphs(masked: &str, raw: &str) -> ParagraphAnalysis {
    let raw_lines = raw.split('\n').collect::<Vec<_>>();
    let groups = paragraphs(masked);
    let total = groups.len();
    let sentence_counts = groups
        .iter()
        .map(|group| {
            let text = group
                .iter()
                .map(|(_, line)| *line)
                .collect::<Vec<_>>()
                .join("\n");
            sentences(&text).len()
        })
        .collect::<Vec<_>>();
    let conjunctions = groups
        .iter()
        .filter_map(|group| {
            let (line_no, line) = group[0];
            let trimmed = line.trim();
            PARAGRAPH_CONJUNCTIONS
                .iter()
                .find(|conjunction| trimmed.starts_with(**conjunction))
                .map(|conjunction| (line_no, *conjunction))
        })
        .collect::<Vec<_>>();
    let conjunction_ratio = if total == 0 {
        0.0
    } else {
        conjunctions.len() as f64 / total as f64
    };
    let mut findings = Vec::new();
    if total >= 3 && conjunction_ratio >= 0.3 {
        let lines = conjunctions
            .iter()
            .map(|(line, _)| *line)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let related = lines
            .iter()
            .map(|line| format!("L{line}"))
            .collect::<Vec<_>>()
            .join(", ");
        for (line, conjunction) in &conjunctions {
            let excerpt = raw_lines
                .get(line - 1)
                .copied()
                .unwrap_or_default()
                .chars()
                .take(40)
                .collect::<String>();
            let mut finding = Finding::new(
                *line,
                "paragraph_lead_conjunction",
                excerpt,
                "info",
                format!(
                    "段落頭が接続詞「{conjunction}」で始まる（文書全体の段落頭接続詞率={:.1}%、閾値30%以上で警告）。対応箇所: {related}",
                    conjunction_ratio * 100.0
                ),
            );
            finding.related_lines = Some(lines.clone());
            findings.push(finding);
        }
    }
    let sentence_count_cv = if sentence_counts.len() >= 4 {
        let values = sentence_counts
            .iter()
            .map(|count| *count as f64)
            .collect::<Vec<_>>();
        mean_and_stdev(&values).map(|(mean, stdev)| if mean == 0.0 { 0.0 } else { stdev / mean })
    } else {
        None
    };
    if sentence_count_cv.is_some_and(|cv| cv < 0.15) {
        findings.push(Finding::new(
            1,
            "uniform_paragraph_structure",
            format!(
                "段落数={}, 各段落の文数={sentence_counts:?}",
                sentence_counts.len()
            ),
            "info",
            format!(
                "段落あたり文数の変動係数={:.3}（閾値0.15未満）。どの段落もほぼ同じ文数=定型段落の疑い",
                sentence_count_cv.unwrap_or_default()
            ),
        ));
    }
    ParagraphAnalysis {
        findings,
        total,
        conjunction_count: conjunctions.len(),
        conjunction_ratio,
        sentence_counts,
        sentence_count_cv,
    }
}

/// 文頭が接続詞で始まる文の割合。日本語のAI生成文は文ごとに接続詞を
/// 置く傾向が報告されているため、まず観測値として出し、コーパスで
/// 人間/AIの分離力を確認してからfinding化を判断する。
pub(super) fn conjunction_observations(tokenized: &[TokenizedSentence]) -> Value {
    if tokenized.is_empty() {
        return json!({
            "sentence_lead_conjunction_count": 0,
            "sentence_lead_conjunction_ratio": Value::Null,
        });
    }
    let count = tokenized
        .iter()
        .filter(|sentence| {
            PARAGRAPH_CONJUNCTIONS
                .iter()
                .any(|conjunction| sentence.text.starts_with(conjunction))
        })
        .count();
    json!({
        "sentence_lead_conjunction_count": count,
        "sentence_lead_conjunction_ratio": count as f64 / tokenized.len() as f64,
    })
}

/// 読者別難易度スコアは校正データが揃うまで実装しない。判定に使える
/// 観測値（文長、品詞比率、文字種比率の近似）だけをmeasurementsへ出す。
/// 文字種は語種（漢語/和語/外来語）の辞書的判定ではなく表層の近似。
pub(super) fn readability_observations(tokenized: &[TokenizedSentence]) -> Value {
    let tokens = tokenized
        .iter()
        .flat_map(|sentence| sentence.tokens.iter())
        .filter(|token| !matches!(token.pos(0), "記号" | "補助記号" | "空白"))
        .collect::<Vec<_>>();
    if tokens.is_empty() || tokenized.is_empty() {
        return json!({"skipped_empty": true});
    }
    let total = tokens.len() as f64;
    let ratio_of = |predicate: &dyn Fn(&&&Morpheme) -> bool| {
        tokens.iter().filter(|token| predicate(token)).count() as f64 / total
    };
    let mean_sentence_chars = tokenized
        .iter()
        .map(|sentence| sentence.raw_text.chars().count())
        .sum::<usize>() as f64
        / tokenized.len() as f64;
    json!({
        "skipped_empty": false,
        "mean_sentence_chars": mean_sentence_chars,
        "verb_token_ratio": ratio_of(&|token| token.pos(0) == "動詞"),
        "particle_token_ratio": ratio_of(&|token| token.pos(0) == "助詞"),
        "script_kanji_token_ratio": ratio_of(&|token| script(&token.surface) == "kanji"),
        "script_hiragana_token_ratio": ratio_of(&|token| script(&token.surface) == "hiragana"),
        "script_katakana_token_ratio": ratio_of(&|token| script(&token.surface) == "katakana"),
        "script_ascii_token_ratio": ratio_of(&|token| script(&token.surface) == "ascii"),
    })
}

fn script(surface: &str) -> &'static str {
    let mut kanji = false;
    let mut hiragana = false;
    let mut katakana = false;
    let mut ascii = false;
    let mut other = false;
    for ch in surface.chars() {
        match ch {
            '一'..='\u{9FFF}' | '々' => kanji = true,
            'ぁ'..='ゖ' => hiragana = true,
            'ァ'..='ヶ' | 'ー' => katakana = true,
            _ if ch.is_ascii() => ascii = true,
            _ => other = true,
        }
    }
    match (kanji, hiragana, katakana, ascii, other) {
        (true, false, false, false, false) => "kanji",
        (false, true, false, false, false) => "hiragana",
        (false, false, true, false, false) => "katakana",
        (false, false, false, true, false) => "ascii",
        _ => "mixed_or_other",
    }
}
