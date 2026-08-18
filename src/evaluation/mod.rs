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

use manifest::{
    Corpus, Expectation, ExternalStats, Genre, Label, Split, load_corpus, load_external_documents,
};

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
    documents: &[manifest::Document],
    morphology: &Morphology,
    thresholds: SweepThresholds,
    experimental: bool,
    split: Option<Split>,
) -> Result<Vec<DocumentReport>, EvaluationError> {
    documents
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

/// report --split の選択肢。holdoutは閾値確定後の一度きり評価に使う。
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum SplitFilter {
    Dev,
    Holdout,
}

impl SplitFilter {
    fn as_split(self) -> Split {
        match self {
            Self::Dev => Split::Dev,
            Self::Holdout => Split::Holdout,
        }
    }
}

/// manifestを読み込み、既定閾値で全文書を評価する共通経路。
fn evaluate_with_defaults(
    manifest_path: &Path,
    experimental: bool,
    split: Option<Split>,
) -> Result<(Corpus, Vec<DocumentReport>), EvaluationError> {
    let corpus = load_corpus(manifest_path)?;
    let morphology = Morphology::new()?;
    let reports = evaluate(
        &corpus.documents,
        &morphology,
        SweepThresholds::default(),
        experimental,
        split,
    )?;
    Ok((corpus, reports))
}

pub fn report(
    manifest_path: &Path,
    experimental: bool,
    split: Option<SplitFilter>,
    external: bool,
) -> Result<String, EvaluationError> {
    let corpus = load_corpus(manifest_path)?;
    let mut documents = corpus.documents;
    let mut external_stats = None;
    if external {
        let (external_documents, stats) = load_external_documents(manifest_path)?;
        external_stats = Some(stats);
        documents.extend(external_documents);
    }
    let morphology = Morphology::new()?;
    let reports = evaluate(
        &documents,
        &morphology,
        SweepThresholds::default(),
        experimental,
        split.map(SplitFilter::as_split),
    )?;
    let corpus = Corpus {
        manifest_sha256: corpus.manifest_sha256,
        documents,
        samples: corpus.samples,
    };
    let (human_total, ai_total) = label_totals(&reports);
    let mut output = format!("documents: human={human_total} ai={ai_total}\n");
    if let Some(stats) = &external_stats {
        output.push_str(&external_line(stats));
    }
    if let Some(selected) = split {
        output.push_str(&format!(
            "split={}のみを評価対象にしている\n",
            match selected {
                SplitFilter::Dev => "dev",
                SplitFilter::Holdout => "holdout",
            }
        ));
    }
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
            &corpus.documents,
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
    let (corpus, reports) = evaluate_with_defaults(manifest_path, experimental, None)?;
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

/// 外部取得文書の使用状況を1行で出力へ残す。
fn external_line(stats: &ExternalStats) -> String {
    format!(
        "external: dev={} holdout={} missing={} unfetched={} (本文非コミット、external-lock.jsonのSHA-256と一致したものだけ使用)\n",
        stats.used_dev, stats.used_holdout, stats.missing, stats.unfetched
    )
}

/// 閾値探索。人間文書のfpr(Wilson 95%上限)が制約以下の候補だけをfeasibleとし、
/// その中でAI検出率が最大の値を推奨する。dev splitのみを使い、holdoutには触れない。
#[allow(clippy::too_many_arguments)]
pub fn calibrate(
    manifest_path: &Path,
    rule: SweepRule,
    values: &[f64],
    max_human_fpr_upper: f64,
    external: bool,
    exclude_id_prefix: Option<&str>,
    experimental: bool,
) -> Result<String, EvaluationError> {
    if values.is_empty() {
        return Err(EvaluationError::Invalid(
            "calibrate値を1件以上指定してください".to_owned(),
        ));
    }
    if !(0.0..=1.0).contains(&max_human_fpr_upper) {
        return Err(EvaluationError::Invalid(
            "--max-human-fpr-upper は0以上1以下で指定してください".to_owned(),
        ));
    }
    let corpus = load_corpus(manifest_path)?;
    let mut documents = corpus.documents;
    let mut output = format!("rule: {}\n", rule.category());
    output.push_str(&format!(
        "corpus: sha256={} ci=wilson95 low_n<{MIN_SAMPLES}\n",
        &corpus.manifest_sha256[..12]
    ));
    if external {
        let (external_documents, stats) = load_external_documents(manifest_path)?;
        output.push_str(&external_line(&stats));
        documents.extend(external_documents);
    }
    if let Some(prefix) = exclude_id_prefix {
        let before = documents.len();
        documents.retain(|document| !document.id.starts_with(prefix));
        output.push_str(&format!(
            "excluded: id prefix \"{prefix}\" (n={})\n",
            before - documents.len()
        ));
    }
    output.push_str(&format!(
        "constraint: human fpr wilson95 upper <= {max_human_fpr_upper:.3} (split=devのみ、再標本化単位=document)\n"
    ));

    let morphology = Morphology::new()?;
    struct Candidate {
        value: f64,
        detection: f64,
        feasible: bool,
    }
    let mut candidates = Vec::with_capacity(values.len());
    let mut human_total_seen = 0usize;
    let mut ai_total_seen = 0usize;
    for value in values {
        let reports = evaluate(
            &documents,
            &morphology,
            rule.thresholds(*value)?,
            experimental,
            Some(Split::Dev),
        )?;
        let (human_total, ai_total) = label_totals(&reports);
        human_total_seen = human_total;
        ai_total_seen = ai_total;
        let counts = category_counts(&reports, rule.lane())
            .remove(rule.category())
            .unwrap_or_default();
        let upper = wilson_ci(counts.human_documents, human_total)
            .map(|(_, high)| high)
            .unwrap_or(1.0);
        let feasible = upper <= max_human_fpr_upper;
        candidates.push(Candidate {
            value: *value,
            detection: rate(counts.ai_documents, ai_total),
            feasible,
        });
        output.push_str(&format!(
            "value={value} {} feasible={}\n",
            counts_line(&counts, human_total, ai_total, rule.lane(), ' '),
            if feasible { "yes" } else { "no" }
        ));
    }

    if human_total_seen < MIN_SAMPLES || ai_total_seen < MIN_SAMPLES {
        output.push_str(&format!(
            "事前条件未達: 分母が{MIN_SAMPLES}未満の側があるため、この結果は性能値として使えない(annotation-guide.md)\n"
        ));
    }
    let best = candidates
        .iter()
        .filter(|candidate| candidate.feasible)
        .max_by(|a, b| {
            a.detection
                .partial_cmp(&b.detection)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    match best {
        Some(candidate) => output.push_str(&format!(
            "recommendation: value={} detection={:.3} (制約内でAI検出率最大。同率の場合は先に指定した値を採る)\n",
            candidate.value, candidate.detection
        )),
        None => output.push_str(
            "recommendation: なし(制約を満たす値がない。閾値変更よりも検出器の見直しを検討する)\n",
        ),
    }
    Ok(output)
}

/// 語彙の実測。FORBIDDEN_PHRASES / HYPE_EXPRESSIONS の各語句について、
/// dev split の人間/AI文書での出現を数え、削除候補と追加候補の判断材料を出す。
/// 率の対数比には0.5の平滑化を使い、区間は文書頻度に対するWilson 95%を示す。
pub fn vocab(
    manifest_path: &Path,
    external: bool,
    exclude_id_prefix: Option<&str>,
) -> Result<String, EvaluationError> {
    let corpus = load_corpus(manifest_path)?;
    let mut documents = corpus.documents;
    let mut output = format!(
        "corpus: sha256={} ci=wilson95 low_n<{MIN_SAMPLES}\n",
        &corpus.manifest_sha256[..12]
    );
    if external {
        let (external_documents, stats) = load_external_documents(manifest_path)?;
        output.push_str(&external_line(&stats));
        documents.extend(external_documents);
    }
    if let Some(prefix) = exclude_id_prefix {
        let before = documents.len();
        documents.retain(|document| !document.id.starts_with(prefix));
        output.push_str(&format!(
            "excluded: id prefix \"{prefix}\" (n={})\n",
            before - documents.len()
        ));
    }
    documents.retain(|document| document.split == Split::Dev);
    output.push_str(
        "split=devのみ。語彙の追加・削除はこの実測とannotation-guideの事前条件で判断する\n",
    );

    struct Side {
        chars: usize,
        documents: usize,
        masked: Vec<String>,
    }
    let mut human = Side {
        chars: 0,
        documents: 0,
        masked: Vec::new(),
    };
    let mut ai = Side {
        chars: 0,
        documents: 0,
        masked: Vec::new(),
    };
    for document in &documents {
        let masked = crate::text::mask_markdown_structure(&document.text);
        let side = match document.label {
            Label::Human => &mut human,
            Label::Ai => &mut ai,
        };
        side.chars += masked.chars().count();
        side.documents += 1;
        side.masked.push(masked);
    }
    output.push_str(&format!(
        "documents: human={} ({} chars masked) ai={} ({} chars masked)\n",
        human.documents, human.chars, ai.documents, ai.chars
    ));

    let per_100k = |occurrences: usize, chars: usize| -> f64 {
        if chars == 0 {
            0.0
        } else {
            occurrences as f64 * 100_000.0 / chars as f64
        }
    };
    // 平滑化付きの率の対数比。正がAI側に偏る語句。
    let log2_ratio = |ai_occurrences: usize, human_occurrences: usize| -> f64 {
        let ai_rate = (ai_occurrences as f64 + 0.5) / (ai.chars.max(1) as f64);
        let human_rate = (human_occurrences as f64 + 0.5) / (human.chars.max(1) as f64);
        (ai_rate / human_rate).log2()
    };

    for (list_name, phrases) in [
        ("forbidden_phrase", lint::forbidden_phrase_list()),
        ("hype_expression", lint::hype_expression_list()),
    ] {
        struct Row<'a> {
            phrase: &'a str,
            human_occurrences: usize,
            human_documents: usize,
            ai_occurrences: usize,
            ai_documents: usize,
            ratio: f64,
        }
        let mut rows = Vec::new();
        for phrase in phrases {
            let count_side = |side: &Side| -> (usize, usize) {
                let mut occurrences = 0;
                let mut with = 0;
                for masked in &side.masked {
                    let found = masked.matches(phrase).count();
                    occurrences += found;
                    if found > 0 {
                        with += 1;
                    }
                }
                (occurrences, with)
            };
            let (human_occurrences, human_documents) = count_side(&human);
            let (ai_occurrences, ai_documents) = count_side(&ai);
            rows.push(Row {
                phrase,
                human_occurrences,
                human_documents,
                ai_occurrences,
                ai_documents,
                ratio: log2_ratio(ai_occurrences, human_occurrences),
            });
        }
        rows.sort_by(|a, b| {
            b.ratio
                .partial_cmp(&a.ratio)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for row in rows {
            output.push_str(&format!(
                "list={list_name} phrase={} human_docs={}/{} {} human_per100k={:.2} ai_docs={}/{} {} ai_per100k={:.2} log2_ratio={:+.2}\n",
                row.phrase,
                row.human_documents,
                human.documents,
                rate_part("rate", row.human_documents, human.documents),
                per_100k(row.human_occurrences, human.chars),
                row.ai_documents,
                ai.documents,
                rate_part("rate", row.ai_documents, ai.documents),
                per_100k(row.ai_occurrences, ai.chars),
                row.ratio,
            ));
        }
    }

    // 追加候補: 内容語の辞書形をexcess vocabulary法(対数頻度比)で並べる。
    // 候補の提示であり自動採用はしない。固有名詞と数詞は主題由来のため除外する。
    let morphology = Morphology::new()?;
    let mut human_terms: BTreeMap<String, (usize, BTreeSet<usize>)> = BTreeMap::new();
    let mut ai_terms: BTreeMap<String, (usize, BTreeSet<usize>)> = BTreeMap::new();
    for (label, side, terms) in [
        (Label::Human, &human, &mut human_terms),
        (Label::Ai, &ai, &mut ai_terms),
    ] {
        let _ = label;
        for (index, masked) in side.masked.iter().enumerate() {
            for line in masked.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                for token in morphology.tokenize(line)? {
                    if !matches!(token.pos(0), "名詞" | "動詞" | "形容詞" | "副詞")
                        || matches!(token.pos(1), "固有名詞" | "数詞")
                    {
                        continue;
                    }
                    let form = token.dictionary_form();
                    if form.chars().count() < 2 {
                        continue;
                    }
                    let entry = terms.entry(form.to_owned()).or_default();
                    entry.0 += 1;
                    entry.1.insert(index);
                }
            }
        }
    }
    let known: BTreeSet<&str> = lint::forbidden_phrase_list()
        .iter()
        .chain(lint::hype_expression_list())
        .copied()
        .collect();
    let mut candidates = Vec::new();
    for (term, (ai_occurrences, ai_document_set)) in &ai_terms {
        if ai_document_set.len() < 3 || known.contains(term.as_str()) {
            continue;
        }
        let (human_occurrences, human_document_set) = human_terms
            .get(term)
            .map(|(occurrences, set)| (*occurrences, set.len()))
            .unwrap_or((0, 0));
        let ratio = log2_ratio(*ai_occurrences, human_occurrences);
        if ratio >= 2.0 {
            candidates.push((
                term.clone(),
                *ai_occurrences,
                ai_document_set.len(),
                human_occurrences,
                human_document_set,
                ratio,
            ));
        }
    }
    candidates.sort_by(|a, b| b.5.partial_cmp(&a.5).unwrap_or(std::cmp::Ordering::Equal));
    output.push_str("candidates: AI側に偏る内容語(辞書形、log2比>=2.0、AI文書3件以上)。判断材料であり自動採用しない\n");
    for (term, ai_occurrences, ai_document_count, human_occurrences, human_document_count, ratio) in
        candidates.into_iter().take(20)
    {
        output.push_str(&format!(
            "candidate term={term} ai_docs={ai_document_count}/{} ai_per100k={:.2} human_docs={human_document_count}/{} human_per100k={:.2} log2_ratio={:+.2}\n",
            ai.documents,
            per_100k(ai_occurrences, ai.chars),
            human.documents,
            per_100k(human_occurrences, human.chars),
            ratio,
        ));
    }
    Ok(output)
}
