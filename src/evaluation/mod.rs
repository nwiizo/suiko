//! 開発用の評価CLI(suiko-eval)の実装。manifestの読み込みはmanifestへ分離し、
//! ここでは文書単位の校正プロキシ(report/sweep/length-analysis)と
//! 正解ラベル付きサンプル評価(labeled)を実装する。

mod manifest;

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;

use clap::ValueEnum;
use thiserror::Error;

use crate::lint::{self, AnalysisThresholds, ReadingLoadThresholds};
use crate::morphology::Morphology;

use manifest::{Corpus, Expectation, Genre, Label, Split, load_corpus};

/// これ未満の分母は率を性能値として扱わず、low_nを付けて参考値に落とす。
const MIN_SAMPLES: usize = 5;

#[derive(Debug, Error)]
pub enum EvaluationError {
    #[error("評価manifestを読み込めません: {path} ({source})")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("評価manifestを解析できません: {path} ({message})")]
    Parse { path: String, message: String },
    #[error("評価manifestが不正です: {0}")]
    Invalid(String),
    #[error("評価文書がUTF-8ではありません: {0}")]
    Utf8(String),
    #[error(transparent)]
    Analysis(#[from] crate::Error),
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum SweepRule {
    RepeatedSentenceLead,
    LowLexicalDiversityTtr,
    LowLexicalDiversityMtld,
    LowSpecificity,
    NominalEnding,
    SentenceTooLong,
}

#[derive(Clone, Copy, Default)]
struct SweepThresholds {
    analysis: AnalysisThresholds,
    reading_load: ReadingLoadThresholds,
}

impl SweepRule {
    fn category(self) -> &'static str {
        match self {
            Self::RepeatedSentenceLead => "repeated_sentence_lead",
            Self::LowLexicalDiversityTtr => "low_lexical_diversity_ttr",
            Self::LowLexicalDiversityMtld => "low_lexical_diversity_mtld",
            Self::LowSpecificity => "low_specificity",
            Self::NominalEnding => "nominal_ending",
            Self::SentenceTooLong => "sentence_too_long",
        }
    }

    fn lane(self) -> Lane {
        match self {
            Self::SentenceTooLong => Lane::ReadingLoad,
            _ => Lane::Naturalness,
        }
    }

    fn thresholds(self, value: f64) -> Result<SweepThresholds, EvaluationError> {
        if !value.is_finite() {
            return Err(EvaluationError::Invalid(
                "sweep値は有限の数で指定してください".to_owned(),
            ));
        }
        let mut thresholds = SweepThresholds::default();
        match self {
            Self::RepeatedSentenceLead => {
                if value < 1.0 || value.fract() != 0.0 || value > usize::MAX as f64 {
                    return Err(EvaluationError::Invalid(
                        "repeated-sentence-leadのsweep値は1以上の整数です".to_owned(),
                    ));
                }
                thresholds.analysis.repeated_sentence_lead = Some(value as usize);
            }
            Self::LowLexicalDiversityTtr => {
                if !(0.0..=1.0).contains(&value) {
                    return Err(EvaluationError::Invalid(
                        "low-lexical-diversity-ttrのsweep値は0以上1以下です".to_owned(),
                    ));
                }
                thresholds.analysis.lexical_ttr = value;
            }
            Self::LowLexicalDiversityMtld => {
                if value <= 0.0 {
                    return Err(EvaluationError::Invalid(
                        "low-lexical-diversity-mtldのsweep値は0より大きい数です".to_owned(),
                    ));
                }
                thresholds.analysis.lexical_mtld = value;
            }
            Self::LowSpecificity => {
                if !(-2.0..=2.0).contains(&value) {
                    return Err(EvaluationError::Invalid(
                        "low-specificityのsweep値は-2以上2以下です".to_owned(),
                    ));
                }
                thresholds.analysis.low_specificity = value;
            }
            Self::NominalEnding => {
                if !(0.0..1.0).contains(&value) {
                    return Err(EvaluationError::Invalid(
                        "nominal-endingのsweep値は0以上1未満の比率です".to_owned(),
                    ));
                }
                thresholds.analysis.nominal_ending_max_ratio = value;
            }
            Self::SentenceTooLong => {
                if value < 1.0 || value.fract() != 0.0 || value > usize::MAX as f64 {
                    return Err(EvaluationError::Invalid(
                        "sentence-too-longのsweep値は1以上の整数です".to_owned(),
                    ));
                }
                thresholds.reading_load.sentence_max = Some(value as usize);
            }
        }
        Ok(thresholds)
    }
}

struct DocumentReport {
    label: Label,
    genre: Genre,
    chars: usize,
    by_category: BTreeMap<String, usize>,
    reading_load_by_category: BTreeMap<String, usize>,
}

#[derive(Clone, Copy)]
enum Lane {
    Naturalness,
    ReadingLoad,
}

#[derive(Default)]
struct CategoryCounts {
    human_documents: usize,
    human_findings: usize,
    ai_documents: usize,
    ai_findings: usize,
}

fn evaluate(
    corpus: &Corpus,
    morphology: &Morphology,
    thresholds: SweepThresholds,
    experimental: bool,
    split: Option<Split>,
) -> Result<Vec<DocumentReport>, EvaluationError> {
    corpus
        .documents
        .iter()
        .filter(|document| split.is_none_or(|selected| document.split == selected))
        .map(|document| {
            let report = lint::analyze_with_thresholds(
                &document.text,
                morphology,
                Some(document.genre.as_str()),
                experimental,
                thresholds.analysis,
            )?;
            let reading_load = lint::analyze_reading_load_with_thresholds(
                &document.text,
                morphology,
                Some(document.genre.as_str()),
                thresholds.reading_load,
            )?;
            Ok(DocumentReport {
                label: document.label,
                genre: document.genre,
                chars: document.text.chars().count(),
                by_category: report.stats.by_category,
                reading_load_by_category: reading_load.stats.by_category,
            })
        })
        .collect()
}

fn label_totals(reports: &[DocumentReport]) -> (usize, usize) {
    let human = reports
        .iter()
        .filter(|report| report.label == Label::Human)
        .count();
    let ai = reports
        .iter()
        .filter(|report| report.label == Label::Ai)
        .count();
    (human, ai)
}

fn category_counts<'a>(
    reports: impl IntoIterator<Item = &'a DocumentReport>,
    lane: Lane,
) -> BTreeMap<String, CategoryCounts> {
    let reading_load_categories = lint::reading_load_categories();
    let selected_categories =
        lint::rule_categories()
            .iter()
            .copied()
            .filter(|category| match lane {
                Lane::Naturalness => !reading_load_categories.contains(category),
                Lane::ReadingLoad => reading_load_categories.contains(category),
            });
    let mut categories = selected_categories
        .map(|category| ((*category).to_owned(), CategoryCounts::default()))
        .collect::<BTreeMap<_, _>>();
    for report in reports {
        let by_category = match lane {
            Lane::Naturalness => &report.by_category,
            Lane::ReadingLoad => &report.reading_load_by_category,
        };
        for (category, findings) in by_category {
            let counts = categories.entry(category.clone()).or_default();
            match report.label {
                Label::Human => {
                    counts.human_documents += 1;
                    counts.human_findings += findings;
                }
                Label::Ai => {
                    counts.ai_documents += 1;
                    counts.ai_findings += findings;
                }
            }
        }
    }
    categories
}

fn rate(fired: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        fired as f64 / total as f64
    }
}

/// 二項比率のWilson 95%信頼区間。決定的で、少数標本でも[0,1]に収まる。
fn wilson_ci(successes: usize, total: usize) -> Option<(f64, f64)> {
    if total == 0 {
        return None;
    }
    let z = 1.96_f64;
    let n = total as f64;
    let p = successes as f64 / n;
    let z2 = z * z;
    let denominator = 1.0 + z2 / n;
    let center = (p + z2 / (2.0 * n)) / denominator;
    let half = (z / denominator) * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt();
    Some(((center - half).max(0.0), (center + half).min(1.0)))
}

/// 率と分母、Wilson 95%区間、低標本マーカーを1つの表示にまとめる。
fn rate_part(metric: &str, fired: usize, total: usize) -> String {
    let mut part = format!("{metric}={:.3}", rate(fired, total));
    if let Some((low, high)) = wilson_ci(fired, total) {
        part.push_str(&format!(" ci95={low:.3}-{high:.3}"));
    }
    if total < MIN_SAMPLES {
        part.push_str(" low_n");
    }
    part
}

fn split_counts<T>(items: &[T], split_of: impl Fn(&T) -> Split) -> (usize, usize) {
    let dev = items
        .iter()
        .filter(|item| split_of(item) == Split::Dev)
        .count();
    (dev, items.len() - dev)
}

/// 評価集合の版と統計の前提を1行で出力へ残す。
fn corpus_line(corpus: &Corpus) -> String {
    let (dev_documents, holdout_documents) =
        split_counts(&corpus.documents, |document| document.split);
    let (dev_samples, holdout_samples) = split_counts(&corpus.samples, |sample| sample.split);
    format!(
        "corpus: sha256={} documents=dev:{dev_documents}+holdout:{holdout_documents} samples=dev:{dev_samples}+holdout:{holdout_samples} ci=wilson95 low_n<{MIN_SAMPLES}\n",
        &corpus.manifest_sha256[..12],
    )
}

/// human/ai別の文書発火率とfinding件数を1行に整形する共通経路。
/// laneで指標名(fpr/detection、prevalence)が変わる。separatorは
/// 既存出力の互換のため呼び出し側の形式(タブまたは空白)を渡す。
fn counts_line(
    counts: &CategoryCounts,
    human_total: usize,
    ai_total: usize,
    lane: Lane,
    separator: char,
) -> String {
    let (human_metric, ai_metric) = match lane {
        Lane::Naturalness => ("fpr", "detection"),
        Lane::ReadingLoad => ("prevalence", "prevalence"),
    };
    format!(
        "human={}/{} {} findings={}{separator}ai={}/{} {} findings={}",
        counts.human_documents,
        human_total,
        rate_part(human_metric, counts.human_documents, human_total),
        counts.human_findings,
        counts.ai_documents,
        ai_total,
        rate_part(ai_metric, counts.ai_documents, ai_total),
        counts.ai_findings,
    )
}

fn push_category_lines(
    output: &mut String,
    prefix: &str,
    reports: &[&DocumentReport],
    human_total: usize,
    ai_total: usize,
    lane: Lane,
) {
    for (category, counts) in category_counts(reports.iter().copied(), lane) {
        output.push_str(&format!(
            "{prefix}{category}\t{}\n",
            counts_line(&counts, human_total, ai_total, lane, '\t')
        ));
    }
}

/// manifestを読み込み、既定閾値で全文書を評価する共通経路。
fn evaluate_with_defaults(
    manifest_path: &Path,
    experimental: bool,
) -> Result<(Corpus, Vec<DocumentReport>), EvaluationError> {
    let corpus = load_corpus(manifest_path)?;
    let morphology = Morphology::new()?;
    let reports = evaluate(
        &corpus,
        &morphology,
        SweepThresholds::default(),
        experimental,
        None,
    )?;
    Ok((corpus, reports))
}

pub fn report(manifest_path: &Path, experimental: bool) -> Result<String, EvaluationError> {
    let (corpus, reports) = evaluate_with_defaults(manifest_path, experimental)?;
    let (human_total, ai_total) = label_totals(&reports);
    let mut output = format!("documents: human={human_total} ai={ai_total}\n");
    output.push_str(&corpus_line(&corpus));
    output.push_str(
        "fpr and detection are document-level calibration proxies (resampling unit: document), not authorship probabilities\n",
    );
    let all = reports.iter().collect::<Vec<_>>();
    push_category_lines(
        &mut output,
        "",
        &all,
        human_total,
        ai_total,
        Lane::Naturalness,
    );
    push_category_lines(
        &mut output,
        "lane=reading_load category=",
        &all,
        human_total,
        ai_total,
        Lane::ReadingLoad,
    );

    let genres = reports
        .iter()
        .map(|report| report.genre)
        .collect::<BTreeSet<_>>();
    for genre in genres {
        let selected = reports
            .iter()
            .filter(|report| report.genre == genre)
            .collect::<Vec<_>>();
        let human = selected
            .iter()
            .filter(|report| report.label == Label::Human)
            .count();
        let ai = selected
            .iter()
            .filter(|report| report.label == Label::Ai)
            .count();
        output.push_str(&format!("genre={} human={human} ai={ai}\n", genre.as_str()));
        push_category_lines(
            &mut output,
            &format!("genre={} category=", genre.as_str()),
            &selected,
            human,
            ai,
            Lane::Naturalness,
        );
        push_category_lines(
            &mut output,
            &format!("genre={} lane=reading_load category=", genre.as_str()),
            &selected,
            human,
            ai,
            Lane::ReadingLoad,
        );
    }
    Ok(output)
}

pub fn sweep(
    manifest_path: &Path,
    rule: SweepRule,
    values: &[f64],
    experimental: bool,
) -> Result<String, EvaluationError> {
    if values.is_empty() {
        return Err(EvaluationError::Invalid(
            "sweep値を1件以上指定してください".to_owned(),
        ));
    }
    let corpus = load_corpus(manifest_path)?;
    let morphology = Morphology::new()?;
    let mut output = format!("rule: {}\n", rule.category());
    output.push_str(&corpus_line(&corpus));
    output.push_str("split=devのみで探索する。holdoutは閾値選定に使わない\n");
    for value in values {
        let reports = evaluate(
            &corpus,
            &morphology,
            rule.thresholds(*value)?,
            experimental,
            Some(Split::Dev),
        )?;
        let (human_total, ai_total) = label_totals(&reports);
        let counts = category_counts(&reports, rule.lane())
            .remove(rule.category())
            .unwrap_or_default();
        output.push_str(&format!(
            "value={value} {}\n",
            counts_line(&counts, human_total, ai_total, rule.lane(), ' ')
        ));
    }
    Ok(output)
}

pub fn labeled(manifest_path: &Path) -> Result<String, EvaluationError> {
    let corpus = load_corpus(manifest_path)?;
    if corpus.samples.is_empty() {
        return Err(EvaluationError::Invalid(
            "labeled評価にはsampleを1件以上定義してください".to_owned(),
        ));
    }
    let morphology = Morphology::new()?;
    let reading_load_categories = lint::reading_load_categories();

    struct SampleResult<'a> {
        sample: &'a manifest::Sample,
        findings: usize,
        fired: bool,
    }
    let mut results = Vec::with_capacity(corpus.samples.len());
    for sample in &corpus.samples {
        let genre = sample.genre.map(Genre::as_str);
        let by_category = if reading_load_categories.contains(&sample.category.as_str()) {
            lint::analyze_reading_load(&sample.text, &morphology, genre)?
                .stats
                .by_category
        } else {
            lint::analyze(&sample.text, &morphology, genre, true)?
                .stats
                .by_category
        };
        let findings = by_category.get(&sample.category).copied().unwrap_or(0);
        results.push(SampleResult {
            sample,
            findings,
            fired: findings > 0,
        });
    }

    #[derive(Default)]
    struct LabeledCounts {
        fire_total: usize,
        fire_hit: usize,
        silent_total: usize,
        silent_fired: usize,
    }
    let mut categories = BTreeMap::<&str, LabeledCounts>::new();
    for result in &results {
        let counts = categories
            .entry(result.sample.category.as_str())
            .or_default();
        match result.sample.expect {
            Expectation::Fire => {
                counts.fire_total += 1;
                if result.fired {
                    counts.fire_hit += 1;
                }
            }
            Expectation::Silent => {
                counts.silent_total += 1;
                if result.fired {
                    counts.silent_fired += 1;
                }
            }
        }
    }

    let mut output = format!(
        "samples: total={} categories={}\n",
        results.len(),
        categories.len()
    );
    output.push_str(&corpus_line(&corpus));
    output.push_str(
        "detection and fpr are rates on labeled fixtures (resampling unit: sample), not population estimates\n",
    );
    for (category, counts) in &categories {
        output.push_str(&format!(
            "category={category}\tfire={}/{} {}\tsilent_fired={}/{} {}\n",
            counts.fire_hit,
            counts.fire_total,
            rate_part("detection", counts.fire_hit, counts.fire_total),
            counts.silent_fired,
            counts.silent_total,
            rate_part("fpr", counts.silent_fired, counts.silent_total),
        ));
    }
    let mismatches = results
        .iter()
        .filter(|result| (result.sample.expect == Expectation::Fire) != result.fired)
        .collect::<Vec<_>>();
    output.push_str(&format!("mismatches: {}\n", mismatches.len()));
    for result in mismatches {
        let expect = match result.sample.expect {
            Expectation::Fire => "fire",
            Expectation::Silent => "silent",
        };
        let note = result
            .sample
            .note
            .as_deref()
            .map(|note| format!(" note={note}"))
            .unwrap_or_default();
        output.push_str(&format!(
            "mismatch id={} category={} expect={expect} findings={} path={}{note}\n",
            result.sample.id, result.sample.category, result.findings, result.sample.path,
        ));
    }
    Ok(output)
}

pub fn length_analysis(
    manifest_path: &Path,
    experimental: bool,
) -> Result<String, EvaluationError> {
    let (corpus, reports) = evaluate_with_defaults(manifest_path, experimental)?;
    let buckets = [
        ("<1000", 0, 1_000),
        ("1000-3999", 1_000, 4_000),
        (">=4000", 4_000, usize::MAX),
    ];
    let mut output = corpus_line(&corpus);
    for (name, lower, upper) in buckets {
        let selected = reports
            .iter()
            .filter(|report| report.chars >= lower && report.chars < upper)
            .collect::<Vec<_>>();
        let human = selected
            .iter()
            .filter(|report| report.label == Label::Human)
            .count();
        let ai = selected
            .iter()
            .filter(|report| report.label == Label::Ai)
            .count();
        let findings = selected
            .iter()
            .flat_map(|report| report.by_category.values())
            .sum::<usize>();
        let reading_load_findings = selected
            .iter()
            .flat_map(|report| report.reading_load_by_category.values())
            .sum::<usize>();
        output.push_str(&format!(
            "bucket={name} documents={} human={human} ai={ai} findings={findings} reading_load_findings={reading_load_findings}\n",
            selected.len()
        ));
        for lane in [Lane::Naturalness, Lane::ReadingLoad] {
            let lane_prefix = match lane {
                Lane::Naturalness => "",
                Lane::ReadingLoad => "lane=reading_load ",
            };
            for (category, counts) in category_counts(selected.iter().copied(), lane) {
                output.push_str(&format!(
                    "bucket={name} {lane_prefix}category={category} {}\n",
                    counts_line(&counts, human, ai, lane, ' ')
                ));
            }
        }
    }
    Ok(output)
}
