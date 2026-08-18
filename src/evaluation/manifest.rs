//! 評価manifest(corpus.toml)の読み込みと検証。
//! document(出典・SHA-256付きの文書)とsample(正解ラベル付きfixture)を扱う。

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::EvaluationError;
use crate::lint;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(super) enum Label {
    Human,
    Ai,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "lowercase")]
pub(super) enum Genre {
    Essay,
    Tech,
    Business,
}

impl Genre {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Essay => "essay",
            Self::Tech => "tech",
            Self::Business => "business",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(super) enum Expectation {
    Fire,
    Silent,
}

/// 評価集合の役割。devは閾値調整に使い、holdoutは閾値探索(sweep)から
/// 除外して最終確認だけに使う。
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(super) enum Split {
    #[default]
    Dev,
    Holdout,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CorpusManifest {
    version: u32,
    #[serde(default)]
    document: Vec<DocumentSpec>,
    #[serde(default)]
    sample: Vec<SampleSpec>,
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
    #[serde(default)]
    split: Split,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SampleSpec {
    id: String,
    path: PathBuf,
    category: String,
    expect: Expectation,
    genre: Option<Genre>,
    note: Option<String>,
    #[serde(default)]
    split: Split,
}

pub(super) struct Document {
    pub(super) id: String,
    pub(super) label: Label,
    pub(super) genre: Genre,
    pub(super) split: Split,
    pub(super) text: String,
}

pub(super) struct Sample {
    pub(super) id: String,
    pub(super) path: String,
    pub(super) category: String,
    pub(super) expect: Expectation,
    pub(super) genre: Option<Genre>,
    pub(super) note: Option<String>,
    pub(super) split: Split,
    pub(super) text: String,
}

pub(super) struct Corpus {
    /// manifest本文のSHA-256。評価出力に「評価集合の版」として併記する。
    pub(super) manifest_sha256: String,
    pub(super) documents: Vec<Document>,
    pub(super) samples: Vec<Sample>,
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

fn digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(&mut output, "{byte:02x}").expect("write to String");
    }
    output
}

pub(super) fn load_corpus(manifest_path: &Path) -> Result<Corpus, EvaluationError> {
    let manifest_bytes = read(manifest_path)?;
    let manifest_sha256 = digest(&manifest_bytes);
    let source = String::from_utf8(manifest_bytes)
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
        let actual = digest(&bytes);
        if !actual.eq_ignore_ascii_case(&spec.sha256) {
            return Err(EvaluationError::Invalid(format!(
                "document {} のSHA-256が一致しません: expected={}, actual={actual}",
                spec.id, spec.sha256
            )));
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| EvaluationError::Utf8(path.display().to_string()))?;
        documents.push(Document {
            id: spec.id,
            label: spec.label,
            genre: spec.genre,
            split: spec.split,
            text,
        });
    }

    let mut sample_ids = BTreeSet::new();
    let mut samples = Vec::with_capacity(manifest.sample.len());
    for spec in manifest.sample {
        require_text("id", &spec.id, &spec.id)?;
        require_text("category", &spec.category, &spec.id)?;
        if !sample_ids.insert(spec.id.clone()) {
            return Err(EvaluationError::Invalid(format!(
                "sample id が重複しています: {}",
                spec.id
            )));
        }
        if !lint::is_known_rule(&spec.category) {
            return Err(EvaluationError::Invalid(format!(
                "sample {} のcategoryが未知のルールです: {}",
                spec.id, spec.category
            )));
        }
        let path = base.join(&spec.path);
        let bytes = read(&path)?;
        let text = String::from_utf8(bytes)
            .map_err(|_| EvaluationError::Utf8(path.display().to_string()))?;
        samples.push(Sample {
            id: spec.id,
            path: spec.path.display().to_string(),
            category: spec.category,
            expect: spec.expect,
            genre: spec.genre,
            note: spec.note,
            split: spec.split,
            text,
        });
    }
    Ok(Corpus {
        manifest_sha256,
        documents,
        samples,
    })
}

/// sources.toml のうち外部取得(type=web)エントリの読み込みに必要な項目。
/// 出典系の追加フィールドは取得スクリプト用のメタデータなのでここでは無視する。
#[derive(Debug, Deserialize)]
struct SourceSpec {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    genre: Genre,
    #[serde(default)]
    split: Split,
}

#[derive(Debug, Deserialize)]
struct SourcesManifest {
    version: u32,
    #[serde(default)]
    source: Vec<SourceSpec>,
}

#[derive(Debug, Deserialize)]
struct LockEntry {
    #[serde(default)]
    sha256: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExternalLock {
    entries: std::collections::BTreeMap<String, LockEntry>,
}

/// 外部取得文書の読み込み結果。欠落は黙って無視せず件数を記録する。
#[derive(Debug, Default)]
pub(super) struct ExternalStats {
    pub(super) used_dev: usize,
    pub(super) used_holdout: usize,
    pub(super) missing: usize,
    pub(super) unfetched: usize,
}

/// eval/sources.toml と external-lock.json から、ローカルに取得済みの
/// 外部人間文書を読み込む。本文は非コミットのため、存在するファイルは
/// lockのSHA-256と一致する場合だけ使い、不一致は取得版のずれとして
/// エラーにする(再取得で解消する)。欠落はskipとして数える。
pub(super) fn load_external_documents(
    manifest_path: &Path,
) -> Result<(Vec<Document>, ExternalStats), EvaluationError> {
    let base = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let sources_path = base.join("sources.toml");
    let lock_path = base.join("corpus/external-lock.json");

    let sources_text = String::from_utf8(read(&sources_path)?)
        .map_err(|_| EvaluationError::Utf8(sources_path.display().to_string()))?;
    let sources = toml::from_str::<SourcesManifest>(&sources_text).map_err(|error| {
        EvaluationError::Parse {
            path: sources_path.display().to_string(),
            message: error.to_string(),
        }
    })?;
    if sources.version != 1 {
        return Err(EvaluationError::Invalid(format!(
            "sources.toml の version = {} は未対応です",
            sources.version
        )));
    }
    let lock_text = String::from_utf8(read(&lock_path)?)
        .map_err(|_| EvaluationError::Utf8(lock_path.display().to_string()))?;
    let lock = serde_json::from_str::<ExternalLock>(&lock_text).map_err(|error| {
        EvaluationError::Parse {
            path: lock_path.display().to_string(),
            message: error.to_string(),
        }
    })?;

    let mut documents = Vec::new();
    let mut stats = ExternalStats::default();
    for spec in sources.source.iter().filter(|spec| spec.kind == "web") {
        let expected = match lock.entries.get(&spec.id).and_then(|e| e.sha256.as_ref()) {
            Some(sha256) => sha256,
            None => {
                stats.unfetched += 1;
                continue;
            }
        };
        let path = base.join("corpus/external").join(format!("{}.md", spec.id));
        if !path.exists() {
            stats.missing += 1;
            continue;
        }
        let bytes = read(&path)?;
        let actual = digest(&bytes);
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(EvaluationError::Invalid(format!(
                "外部文書 {} のSHA-256がexternal-lock.jsonと一致しません: expected={expected}, actual={actual}。scripts/fetch-corpus.py --id {} で再取得してください",
                spec.id, spec.id
            )));
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| EvaluationError::Utf8(path.display().to_string()))?;
        match spec.split {
            Split::Dev => stats.used_dev += 1,
            Split::Holdout => stats.used_holdout += 1,
        }
        documents.push(Document {
            id: spec.id.clone(),
            label: Label::Human,
            genre: spec.genre,
            split: spec.split,
            text,
        });
    }
    Ok((documents, stats))
}
