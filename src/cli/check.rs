//! 検査結果の保存と、検査後に対象が変わっていないことの確認。
//! 記録は意味の正しさや人による承認を表さない。

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{
    Config, FailOn, Genre, apply_reading_load_config, config_path, configured_lint, parse_config,
};
use crate::{Error, diff, fact_gaps, lint, materials, morphology::Morphology, read_source};

pub(super) const RECORD_FORMAT: &str = "suiko-check";
pub(super) const RECORD_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Args)]
pub(super) struct CheckArgs {
    /// 検査する最終稿。再確認できるファイルを指定する
    file: PathBuf,
    /// 改稿前の原文。省略時は差分検査を行わない
    #[arg(long)]
    before: Option<PathBuf>,
    /// 提供資料。複数指定可。数値・条件・未確認事項を最終稿と照合する
    #[arg(long)]
    material: Vec<PathBuf>,
    /// 検査記録の保存先（既定: <最終稿>.suiko-check.json）
    #[arg(long)]
    record: Option<PathBuf>,
    #[arg(long, value_enum)]
    genre: Option<Genre>,
    #[arg(long)]
    experimental: bool,
    /// 比喩・効果・程度の語だけで中身や値が見えない文を、書き手に確かめる候補として示す。--fail-onには数えない
    #[arg(long)]
    fact_gaps: bool,
    /// 指定severity以上の候補があれば、記録を保存して終了コード2を返す
    #[arg(long, value_enum)]
    fail_on: Option<FailOn>,
    #[arg(long, conflicts_with = "no_config")]
    config: Option<PathBuf>,
    #[arg(long, conflicts_with = "config")]
    no_config: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
pub(super) struct VerifyArgs {
    /// checkで保存したJSON記録
    record: PathBuf,
    /// 記録の設定で検査も再実行し、保存した結果と照合する
    #[arg(long)]
    replay: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InputFile {
    path: PathBuf,
    sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Configuration {
    path: PathBuf,
    /// 自動検出先が存在しなかった場合も、あとで作成されていないか調べる。
    sha256: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    genre: Option<Genre>,
    experimental: bool,
    fail_on: Option<FailOn>,
    /// 追加前の記録にはないため、読み込み時は false とみなす。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    fact_gaps: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    suiko_version: String,
    tool_sha256: String,
    checked_at_unix_seconds: u64,
    final_file: InputFile,
    before: Option<InputFile>,
    materials: Vec<InputFile>,
    configuration: Option<Configuration>,
    options: Options,
    threshold_exceeded: bool,
    report: Value,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredRecord {
    format: String,
    schema_version: u32,
    record: Record,
    /// 誤編集や途中書き込みを検出する。署名や改ざん耐性を提供するものではない。
    checksum: String,
}

struct LoadedFile {
    input: InputFile,
    text: String,
}

#[derive(Serialize)]
struct CheckReport {
    lint: lint::LintReport,
    reading_load: lint::ReadingLoadReport,
    diff: Option<diff::DiffReport>,
    materials: Option<materials::MaterialReport>,
    /// 原文の指摘のうち、改稿で消えたもの・残ったもの・新しく出たもの。
    #[serde(skip_serializing_if = "Option::is_none")]
    finding_changes: Option<FindingChanges>,
    /// 書き手への確認候補。--fail-onの判定には含めない。
    #[serde(skip_serializing_if = "Option::is_none")]
    fact_gaps: Option<fact_gaps::FactGapReport>,
}

#[derive(Serialize)]
struct FindingKey {
    category: String,
    line: usize,
    excerpt: String,
}

/// 指摘を（カテゴリ, 抜粋）で突き合わせる。行番号は改稿で動くため照合に使わない。
#[derive(Serialize)]
struct FindingChanges {
    resolved: Vec<FindingKey>,
    kept: Vec<FindingKey>,
    introduced: Vec<FindingKey>,
}

impl FindingChanges {
    fn compare<'a>(
        before: impl Iterator<Item = &'a lint::Finding>,
        after: impl Iterator<Item = &'a lint::Finding>,
    ) -> Self {
        let key = |finding: &lint::Finding| FindingKey {
            category: finding.category.clone(),
            line: finding.line,
            excerpt: finding.excerpt.clone(),
        };
        let mut remaining = before.map(key).collect::<Vec<_>>();
        let mut kept = Vec::new();
        let mut introduced = Vec::new();
        for finding in after.map(key) {
            match remaining
                .iter()
                .position(|old| old.category == finding.category && old.excerpt == finding.excerpt)
            {
                Some(index) => {
                    remaining.remove(index);
                    kept.push(finding);
                }
                None => introduced.push(finding),
            }
        }
        Self {
            resolved: remaining,
            kept,
            introduced,
        }
    }
}

impl CheckReport {
    fn exceeds(&self, threshold: Option<FailOn>) -> bool {
        threshold.is_some_and(|threshold| {
            self.lint
                .findings
                .iter()
                .chain(&self.reading_load.findings)
                .any(|f| threshold.matches(&f.severity))
                || self
                    .diff
                    .iter()
                    .flat_map(|report| &report.findings)
                    .any(|f| threshold.matches(&f.severity))
                || self
                    .materials
                    .iter()
                    .flat_map(|report| &report.findings)
                    .any(|f| threshold.matches(&f.severity))
        })
    }
}

fn error(message: impl Into<String>) -> Error {
    Error::InvalidArguments(message.into())
}

fn absolute(path: &Path) -> Result<PathBuf, Error> {
    if path == Path::new("-") {
        return Err(error(
            "checkには再確認できるファイルを指定してください。標準入力はlint/diffで使えます",
        ));
    }
    if path.is_absolute() {
        Ok(path.to_owned())
    } else {
        std::env::current_dir()
            .map(|dir| dir.join(path))
            .map_err(|e| error(e.to_string()))
    }
}

fn load_file(path: &Path) -> Result<LoadedFile, Error> {
    let path = absolute(path)?;
    let text = read_source(&path)?;
    Ok(LoadedFile {
        input: InputFile {
            path,
            sha256: diff::sha256_hex(&text),
        },
        text,
    })
}

fn executable_hash() -> Result<String, Error> {
    let path = std::env::current_exe()
        .map_err(|e| error(format!("実行ファイルの場所を取得できません: {e}")))?;
    let mut file =
        File::open(&path).map_err(|e| error(format!("実行ファイルを読めません: {e}")))?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let size = file
            .read(&mut buffer)
            .map_err(|e| error(format!("実行ファイルを読めません: {e}")))?;
        if size == 0 {
            break;
        }
        digest.update(&buffer[..size]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn load_configuration(args: &CheckArgs) -> Result<(Option<Configuration>, Option<Config>), Error> {
    let Some(path) = config_path(args.config.as_deref(), args.no_config)? else {
        return Ok((None, None));
    };
    let path = absolute(&path)?;
    if args.config.is_none() && !path.try_exists().map_err(|e| error(e.to_string()))? {
        return Ok((Some(Configuration { path, sha256: None }), None));
    }
    let source = read_source(&path)?;
    let config = parse_config(&path, &source)?;
    Ok((
        Some(Configuration {
            path,
            sha256: Some(diff::sha256_hex(&source)),
        }),
        Some(config),
    ))
}

fn analyze(
    final_text: &str,
    before: Option<&str>,
    sources: &[LoadedFile],
    config: Option<&Config>,
    options: &Options,
) -> Result<CheckReport, Error> {
    let morphology = Morphology::new()?;
    let genre = options.genre.map(Genre::as_str);
    let lint = configured_lint(final_text, &morphology, genre, options.experimental, config)?;
    let mut reading_load = lint::analyze_reading_load(final_text, &morphology, genre)?;
    apply_reading_load_config(&mut reading_load, config);
    let diff = before
        .map(|before| diff::compare(before, final_text, &morphology))
        .transpose()?;
    let finding_changes = before
        .map(|before| -> Result<FindingChanges, Error> {
            let old_lint =
                configured_lint(before, &morphology, genre, options.experimental, config)?;
            let mut old_load = lint::analyze_reading_load(before, &morphology, genre)?;
            apply_reading_load_config(&mut old_load, config);
            Ok(FindingChanges::compare(
                old_lint.findings.iter().chain(&old_load.findings),
                lint.findings.iter().chain(&reading_load.findings),
            ))
        })
        .transpose()?;
    let materials = if sources.is_empty() {
        None
    } else {
        let named = sources
            .iter()
            .map(|source| materials::Source {
                path: source.input.path.clone(),
                text: &source.text,
            })
            .collect::<Vec<_>>();
        Some(materials::compare(final_text, &named, &morphology)?)
    };
    // --genre auto のときは、lintが推定して適用した種類で随筆を除く。
    let fact_gaps = options.fact_gaps.then(|| {
        let applied = lint
            .stats
            .measurements
            .get("genre_inference")
            .and_then(|inference| inference["applied"].as_str());
        fact_gaps::analyze(final_text, applied.or(genre))
    });
    Ok(CheckReport {
        lint,
        reading_load,
        diff,
        materials,
        finding_changes,
        fact_gaps,
    })
}

fn record_checksum(record: &Record) -> Result<String, Error> {
    // JSONのobjectキーを一定の順序にし、空白やフィールド順に依存させない。
    Ok(diff::sha256_hex(&serde_json::to_string(
        &serde_json::to_value(record)?,
    )?))
}

fn read_record(path: &Path) -> Result<StoredRecord, Error> {
    let stored: StoredRecord = serde_json::from_str(&read_source(path)?)?;
    if stored.format != RECORD_FORMAT || stored.schema_version != RECORD_SCHEMA_VERSION {
        return Err(error(
            "未対応の検査記録です。現在のsuiko checkで作り直してください",
        ));
    }
    if stored.checksum != record_checksum(&stored.record)? {
        return Err(error(
            "検査記録の内容が一致しません。suiko checkで再検査してください",
        ));
    }
    Ok(stored)
}

fn validate_destination(path: &Path, record: &Record) -> Result<(), Error> {
    let canonical = resolved_destination(path)?;
    let mut inputs = vec![&record.final_file.path];
    inputs.extend(record.before.iter().map(|file| &file.path));
    inputs.extend(record.materials.iter().map(|file| &file.path));
    inputs.extend(record.configuration.iter().map(|config| &config.path));
    for input in inputs {
        if resolved_destination(input)? == canonical {
            return Err(error(
                "検査記録の保存先を本文・原文・資料・設定と同じ場所にはできません",
            ));
        }
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(error(format!(
                    "検査記録の保存先には通常のファイルを指定してください: {}",
                    path.display()
                )));
            }
            // 既存の無関係なファイルを上書きしない。
            read_record(path)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(error(format!("検査記録の保存先を確認できません: {e}"))),
    }
    Ok(())
}

fn resolved_destination(path: &Path) -> Result<PathBuf, Error> {
    if let Ok(resolved) = path.canonicalize() {
        return Ok(resolved);
    }
    // 未作成の設定・記録も、親のsymlinkや「..」を解決して同じ場所か確認する。
    let parent = path.parent().and_then(|parent| parent.canonicalize().ok());
    match (parent, path.file_name()) {
        (Some(parent), Some(name)) => Ok(parent.join(name)),
        _ => Err(error(format!(
            "ファイルの保存先を確認できません: {}",
            path.display()
        ))),
    }
}

fn save_record(path: &Path, record: Record) -> Result<(), Error> {
    validate_destination(path, &record)?;
    let checksum = record_checksum(&record)?;
    let stored = StoredRecord {
        format: RECORD_FORMAT.to_owned(),
        schema_version: RECORD_SCHEMA_VERSION,
        record,
        checksum,
    };
    let bytes = serde_json::to_vec_pretty(&stored)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| error(e.to_string()))?
        .as_nanos();
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".{}.{}.tmp", std::process::id(), nonce));
    let temporary = PathBuf::from(name);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).map_err(|e| {
        error(format!(
            "検査記録を保存できません: {} ({e})",
            path.display()
        ))
    })?;
    let result = (|| {
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&temporary);
        return Err(error(format!(
            "検査記録を保存できません: {} ({e})",
            path.display()
        )));
    }
    Ok(())
}

#[derive(Serialize)]
struct Change {
    kind: &'static str,
    path: Option<PathBuf>,
    detail: String,
}

fn compare_file(
    path: &Path,
    expected: Option<&str>,
    kind: &'static str,
    changes: &mut Vec<Change>,
) {
    if expected.is_none() {
        match path.try_exists() {
            Ok(false) => return,
            Err(e) => {
                changes.push(Change {
                    kind,
                    path: Some(path.to_owned()),
                    detail: e.to_string(),
                });
                return;
            }
            Ok(true) => {}
        }
    }
    let changed = match read_source(path) {
        Ok(text) => (Some(diff::sha256_hex(&text).as_str()) != expected)
            .then(|| "検査時と内容が異なります".to_owned()),
        Err(e) => Some(e.to_string()),
    };
    if let Some(detail) = changed {
        changes.push(Change {
            kind,
            path: Some(path.to_owned()),
            detail,
        });
    }
}

fn changed_inputs(record: &Record) -> Vec<Change> {
    let mut changes = Vec::new();
    compare_file(
        &record.final_file.path,
        Some(&record.final_file.sha256),
        "final_changed",
        &mut changes,
    );
    if let Some(file) = &record.before {
        compare_file(
            &file.path,
            Some(&file.sha256),
            "before_changed",
            &mut changes,
        );
    }
    for file in &record.materials {
        compare_file(
            &file.path,
            Some(&file.sha256),
            "material_changed",
            &mut changes,
        );
    }
    if let Some(config) = &record.configuration {
        compare_file(
            &config.path,
            config.sha256.as_deref(),
            "config_changed",
            &mut changes,
        );
    }
    changes
}

pub(super) fn execute_check(args: CheckArgs) -> Result<ExitCode, Error> {
    let final_file = load_file(&args.file)?;
    let before = args.before.as_deref().map(load_file).transpose()?;
    let sources = args
        .material
        .iter()
        .map(|path| load_file(path))
        .collect::<Result<Vec<_>, _>>()?;
    let (configuration, config) = load_configuration(&args)?;
    let options = Options {
        genre: args.genre.or_else(|| config.as_ref().and_then(|c| c.genre)),
        experimental: args.experimental,
        fail_on: args
            .fail_on
            .or_else(|| config.as_ref().and_then(|c| c.fail_on)),
        fact_gaps: args.fact_gaps,
    };
    let tool_sha256 = executable_hash()?;
    let report = analyze(
        &final_file.text,
        before.as_ref().map(|file| file.text.as_str()),
        &sources,
        config.as_ref(),
        &options,
    )?;
    let threshold_exceeded = report.exceeds(options.fail_on);
    let record = Record {
        suiko_version: env!("CARGO_PKG_VERSION").to_owned(),
        tool_sha256,
        checked_at_unix_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| error(e.to_string()))?
            .as_secs(),
        final_file: final_file.input,
        before: before.map(|file| file.input),
        materials: sources.into_iter().map(|file| file.input).collect(),
        configuration,
        options,
        threshold_exceeded,
        report: serde_json::to_value(&report)?,
    };
    if !changed_inputs(&record).is_empty() {
        return Err(error(
            "検査中に対象ファイルが変わりました。記録を保存せず終了します。再検査してください",
        ));
    }
    let path = if let Some(path) = args.record {
        absolute(&path)?
    } else {
        let mut path = record.final_file.path.as_os_str().to_os_string();
        path.push(".suiko-check.json");
        PathBuf::from(path)
    };
    // 保存が成功するまでは「検査済み」の結果を出さない。
    let output = json!({"status": "checked", "record": path, "check": record});
    save_record(&path, record)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("検査を実行しました。記録: {}", path.display());
        print_report(&report);
        println!(
            "提出前に suiko verify <記録ファイル> を実行してください。記録は検査の実行を示し、意味や事実の正しさを保証しません。"
        );
    }
    Ok(if threshold_exceeded {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    })
}

fn print_report(report: &CheckReport) {
    for (lane, findings) in [
        ("lint", &report.lint.findings),
        ("reading_load", &report.reading_load.findings),
    ] {
        println!("{lane}: {}件", findings.len());
        for finding in findings {
            println!(
                "  L{} [{}:{}] {}\n    {}",
                finding.line, finding.severity, finding.category, finding.excerpt, finding.detail
            );
        }
    }
    if let Some(changes) = &report.finding_changes {
        println!(
            "原文からの指摘の変化: 解消 {}件 / 残存 {}件 / 新規 {}件",
            changes.resolved.len(),
            changes.kept.len(),
            changes.introduced.len()
        );
        for (label, keys) in [("残存", &changes.kept), ("新規", &changes.introduced)] {
            for key in keys {
                println!("  {label} L{} [{}] {}", key.line, key.category, key.excerpt);
            }
        }
    }
    if let Some(diff) = &report.diff {
        super::print_diff_human("原文", "最終稿", diff);
    } else {
        println!("diff: 未実施（原文の指定なし）");
    }
    if let Some(materials) = &report.materials {
        println!("資料照合: {}件", materials.findings.len());
        for finding in &materials.findings {
            println!(
                "  L{} [{}:{}] {}\n    {}",
                finding.line, finding.severity, finding.category, finding.excerpt, finding.detail
            );
            for evidence in &finding.evidence {
                println!(
                    "    資料 {} L{}: {}",
                    evidence.path.display(),
                    evidence.line,
                    evidence.excerpt
                );
            }
        }
    } else {
        println!("資料照合: 未実施（資料の指定なし）");
    }
    if let Some(gaps) = &report.fact_gaps {
        if let Some(genre) = &gaps.skipped {
            println!("事実の穴の候補: 未実施（ジャンル {genre} では出しません）");
            return;
        }
        println!(
            "事実の穴の候補: {}件（比喩や程度の語だけで、中身や値が書かれていないかもしれない文です。資料や前後に中身があれば補い、なければ書き手に確かめてください。候補は一部だけです）",
            gaps.count
        );
        for gap in &gaps.findings {
            println!(
                "  L{} [{}] 「{}」: {}\n    足りないかもしれないもの: {}",
                gap.line, gap.kind, gap.matched, gap.excerpt, gap.need
            );
        }
    }
}

pub(super) fn execute_verify(args: VerifyArgs) -> Result<ExitCode, Error> {
    let stored = read_record(&args.record)?;
    let record = stored.record;
    let mut changes = changed_inputs(&record);
    if record.tool_sha256 != executable_hash()? || record.suiko_version != env!("CARGO_PKG_VERSION")
    {
        changes.push(Change {
            kind: "tool_changed",
            path: None,
            detail: "実行ファイルが変わりました。現在のsuiko checkで再検査してください".to_owned(),
        });
    }
    let mut replayed = false;
    if args.replay && changes.is_empty() {
        let final_file = load_file(&record.final_file.path)?;
        let before = record
            .before
            .as_ref()
            .map(|file| load_file(&file.path))
            .transpose()?;
        let sources = record
            .materials
            .iter()
            .map(|file| load_file(&file.path))
            .collect::<Result<Vec<_>, _>>()?;
        let config = record
            .configuration
            .as_ref()
            .filter(|config| config.sha256.is_some())
            .map(|config| parse_config(&config.path, &read_source(&config.path)?))
            .transpose()?;
        let report = analyze(
            &final_file.text,
            before.as_ref().map(|file| file.text.as_str()),
            &sources,
            config.as_ref(),
            &record.options,
        )?;
        replayed = true;
        if serde_json::to_value(&report)? != record.report
            || report.exceeds(record.options.fail_on) != record.threshold_exceeded
        {
            changes.push(Change {
                kind: "result_changed",
                path: None,
                detail: "再実行した検査結果が記録と異なります".to_owned(),
            });
        }
        changes.extend(changed_inputs(&record));
    }
    let current = changes.is_empty();
    let status = if current { "current" } else { "stale" };
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &json!({"status": status, "record": absolute(&args.record)?, "replayed": replayed, "threshold_exceeded": record.threshold_exceeded, "changes": changes})
            )?
        );
    } else {
        println!(
            "{}",
            if current {
                "検査時の本文・原文・資料・設定・実行ファイルと一致しています。"
            } else {
                "検査記録は現在の状態と一致しません。suiko checkで再検査してください。"
            }
        );
        for change in &changes {
            println!(
                "  {} {}: {}",
                change.kind,
                change
                    .path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
                change.detail
            );
        }
        if record.threshold_exceeded {
            println!("指定した重要度以上の候補が記録に残っています。");
        }
        println!("この確認は、意味や事実の正しさを保証するものではありません。");
    }
    Ok(if current && !record.threshold_exceeded {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    })
}
