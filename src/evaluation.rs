use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::lint::{self, AnalysisThresholds};
use crate::morphology::Morphology;

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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Label {
    Human,
    Ai,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "lowercase")]
enum Genre {
    Essay,
    Tech,
    Business,
}

impl Genre {
    fn as_str(self) -> &'static str {
        match self {
            Self::Essay => "essay",
            Self::Tech => "tech",
            Self::Business => "business",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CorpusManifest {
    version: u32,
    #[serde(default)]
    document: Vec<DocumentSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DocumentSpec {
    id: String,
    path: PathBuf,
    label: Label,
    genre: Genre,
    source: String,
    license: String,
    sha256: String,
}

struct Document {
    label: Label,
    genre: Genre,
    text: String,
}

struct Corpus {
    documents: Vec<Document>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum SweepRule {
    RepeatedSentenceLead,
    LowLexicalDiversityTtr,
    LowLexicalDiversityMtld,
}

impl SweepRule {
    fn category(self) -> &'static str {
        match self {
            Self::RepeatedSentenceLead => "repeated_sentence_lead",
            Self::LowLexicalDiversityTtr => "low_lexical_diversity_ttr",
            Self::LowLexicalDiversityMtld => "low_lexical_diversity_mtld",
        }
    }

    fn thresholds(self, value: f64) -> Result<AnalysisThresholds, EvaluationError> {
        if !value.is_finite() {
            return Err(EvaluationError::Invalid(
                "sweep値は有限の数で指定してください".to_owned(),
            ));
        }
        let mut thresholds = AnalysisThresholds::default();
        match self {
            Self::RepeatedSentenceLead => {
                if value < 1.0 || value.fract() != 0.0 || value > usize::MAX as f64 {
                    return Err(EvaluationError::Invalid(
                        "repeated-sentence-leadのsweep値は1以上の整数です".to_owned(),
                    ));
                }
                thresholds.repeated_sentence_lead = Some(value as usize);
            }
            Self::LowLexicalDiversityTtr => {
                if !(0.0..=1.0).contains(&value) {
                    return Err(EvaluationError::Invalid(
                        "low-lexical-diversity-ttrのsweep値は0以上1以下です".to_owned(),
                    ));
                }
                thresholds.lexical_ttr = value;
            }
            Self::LowLexicalDiversityMtld => {
                if value <= 0.0 {
                    return Err(EvaluationError::Invalid(
                        "low-lexical-diversity-mtldのsweep値は0より大きい数です".to_owned(),
                    ));
                }
                thresholds.lexical_mtld = value;
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

fn read(path: &Path) -> Result<Vec<u8>, EvaluationError> {
    fs::read(path).map_err(|source| EvaluationError::Read {
        path: path.display().to_string(),
        source,
    })
}

fn require_text(field: &str, value: &str, id: &str) -> Result<(), EvaluationError> {
    if value.trim().is_empty() {
        return Err(EvaluationError::Invalid(format!(
            "document {id} の {field} は空にできません"
        )));
    }
    Ok(())
}

fn load_corpus(manifest_path: &Path) -> Result<Corpus, EvaluationError> {
    let source = String::from_utf8(read(manifest_path)?)
        .map_err(|_| EvaluationError::Utf8(manifest_path.display().to_string()))?;
    let manifest =
        toml::from_str::<CorpusManifest>(&source).map_err(|error| EvaluationError::Parse {
            path: manifest_path.display().to_string(),
            message: error.to_string(),
        })?;
    if manifest.version != 1 {
        return Err(EvaluationError::Invalid(format!(
            "version = {} は未対応です。version = 1 を指定してください",
            manifest.version
        )));
    }
    if manifest.document.is_empty() {
        return Err(EvaluationError::Invalid(
            "documentを1件以上指定してください".to_owned(),
        ));
    }

    let base = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let mut ids = BTreeSet::new();
    let mut documents = Vec::with_capacity(manifest.document.len());
    for spec in manifest.document {
        require_text("id", &spec.id, &spec.id)?;
        require_text("source", &spec.source, &spec.id)?;
        require_text("license", &spec.license, &spec.id)?;
        if !ids.insert(spec.id.clone()) {
            return Err(EvaluationError::Invalid(format!(
                "document id が重複しています: {}",
                spec.id
            )));
        }
        if spec.sha256.len() != 64 || !spec.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(EvaluationError::Invalid(format!(
                "document {} のsha256は64桁の16進数で指定してください",
                spec.id
            )));
        }
        let path = base.join(&spec.path);
        let bytes = read(&path)?;
        let mut actual = String::with_capacity(64);
        for byte in Sha256::digest(&bytes) {
            write!(&mut actual, "{byte:02x}").expect("write to String");
        }
        if !actual.eq_ignore_ascii_case(&spec.sha256) {
            return Err(EvaluationError::Invalid(format!(
                "document {} のSHA-256が一致しません: expected={}, actual={actual}",
                spec.id, spec.sha256
            )));
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| EvaluationError::Utf8(path.display().to_string()))?;
        documents.push(Document {
            label: spec.label,
            genre: spec.genre,
            text,
        });
    }
    Ok(Corpus { documents })
}

fn evaluate(
    corpus: &Corpus,
    morphology: &Morphology,
    thresholds: AnalysisThresholds,
    experimental: bool,
) -> Result<Vec<DocumentReport>, EvaluationError> {
    corpus
        .documents
        .iter()
        .map(|document| {
            let report = lint::analyze_with_thresholds(
                &document.text,
                morphology,
                Some(document.genre.as_str()),
                experimental,
                thresholds,
            )?;
            let reading_load = lint::analyze_reading_load(
                &document.text,
                morphology,
                Some(document.genre.as_str()),
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

fn push_category_lines(
    output: &mut String,
    prefix: &str,
    reports: &[&DocumentReport],
    human_total: usize,
    ai_total: usize,
    lane: Lane,
) {
    for (category, counts) in category_counts(reports.iter().copied(), lane) {
        match lane {
            Lane::Naturalness => output.push_str(&format!(
                "{prefix}{category}\thuman={}/{} fpr={:.3} findings={}\tai={}/{} detection={:.3} findings={}\n",
                counts.human_documents,
                human_total,
                rate(counts.human_documents, human_total),
                counts.human_findings,
                counts.ai_documents,
                ai_total,
                rate(counts.ai_documents, ai_total),
                counts.ai_findings,
            )),
            Lane::ReadingLoad => output.push_str(&format!(
                "{prefix}{category}\thuman={}/{} prevalence={:.3} findings={}\tai={}/{} prevalence={:.3} findings={}\n",
                counts.human_documents,
                human_total,
                rate(counts.human_documents, human_total),
                counts.human_findings,
                counts.ai_documents,
                ai_total,
                rate(counts.ai_documents, ai_total),
                counts.ai_findings,
            )),
        }
    }
}

pub fn report(manifest_path: &Path, experimental: bool) -> Result<String, EvaluationError> {
    let corpus = load_corpus(manifest_path)?;
    let morphology = Morphology::new()?;
    let reports = evaluate(
        &corpus,
        &morphology,
        AnalysisThresholds::default(),
        experimental,
    )?;
    let (human_total, ai_total) = label_totals(&reports);
    let mut output = format!("documents: human={human_total} ai={ai_total}\n");
    output.push_str(
        "fpr and detection are document-level calibration proxies, not authorship probabilities\n",
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
    for value in values {
        let reports = evaluate(&corpus, &morphology, rule.thresholds(*value)?, experimental)?;
        let (human_total, ai_total) = label_totals(&reports);
        let counts = category_counts(&reports, Lane::Naturalness)
            .remove(rule.category())
            .unwrap_or_default();
        output.push_str(&format!(
            "value={value} human={}/{} fpr={:.3} findings={} ai={}/{} detection={:.3} findings={}\n",
            counts.human_documents,
            human_total,
            rate(counts.human_documents, human_total),
            counts.human_findings,
            counts.ai_documents,
            ai_total,
            rate(counts.ai_documents, ai_total),
            counts.ai_findings,
        ));
    }
    Ok(output)
}

pub fn length_analysis(
    manifest_path: &Path,
    experimental: bool,
) -> Result<String, EvaluationError> {
    let corpus = load_corpus(manifest_path)?;
    let morphology = Morphology::new()?;
    let reports = evaluate(
        &corpus,
        &morphology,
        AnalysisThresholds::default(),
        experimental,
    )?;
    let buckets = [
        ("<1000", 0, 1_000),
        ("1000-3999", 1_000, 4_000),
        (">=4000", 4_000, usize::MAX),
    ];
    let mut output = String::new();
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
        for (category, counts) in category_counts(selected.iter().copied(), Lane::Naturalness) {
            output.push_str(&format!(
                "bucket={name} category={category} human={}/{} fpr={:.3} findings={} ai={}/{} detection={:.3} findings={}\n",
                counts.human_documents,
                human,
                rate(counts.human_documents, human),
                counts.human_findings,
                counts.ai_documents,
                ai,
                rate(counts.ai_documents, ai),
                counts.ai_findings,
            ));
        }
        for (category, counts) in category_counts(selected.iter().copied(), Lane::ReadingLoad) {
            output.push_str(&format!(
                "bucket={name} lane=reading_load category={category} human={}/{} prevalence={:.3} findings={} ai={}/{} prevalence={:.3} findings={}\n",
                counts.human_documents,
                human,
                rate(counts.human_documents, human),
                counts.human_findings,
                counts.ai_documents,
                ai,
                rate(counts.ai_documents, ai),
                counts.ai_findings,
            ));
        }
    }
    Ok(output)
}
