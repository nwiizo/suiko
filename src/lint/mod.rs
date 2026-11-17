//! 自然さ・構造・読解負荷の検出器。公開APIはこのモジュールに集約し、
//! 検出器の実装はpatterns(表層)、morph(品詞列)、metrics(統計)、
//! reading_load(読解負荷レーン)、baseline(前回比較)へ分割している。

mod abstract_metaphor;
mod agency;
mod baseline;
mod clauses;
mod commas;
mod context;
mod contrast;
mod exceptions;
mod genre;
mod hollow;
mod item_count;
pub mod matching;
mod metrics;
mod morph;
mod patterns;
mod phrasing;
mod priority;
mod quantities;
mod reading_load;
mod registry;
mod relations;
mod repetition;
mod revision;
mod revision_lexicon;
mod sensory_roles;
mod sensory_terms;
mod sentence_endings;
mod similes;
mod technical_jargon;
mod word_rules;

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Value, json};

use crate::Error;
use crate::morphology::Morphology;
use crate::text::{mask_html_comments, mask_markdown_structure_with_stats, sentences_with_raw};

pub use baseline::{BaselineReport, BaselineSummary, apply_baseline, baseline_added};
pub(crate) use clauses::FORMAL_NOUNS;
pub use patterns::{forbidden_phrase_list, hype_expression_list};
pub use priority::{Confidence, FIRST_FIX_LIMIT, FirstFix, confidence, first_fixes};
pub use reading_load::{analyze_reading_load, analyze_reading_load_with_thresholds};
pub use registry::{Family, Rule, Scope, rule_of};
pub(crate) use word_rules::{WordRule, validate_word_rules, word_rule_findings};

pub fn is_known_rule(category: &str) -> bool {
    registry::exact(category).is_some()
}

pub fn rule_categories() -> &'static [&'static str] {
    registry::categories()
}

pub fn is_experimental_rule(category: &str) -> bool {
    registry::exact(category).is_some_and(|rule| rule.evaluating)
}

pub fn reading_load_categories() -> &'static [&'static str] {
    registry::reading_load()
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

impl Span {
    /// 同じ行で始まり、行内のbyte範囲が重なるか。
    fn overlaps(self, other: Span) -> bool {
        self.start_line == other.start_line
            && self.start_byte < other.end_byte
            && other.start_byte < self.end_byte
    }
}

/// `category`の指摘が指す範囲。
fn spans_of(findings: &[Finding], category: &str) -> Vec<Span> {
    findings
        .iter()
        .filter(|finding| finding.category == category)
        .filter_map(|finding| finding.span)
        .collect()
}

/// 指摘の範囲が`spans`のどれかと重なるか。一つの箇所に一つの指摘を残すために使う。
fn overlaps_any(finding: &Finding, spans: &[Span]) -> bool {
    finding
        .span
        .is_some_and(|span| spans.iter().any(|other| span.overlaps(*other)))
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
    /// 文脈の確認を要する言い換え。自動適用可能なsuggestionとは区別する。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_suggestion: Option<Suggestion>,
}

impl Finding {
    pub(crate) fn new(
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
            review_suggestion: None,
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

fn span_contains(outer: Span, inner: Span) -> bool {
    (outer.start_line, outer.start_byte) <= (inner.start_line, inner.start_byte)
        && (outer.end_line, outer.end_byte) >= (inner.end_line, inner.end_byte)
}

fn suppress_surface_inanimate_duplicates(findings: &mut Vec<Finding>) {
    let morph_spans = findings
        .iter()
        .filter(|finding| finding.category == "inanimate_subject_morph")
        .filter_map(|finding| finding.span)
        .collect::<Vec<_>>();
    findings.retain(|finding| {
        if finding.category != "english_syntax_inanimate_subject" {
            return true;
        }
        finding.span.is_none_or(|surface_span| {
            !morph_spans.iter().any(|morph_span| {
                span_contains(surface_span, *morph_span) || span_contains(*morph_span, surface_span)
            })
        })
    });
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
    /// `long_attributive_span`が指さす、被修飾名詞に前置された修飾節の最小字数。
    pub attributive_span_min: Option<usize>,
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
    let (prepared, review_mask_stats) = crate::text::prepare_lint_text(raw);
    let raw = prepared.as_str();
    let (masked, mask_stats) = mask_markdown_structure_with_stats(raw);
    let raw_lines = raw.split('\n').collect::<Vec<_>>();
    let mut split = sentences_with_raw(&crate::markup::mask_review_emphasis(&masked), raw);
    // 文の端の区切りだけを外し、文中は既存の形態素規則が扱う表記を保つ。
    // 空白へ変えると、隣接を確認する規則がコード除外の空白と混同する。
    let masked_lines = masked.split('\n').collect::<Vec<_>>();
    for sentence in &mut split {
        sentence.text = masked_lines[sentence.line - 1]
            [sentence.line_byte_start..sentence.line_byte_start + sentence.text.len()]
            .to_owned();
    }
    let tokenized = morph::tokenize(&split, morphology)?;
    let requested_genre = genre;
    let genre_features = genre::features(&tokenized, raw);
    let genre = genre::resolve(genre, &genre_features);
    let critical_above = if genre == Some("tech") { 0.045 } else { 0.03 };

    let (structural_findings, structural_stats) =
        patterns::structural_analysis(&mask_html_comments(raw));
    let (ending_findings, ending_stats) = sentence_endings::analysis(&tokenized, &raw_lines);
    let (rhythm_findings, rhythm_stats) =
        metrics::rhythm_analysis(&tokenized, ending_findings, ending_stats);
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
    if genre == Some("tech") {
        findings.extend(context::findings(&tokenized, &raw_lines, experimental));
    }
    for line in &review_mask_stats.invalid_directives {
        findings.push(Finding::new(*line, "invalid_ignore_directive", raw_lines[*line - 1], "warn", "除外範囲の開始・終了が対応していないか、入れ子になっています。この範囲の本文は検査対象に残しています。"));
    }
    findings.extend(patterns::local_pattern_findings(raw, morphology)?);
    if experimental {
        findings.extend(patterns::mixed_latin_spacing_findings(&masked, &raw_lines));
    }
    findings.extend(quantities::quoted_character_count_findings(
        &masked, &raw_lines,
    ));
    findings.extend(patterns::boilerplate_closing_findings(&split, &raw_lines));
    findings.extend(item_count::declared_item_count_findings(
        &mask_html_comments(raw),
        &raw_lines,
    ));
    // 箇条書きの並行性は技術・業務文書では理解を助けるため、散文を明示した場合だけ観測する。
    if experimental && genre == Some("essay") {
        findings.extend(patterns::uniform_bullet_structure_findings(
            raw, morphology,
        )?);
    }
    findings.extend(patterns::bold_not_rendered_findings(raw, &raw_lines));
    findings.extend(patterns::technical_surface_findings(raw, genre));
    findings.extend(patterns::duplicate_comma_findings(&masked, &raw_lines));
    findings.extend(patterns::instruction_residue_findings(&split, &raw_lines));
    findings.extend(patterns::forbidden_findings(&masked, raw));
    findings.extend(patterns::translationese_findings(&masked, raw));
    findings.extend(patterns::antithesis_findings(
        &masked,
        raw,
        &tokenized,
        split.len(),
        critical_above,
    ));
    findings.extend(contrast::negated_contrast_findings(&tokenized, &raw_lines));
    findings.extend(patterns::english_syntax_findings(&masked, raw, &split));
    findings.extend(phrasing::translationese_morph_findings(
        &tokenized, &raw_lines,
    ));
    let experiential = spans_of(&findings, "evaluative_ability_past");
    findings.retain(|finding| {
        !(finding.category == "evaluative_ability_past" && genre == Some("essay")
            || finding.category == "translationese" && overlaps_any(finding, &experiential))
    });
    findings.extend(repetition::self_labeling_repetition_findings(
        &tokenized, &raw_lines,
    ));
    findings.extend(repetition::negative_listing_findings(
        &tokenized, &raw_lines,
    ));
    findings.extend(commas::short_phrase_comma_findings(&tokenized, &raw_lines));
    findings.extend(phrasing::decision_direction_findings(
        &tokenized, &raw_lines,
    ));
    findings.extend(sensory_terms::vague_sensory_findings(
        &tokenized, &raw_lines,
    ));
    findings.extend(phrasing::copy_fragment_findings(&tokenized, &raw_lines));
    findings.extend(sensory_roles::findings(&tokenized, &raw_lines));
    findings.extend(exceptions::findings(&tokenized, &raw_lines));
    findings.extend(agency::findings(&tokenized, &raw_lines));
    findings.extend(similes::findings(&tokenized, &raw_lines));
    findings.extend(hollow::sentence_findings(&tokenized, &raw_lines, genre));
    findings.extend(hollow::heading_findings(raw));
    if genre == Some("essay") {
        findings.extend(hollow::essay_findings(raw));
    }
    findings.extend(quantities::quantity_predicate_findings(
        &tokenized, &raw_lines,
    ));
    if genre == Some("tech") {
        findings.extend(repetition::technical_repetition_findings(
            &tokenized, &raw_lines, raw,
        ));
    }
    findings.extend(repetition::explanation_preview_findings(&tokenized, raw));
    findings.extend(phrasing::technical_ambiguity_findings(
        &tokenized, &raw_lines,
    ));
    findings.extend(technical_jargon::technical_jargon_metaphor_findings(
        &tokenized, &raw_lines,
    ));
    findings.extend(abstract_metaphor::abstract_predicate_metaphor_findings(
        &tokenized, &raw_lines,
    ));
    findings.extend(phrasing::inanimate_morph_findings(&tokenized, &raw_lines));
    let mut revision_findings = if genre == Some("tech") {
        let units = crate::text::revision_sentences(raw);
        let technical = morph::tokenize(&units, morphology)?;
        for candidate in commas::short_phrase_comma_findings(&technical, &raw_lines) {
            if !findings.iter().any(|existing| {
                existing.category == candidate.category
                    && existing
                        .span
                        .zip(candidate.span)
                        .is_some_and(|(left, right)| {
                            left.start_line == right.start_line
                                && left.start_byte == right.start_byte
                        })
            }) {
                findings.push(candidate);
            }
        }
        revision::findings(&technical, &raw_lines, true)
    } else {
        revision::findings(&tokenized, &raw_lines, false)
    };
    // 既存の形態素検査が同じ比喩を指す場合は、より限定された従来のカテゴリを保つ。
    let technical_metaphors = spans_of(&findings, "technical_jargon_metaphor");
    revision_findings.retain(|candidate| {
        candidate.category != "explanatory_metaphor"
            || !overlaps_any(candidate, &technical_metaphors)
    });
    findings.extend(revision_findings);
    findings.extend(abstract_metaphor::abstract_metaphor_findings(
        &tokenized, &raw_lines, true,
    ));
    // 「入口」を技術の言い回しとして指した箇所は、そちらだけを残す（一つの箇所に一つの指摘）。
    let technical_metaphors = spans_of(&findings, "technical_jargon_metaphor");
    findings.retain(|finding| {
        finding.category != "abstract_metaphor" || !overlaps_any(finding, &technical_metaphors)
    });
    findings.extend(phrasing::redundant_light_verb_findings(
        &tokenized, &raw_lines,
    ));
    findings.extend(rhythm_findings);
    findings.extend(ngram_findings);
    let preview_starts = findings
        .iter()
        .filter(|finding| finding.category == "repeated_explanation_preview")
        .flat_map(|finding| finding.related_lines.iter().flatten().copied())
        .map(|line| {
            (
                line,
                raw_lines[line - 1].len() - raw_lines[line - 1].trim_start().len(),
            )
        })
        .collect::<BTreeSet<_>>();
    // 同じ文頭に対する品詞4-gramより、述語まで見た具体的な指摘を残す。
    findings.retain(|finding| {
        finding.category != "repeated_syntax_template"
            || !finding
                .span
                .is_some_and(|span| preview_starts.contains(&(span.start_line, span.start_byte)))
    });
    findings.extend(lexical_findings);
    findings.extend(specificity_findings);
    findings.extend(paragraph_analysis.findings);
    suppress_surface_inanimate_duplicates(&mut findings);

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
            "文の長さがそろっています（変動係数0.25未満）。通読して区切りが不自然な箇所だけを見直してください。手順や並列の説明は、長さを変えるためだけに書き換える必要はありません。",
        ));
    }
    if !experimental {
        findings.retain(|finding| !is_experimental_rule(&finding.category));
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
                "体言止めの比率が閾値{:.3}以下です。文末の単調さを読み直す手掛かりですが、説明・依頼・条件を保つために述語が必要なら、そのまま残してください。体言止めを増やす指示ではありません。",
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
            "ignored_regions": review_mask_stats.ignored_regions,
            "ignored_lines": review_mask_stats.ignored_lines,
            "hatena_spans": review_mask_stats.hatena_spans,
            "invalid_ignore_directives": review_mask_stats.invalid_directives.len(),
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
    measurements.insert(
        "genre_inference".to_owned(),
        genre::observation(&genre_features, genre),
    );

    Ok(LintReport {
        stats: LintStats {
            total_findings: findings.len(),
            by_category,
            genre: requested_genre.map(str::to_owned),
            experimental,
            measurements,
        },
        findings,
    })
}
