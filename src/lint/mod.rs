//! 自然さ・構造・読解負荷の検出器。公開APIはこのモジュールに集約し、
//! 検出器の実装はpatterns(表層)、morph(品詞列)、metrics(統計)、
//! reading_load(読解負荷レーン)、baseline(前回比較)へ分割している。

mod baseline;
mod metrics;
mod morph;
mod patterns;
mod reading_load;

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Value, json};

use crate::Error;
use crate::morphology::Morphology;
use crate::text::{mask_html_comments, mask_markdown_structure_with_stats, sentences_with_raw};

pub use baseline::{BaselineReport, BaselineSummary, apply_baseline, baseline_added};
pub use reading_load::{analyze_reading_load, analyze_reading_load_with_thresholds};

const EXPERIMENTAL_CATEGORIES: &[&str] = &[
    "high_length_autocorrelation",
    "paragraph_lead_conjunction",
    "repeated_syntax_template",
    "english_syntax_cleft_because",
    "high_bold_density",
    "high_bullet_ratio",
    "boilerplate_heading",
    "numbered_phase_structure",
    "high_emoji_symbol_density",
];

const RULE_CATEGORIES: &[&str] = &[
    "antithesis_repetition",
    "boilerplate_heading",
    "bullet_bold_label",
    "bullet_emoji",
    "buried_list",
    "double_negative",
    "english_syntax_cleft_because",
    "english_syntax_inanimate_subject",
    "forbidden_phrase",
    "high_bold_density",
    "high_bullet_ratio",
    "high_emoji_symbol_density",
    "high_length_autocorrelation",
    "hype_expression",
    "inanimate_subject_morph",
    "kanji_run",
    "low_burstiness",
    "low_lexical_diversity_mtld",
    "low_lexical_diversity_ttr",
    "low_sentence_variance",
    "low_specificity",
    "no_chain",
    "no_comma_sentence",
    "nominal_ending",
    "numbered_phase_structure",
    "paragraph_lead_conjunction",
    "predicate_colon_lead",
    "redundant_light_verb",
    "repeated_sentence_lead",
    "repeated_syntax_template",
    "sentence_too_long",
    "translationese",
    "translationese_morph",
    "uniform_paragraph_structure",
];

const READING_LOAD_CATEGORIES: &[&str] = &[
    "buried_list",
    "double_negative",
    "kanji_run",
    "no_chain",
    "no_comma_sentence",
    "sentence_too_long",
];

pub fn is_known_rule(category: &str) -> bool {
    RULE_CATEGORIES.contains(&category)
}

pub fn rule_categories() -> &'static [&'static str] {
    RULE_CATEGORIES
}

pub fn reading_load_categories() -> &'static [&'static str] {
    READING_LOAD_CATEGORIES
}

/// findingが指す本文範囲。行は1始まり、columnはUnicode scalar数え・1始まりで
/// 半開区間、byteは各行内のUTF-8 offset・0始まりで半開区間。
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Span {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub start_byte: usize,
    pub end_byte: usize,
}

/// 機械的に安全と確認した置換候補。`span`の範囲が`preimage`と一致する場合に
/// 限って適用できる。Suiko自身はファイルを書き換えない。
#[derive(Clone, Debug, Serialize)]
pub struct Suggestion {
    pub span: Span,
    pub preimage: String,
    pub replacement: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Finding {
    pub line: usize,
    pub category: String,
    pub excerpt: String,
    pub severity: String,
    pub detail: String,
    pub related_lines: Option<Vec<usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<Suggestion>,
}

impl Finding {
    fn new(
        line: usize,
        category: &str,
        excerpt: impl Into<String>,
        severity: &str,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            line,
            category: category.to_owned(),
            excerpt: excerpt.into(),
            severity: severity.to_owned(),
            detail: detail.into(),
            related_lines: None,
            status: None,
            span: None,
            suggestion: None,
        }
    }
}

fn column_at(raw_line: &str, byte: usize) -> Option<usize> {
    if byte > raw_line.len() || !raw_line.is_char_boundary(byte) {
        return None;
    }
    Some(raw_line[..byte].chars().count() + 1)
}

fn make_span(
    raw_lines: &[&str],
    start_line: usize,
    start_byte: usize,
    end_line: usize,
    end_byte: usize,
) -> Option<Span> {
    if start_line > end_line || (start_line == end_line && start_byte >= end_byte) {
        return None;
    }
    let start_column = column_at(raw_lines.get(start_line - 1)?, start_byte)?;
    let end_column = column_at(raw_lines.get(end_line - 1)?, end_byte)?;
    Some(Span {
        start_line,
        start_column,
        end_line,
        end_column,
        start_byte,
        end_byte,
    })
}

#[derive(Clone, Debug, Serialize)]
pub struct LintStats {
    pub total_findings: usize,
    pub by_category: BTreeMap<String, usize>,
    pub genre: Option<String>,
    pub experimental: bool,
    #[serde(flatten)]
    pub measurements: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LintReport {
    pub stats: LintStats,
    pub findings: Vec<Finding>,
}

#[derive(Clone, Copy, Debug)]
pub struct AnalysisThresholds {
    pub repeated_sentence_lead: Option<usize>,
    pub lexical_ttr: f64,
    pub lexical_mtld: f64,
    pub low_specificity: f64,
    pub nominal_ending_max_ratio: f64,
}

impl Default for AnalysisThresholds {
    fn default() -> Self {
        Self {
            repeated_sentence_lead: None,
            lexical_ttr: 0.45,
            lexical_mtld: 40.0,
            low_specificity: -0.15,
            nominal_ending_max_ratio: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ReadingLoadThresholds {
    pub sentence_max: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReadingLoadStats {
    pub total: usize,
    pub sentences: usize,
    pub genre: Option<String>,
    pub by_category: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReadingLoadReport {
    pub stats: ReadingLoadStats,
    pub findings: Vec<Finding>,
}

pub fn analyze(
    raw: &str,
    morphology: &Morphology,
    genre: Option<&str>,
    experimental: bool,
) -> Result<LintReport, Error> {
    analyze_with_thresholds(
        raw,
        morphology,
        genre,
        experimental,
        AnalysisThresholds::default(),
    )
}

pub fn analyze_with_thresholds(
    raw: &str,
    morphology: &Morphology,
    genre: Option<&str>,
    experimental: bool,
    thresholds: AnalysisThresholds,
) -> Result<LintReport, Error> {
    let (masked, mask_stats) = mask_markdown_structure_with_stats(raw);
    let raw_lines = raw.split('\n').collect::<Vec<_>>();
    let split = sentences_with_raw(&masked, raw);
    let tokenized = morph::tokenize(&split, morphology)?;
    let critical_above = if genre == Some("tech") { 0.045 } else { 0.03 };

    let (structural_findings, structural_stats) =
        patterns::structural_analysis(&mask_html_comments(raw));
    let (rhythm_findings, rhythm_stats) = metrics::rhythm_analysis(&tokenized);
    let (ngram_findings, ngram_stats) = metrics::ngram_analysis(
        &tokenized,
        &raw_lines,
        genre,
        thresholds.repeated_sentence_lead,
    );
    let (lexical_findings, lexical_stats) = metrics::lexical_diversity_analysis(
        &tokenized,
        thresholds.lexical_ttr,
        thresholds.lexical_mtld,
    );
    let (specificity_findings, specificity_stats) =
        metrics::low_specificity_analysis(&masked, raw, morphology, thresholds.low_specificity)?;
    let paragraph_analysis = metrics::analyze_paragraphs(&masked, raw);

    let mut findings = structural_findings;
    findings.extend(patterns::local_pattern_findings(raw, morphology)?);
    findings.extend(patterns::forbidden_findings(&masked, raw));
    findings.extend(patterns::translationese_findings(&masked, raw));
    findings.extend(patterns::antithesis_findings(
        &masked,
        raw,
        split.len(),
        critical_above,
    ));
    findings.extend(patterns::english_syntax_findings(&masked, raw, &split));
    findings.extend(morph::translationese_morph_findings(&tokenized, &raw_lines));
    findings.extend(morph::inanimate_morph_findings(&tokenized, &raw_lines));
    findings.extend(morph::redundant_light_verb_findings(&tokenized, &raw_lines));
    findings.extend(rhythm_findings);
    findings.extend(ngram_findings);
    findings.extend(lexical_findings);
    findings.extend(specificity_findings);
    findings.extend(paragraph_analysis.findings);

    let sentence_lengths = split
        .iter()
        .map(|sentence| sentence.raw_text.chars().count() as f64)
        .collect::<Vec<_>>();
    if sentence_lengths.len() >= 5
        && let Some((mean, stdev)) = metrics::mean_and_stdev(&sentence_lengths)
        && mean > 0.0
        && stdev / mean < 0.25
    {
        let cv = stdev / mean;
        findings.push(Finding::new(
            split.first().map_or(1, |sentence| sentence.line),
            "low_sentence_variance",
            format!(
                "文数={}, 平均文長={mean:.1}字, 変動係数={cv:.3}",
                sentence_lengths.len()
            ),
            "warn",
            "文長の変動係数が閾値(0.25)未満。リズムが均質でAI臭い可能性",
        ));
    }
    if !experimental {
        findings.retain(|finding| !EXPERIMENTAL_CATEGORIES.contains(&finding.category.as_str()));
    }
    if genre == Some("business") {
        findings.retain(|finding| {
            !matches!(
                finding.category.as_str(),
                "high_bullet_ratio"
                    | "high_bold_density"
                    | "boilerplate_heading"
                    | "numbered_phase_structure"
                    | "bullet_bold_label"
                    | "bullet_emoji"
            )
        });
    }
    let total_chars = split
        .iter()
        .map(|sentence| sentence.text.chars().count())
        .sum::<usize>();
    let nominal_count = tokenized
        .iter()
        .filter(|sentence| morph::noun_ended(&sentence.tokens))
        .count();
    let nominal_ratio = if split.is_empty() {
        0.0
    } else {
        nominal_count as f64 / split.len() as f64
    };
    let nominal_min_chars = match genre {
        Some("essay") => 1500,
        Some("tech" | "business") => 3000,
        _ => 2000,
    };
    if split.len() >= 5
        && total_chars >= nominal_min_chars
        && nominal_ratio <= thresholds.nominal_ending_max_ratio
    {
        findings.push(Finding::new(
            split.last().map_or(1, |sentence| sentence.line),
            "nominal_ending",
            format!(
                "体言止め{nominal_count}件（全{}文、約{total_chars}字、比率{nominal_ratio:.3}）",
                split.len()
            ),
            "info",
            format!(
                "体言止めの比率が閾値{:.3}以下。ある程度の長さの文書でこの修辞技法がほぼ皆無なのはAI文章に特徴的。人間的な修辞の欠如の疑い",
                thresholds.nominal_ending_max_ratio
            ),
        ));
    }

    findings.sort_by_key(|finding| finding.line);
    let mut by_category = BTreeMap::new();
    for finding in &findings {
        *by_category.entry(finding.category.clone()).or_default() += 1;
    }

    let mut measurements = BTreeMap::new();
    measurements.insert("total_sentences".to_owned(), json!(split.len()));
    measurements.insert("nominal_ending_count".to_owned(), json!(nominal_count));
    measurements.insert("nominal_ending_ratio".to_owned(), json!(nominal_ratio));
    measurements.insert(
        "total_paragraphs".to_owned(),
        json!(paragraph_analysis.total),
    );
    measurements.insert(
        "paragraph_lead_conjunction_count".to_owned(),
        json!(paragraph_analysis.conjunction_count),
    );
    measurements.insert(
        "paragraph_lead_conjunction_ratio".to_owned(),
        json!(paragraph_analysis.conjunction_ratio),
    );
    measurements.insert(
        "paragraph_sentence_counts".to_owned(),
        json!(paragraph_analysis.sentence_counts),
    );
    measurements.insert(
        "paragraph_sentence_count_cv".to_owned(),
        json!(paragraph_analysis.sentence_count_cv),
    );
    measurements.insert(
        "masking".to_owned(),
        json!({
            "reference_lines": mask_stats.reference_lines,
            "code_annotation_lines": mask_stats.code_annotation_lines,
        }),
    );
    measurements.insert(
        "conjunction".to_owned(),
        metrics::conjunction_observations(&tokenized),
    );
    measurements.insert(
        "readability".to_owned(),
        metrics::readability_observations(&tokenized),
    );
    measurements.insert("rhythm".to_owned(), rhythm_stats);
    measurements.insert("ngram".to_owned(), ngram_stats);
    measurements.insert("lexical_diversity".to_owned(), lexical_stats);
    measurements.insert("structural".to_owned(), structural_stats);
    measurements.insert("low_specificity".to_owned(), specificity_stats);

    Ok(LintReport {
        stats: LintStats {
            total_findings: findings.len(),
            by_category,
            genre: genre.map(str::to_owned),
            experimental,
            measurements,
        },
        findings,
    })
}
