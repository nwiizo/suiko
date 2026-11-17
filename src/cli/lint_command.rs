//! `suiko lint`: 引数、出力の形（人が読む表示・JSON・GitHubの注釈・SARIF）、baselineとの比較。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, ValueEnum};
use serde::Serialize;
use serde_json::json;

use crate::lint::{Finding, LintStats};
use crate::morphology::Morphology;
use crate::{Error, lint, read_source};

use super::batch::{SummaryFormat, tsv};
use super::config::{apply_reading_load_config, configured_lint, load_config};
use super::{ALL_RULES, FailOn, Genre, note_experimental_flag, read_input, validate_inputs};

#[derive(Debug, Args)]
pub(super) struct LintArgs {
    /// lint 対象の Markdown/テキストファイル。複数指定可。- で標準入力
    #[arg(required = true)]
    files: Vec<String>,
    /// 機械可読な JSON で出力する
    #[arg(long)]
    json: bool,
    /// ファイル別・規則別の件数を集計する
    #[arg(long, value_enum, conflicts_with_all = ["json", "format", "baseline"])]
    summary: Option<SummaryFormat>,
    /// ジャンル別の校正済み閾値を適用する。一部の構造検査は --genre essay が必要
    #[arg(long, value_enum)]
    genre: Option<Genre>,
    /// 互換のため受け付ける（v0.4.3から全規則を既定で表示する）
    #[arg(long)]
    experimental: bool,
    /// 前回の JSON と比較して解消・新規・継続を分類する
    #[arg(long, value_name = "PREV.json")]
    baseline: Option<PathBuf>,
    /// 読解負荷レーンを出力する（既定で有効。v0.3.9以前との互換のため残す）
    #[arg(long, conflicts_with = "no_reading_load")]
    reading_load: bool,
    /// 読解負荷レーンを出力しない
    #[arg(long)]
    no_reading_load: bool,
    /// 指定 severity 以上の finding があれば終了コード2を返す
    #[arg(long, value_enum)]
    fail_on: Option<FailOn>,
    /// 出力形式を指定する（github: GitHub Actionsのworkflowコマンド注釈、
    /// sarif: エディタやコードスキャンが読めるSARIF 2.1.0）
    #[arg(long, value_enum, conflicts_with = "json")]
    format: Option<OutputFormat>,
    /// 指定した設定ファイルを使用する（自動検出より優先）
    #[arg(long, value_name = "PATH", conflicts_with = "no_config")]
    config: Option<PathBuf>,
    /// カレントディレクトリの .suiko.toml を読み込まない
    #[arg(long, conflicts_with = "config")]
    no_config: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum OutputFormat {
    Github,
    Sarif,
}

/// lint --jsonの指摘。ライブラリのFindingのキーに、登録表の族と確度を足す。
#[derive(Serialize)]
struct FindingOutput<'a> {
    #[serde(flatten)]
    finding: &'a Finding,
    #[serde(skip_serializing_if = "Option::is_none")]
    family: Option<lint::Family>,
    confidence: lint::Confidence,
}

fn finding_outputs(findings: &[Finding]) -> Vec<FindingOutput<'_>> {
    findings
        .iter()
        .map(|finding| FindingOutput {
            finding,
            family: lint::rule_of(&finding.category).map(|rule| rule.family),
            confidence: lint::confidence(&finding.category),
        })
        .collect()
}

#[derive(Serialize)]
struct ReadingLoadOutput<'a> {
    stats: &'a lint::ReadingLoadStats,
    findings: Vec<FindingOutput<'a>>,
}

#[derive(Serialize)]
struct LintOutput<'a> {
    file: &'a str,
    suiko_version: &'static str,
    stats: &'a LintStats,
    /// まず直す候補。重大度と規則の確度で並べた上位数件。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    first_fixes: Vec<lint::FirstFix>,
    findings: Vec<FindingOutput<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    baseline: Option<&'a lint::BaselineReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reading_load: Option<ReadingLoadOutput<'a>>,
}

pub(super) struct LintRun {
    pub(super) file: String,
    pub(super) report: lint::LintReport,
    pub(super) baseline: Option<lint::BaselineReport>,
    pub(super) reading_load: Option<lint::ReadingLoadReport>,
}

/// --baseline のJSONを、単一record(object)または複数record(array)として展開する。
fn baseline_records(data: &serde_json::Value) -> Result<Vec<&serde_json::Value>, String> {
    match data {
        serde_json::Value::Object(_) => Ok(vec![data]),
        serde_json::Value::Array(items) => {
            if items.iter().all(serde_json::Value::is_object) {
                Ok(items.iter().collect())
            } else {
                Err("--baseline の配列にオブジェクト以外の要素があります。baseline比較を無視して通常のlintを実行します。".to_owned())
            }
        }
        _ => Err(
            "--baseline の内容が JSON オブジェクトでも配列でもありません。baseline比較を無視して通常のlintを実行します。"
                .to_owned(),
        ),
    }
}

/// baseline recordと今回の実行条件が一致しないときは明示的に失敗させる。
fn ensure_baseline_compatible(
    record: &serde_json::Value,
    file: &str,
    genre: Option<&str>,
    experimental: bool,
) -> Result<(), Error> {
    if let Some(version) = record
        .get("suiko_version")
        .and_then(serde_json::Value::as_str)
        && version != env!("CARGO_PKG_VERSION")
    {
        return Err(Error::InvalidArguments(format!(
            "--baseline のSuikoバージョンが現在と一致しません: {file} (baseline={version}, current={})。同じバージョンで作り直してください",
            env!("CARGO_PKG_VERSION")
        )));
    }
    let baseline_genre = record
        .pointer("/stats/genre")
        .and_then(serde_json::Value::as_str);
    if baseline_genre != genre {
        return Err(Error::InvalidArguments(format!(
            "--baseline のgenreが現在と一致しません: {file} (baseline={}, current={})",
            baseline_genre.unwrap_or("なし"),
            genre.unwrap_or("なし")
        )));
    }
    let baseline_experimental = record
        .pointer("/stats/experimental")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if baseline_experimental != experimental {
        return Err(Error::InvalidArguments(format!(
            "--baseline のexperimental設定が現在と一致しません: {file} (baseline={baseline_experimental}, current={experimental})。v0.4.3から全規則を既定で表示するため、baselineを作り直してください"
        )));
    }
    Ok(())
}

/// workflowコマンドのメッセージ用エスケープ。
fn github_escape_data(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// workflowコマンドのプロパティ値用エスケープ。
fn github_escape_property(value: &str) -> String {
    github_escape_data(value)
        .replace(',', "%2C")
        .replace(':', "%3A")
}

fn output_level(severity: &str, fallback_level: &'static str) -> &'static str {
    match severity {
        "critical" => "error",
        "warn" => "warning",
        _ => fallback_level,
    }
}

fn lint_findings(run: &LintRun) -> impl Iterator<Item = (&Finding, Option<&'static str>)> {
    run.report
        .findings
        .iter()
        .map(|finding| (finding, None))
        // 読解負荷レーンは自然度と分離して、常にinfo相当で出力する。
        .chain(run.reading_load.iter().flat_map(|report| {
            report
                .findings
                .iter()
                .map(|finding| (finding, Some("info")))
        }))
}

fn github_annotation(file: &str, finding: &Finding, severity_override: Option<&str>) -> String {
    let command = output_level(
        severity_override.unwrap_or(finding.severity.as_str()),
        "notice",
    );
    let mut properties = format!("file={}", github_escape_property(file));
    if let Some(span) = &finding.span {
        properties.push_str(&format!(
            ",line={},endLine={},col={},endColumn={}",
            span.start_line, span.end_line, span.start_column, span.end_column
        ));
    } else {
        properties.push_str(&format!(",line={}", finding.line));
    }
    properties.push_str(&format!(
        ",title={}",
        github_escape_property(&format!("suiko {}", finding.category))
    ));
    format!(
        "::{command} {properties}::{}",
        github_escape_data(&finding.detail)
    )
}

/// SARIF 2.1.0の最小構成。columnKind=unicodeCodePointsを宣言することで、
/// spanのUnicode scalar数え・1始まりの列をそのまま使える。
fn sarif_result(file: &str, finding: &Finding, level_override: Option<&str>) -> serde_json::Value {
    let level = output_level(level_override.unwrap_or(finding.severity.as_str()), "note");
    let region = if let Some(span) = &finding.span {
        serde_json::json!({
            "startLine": span.start_line,
            "startColumn": span.start_column,
            "endLine": span.end_line,
            "endColumn": span.end_column,
        })
    } else {
        serde_json::json!({"startLine": finding.line})
    };
    serde_json::json!({
        "ruleId": finding.category,
        "level": level,
        "message": {"text": finding.detail},
        "locations": [{
            "physicalLocation": {
                "artifactLocation": {"uri": file},
                "region": region,
            }
        }],
    })
}

fn sarif_output(runs: &[LintRun]) -> serde_json::Value {
    let mut results = Vec::new();
    let mut rule_ids = std::collections::BTreeSet::new();
    for run in runs {
        for (finding, level_override) in lint_findings(run) {
            rule_ids.insert(finding.category.clone());
            results.push(sarif_result(&run.file, finding, level_override));
        }
    }
    serde_json::json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {"driver": {
                "name": "suiko",
                "version": env!("CARGO_PKG_VERSION"),
                "informationUri": "https://github.com/nwiizo/suiko",
                "rules": rule_ids
                    .iter()
                    .map(|id| serde_json::json!({"id": id}))
                    .collect::<Vec<_>>(),
            }},
            "columnKind": "unicodeCodePoints",
            "results": results,
        }],
    })
}

fn print_lint_github(run: &LintRun) {
    for (finding, severity_override) in lint_findings(run) {
        println!(
            "{}",
            github_annotation(&run.file, finding, severity_override)
        );
    }
}

fn print_lint_human(run: &LintRun) {
    let file = &run.file;
    let report = &run.report;
    println!("=== lint: {file} ===");
    println!("検出件数: {}", report.stats.total_findings);
    if !report.stats.by_category.is_empty() {
        println!("カテゴリ別内訳:");
        let mut categories = report.stats.by_category.iter().collect::<Vec<_>>();
        categories.sort_by_key(|(_, count)| std::cmp::Reverse(**count));
        for (category, count) in categories {
            println!("  - {category}: {count}");
        }
    }
    if let Some(baseline) = &run.baseline {
        println!(
            "ベースライン比較: 解消: {}件 / 新規: {}件 / 継続: {}件",
            baseline.summary.resolved, baseline.summary.new, baseline.summary.persisting
        );
        if baseline.file_status == "added" {
            println!("(baselineに対応recordがないファイルのため、全findingを新規として扱う)");
        }
    }
    let first_fixes = lint::first_fixes(&report.findings, lint::FIRST_FIX_LIMIT);
    if !first_fixes.is_empty() {
        println!(
            "まず直す候補（重大度と規則の確度で上位{}件）:",
            first_fixes.len()
        );
        for fix in &first_fixes {
            let confidence = match fix.confidence {
                lint::Confidence::High => "確度高",
                lint::Confidence::Medium => "確度中",
                lint::Confidence::Low => "確度低",
            };
            println!(
                "  {}. L{} ({}・{confidence}) {}",
                fix.rank, fix.line, fix.category, fix.excerpt
            );
        }
    }
    println!();
    if report.findings.is_empty() {
        println!("検出なし。");
    } else {
        for finding in &report.findings {
            let label = match finding.severity.as_str() {
                "info" => "情報",
                "warn" => "警告",
                "critical" => "重大",
                other => other,
            };
            let status = match finding.status.as_deref() {
                Some("new") => "[新規] ",
                Some("persisting") => "[継続] ",
                _ => "",
            };
            println!("{status}[{label}] L{} ({})", finding.line, finding.category);
            println!("    該当箇所: {}", finding.excerpt);
            if !finding.detail.is_empty() {
                println!("    詳細    : {}", finding.detail);
            }
            if let Some(suggestion) = &finding.suggestion {
                let replacement = if suggestion.replacement.is_empty() {
                    "(削除)".to_owned()
                } else {
                    format!("「{}」", suggestion.replacement)
                };
                println!(
                    "    修正候補: L{} C{} -「{}」 +{replacement}（preimage一致時のみ適用可。Suikoは書き換えない）",
                    suggestion.span.start_line, suggestion.span.start_column, suggestion.preimage
                );
            }
            println!();
        }
    }
    if let Some(reading_load) = &run.reading_load {
        println!("=== 読解負荷（推敲用の指さし・自然度スコアには含まない） ===");
        println!(
            "指摘件数: {}（本文 {} 文）\n",
            reading_load.stats.total, reading_load.stats.sentences
        );
        for finding in &reading_load.findings {
            println!("[指さし] L{} ({})", finding.line, finding.category);
            println!("    該当箇所: {}", finding.excerpt);
            println!("    詳細    : {}\n", finding.detail);
        }
    }
}

pub(super) fn execute(args: LintArgs) -> Result<ExitCode, Error> {
    validate_inputs(&args.files)?;
    note_experimental_flag(args.experimental);
    let config = load_config(args.config.as_deref(), args.no_config)?;
    let morphology = Morphology::new()?;
    let genre = args
        .genre
        .or_else(|| config.as_ref().and_then(|config| config.genre))
        .map(Genre::as_str);
    let fail_on = args
        .fail_on
        .or_else(|| config.as_ref().and_then(|config| config.fail_on));
    let baseline_data = if let Some(path) = &args.baseline {
        Some((
            path.display().to_string(),
            serde_json::from_str::<serde_json::Value>(&read_source(path)?)?,
        ))
    } else {
        None
    };
    let baseline_map = if let Some((_, data)) = &baseline_data {
        match baseline_records(data) {
            Ok(records) => Some(
                records
                    .into_iter()
                    .map(|record| {
                        (
                            record
                                .get("file")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                            record,
                        )
                    })
                    .collect::<BTreeMap<_, _>>(),
            ),
            Err(warning) => {
                eprintln!("警告: {warning}");
                None
            }
        }
    } else {
        None
    };
    let mut runs = Vec::new();
    for file in &args.files {
        let text = read_input(file)?;
        let mut report = configured_lint(&text, &morphology, genre, ALL_RULES, config.as_ref())?;
        let baseline = match (&baseline_data, &baseline_map) {
            (Some((baseline_file, _)), Some(records)) => {
                if let Some(record) = records.get(file.as_str()) {
                    ensure_baseline_compatible(record, file, genre, ALL_RULES)?;
                    match lint::apply_baseline(&mut report.findings, record, baseline_file.clone())
                    {
                        Ok(report) => Some(report),
                        Err(warning) => {
                            eprintln!("警告: {warning}");
                            None
                        }
                    }
                } else {
                    eprintln!(
                        "警告: --baseline に {file} のrecordがないため、全findingを新規として扱います。"
                    );
                    Some(lint::baseline_added(
                        &mut report.findings,
                        baseline_file.clone(),
                    ))
                }
            }
            _ => None,
        };
        let reading_load = if !args.no_reading_load {
            let mut report = lint::analyze_reading_load(&text, &morphology, genre)?;
            apply_reading_load_config(&mut report, config.as_ref());
            Some(report)
        } else {
            None
        };
        runs.push(LintRun {
            file: file.clone(),
            report,
            baseline,
            reading_load,
        });
    }
    if let Some(records) = &baseline_map {
        let removed = records
            .keys()
            .filter(|file| !args.files.iter().any(|input| input == *file))
            .cloned()
            .collect::<Vec<_>>();
        if !removed.is_empty() {
            eprintln!(
                "警告: --baseline にあって今回の対象にないファイル: {}",
                removed.join(", ")
            );
        }
    }
    if let Some(format) = args.summary {
        print_summary(&runs, format)?;
    } else if args.json {
        let output = runs
            .iter()
            .map(|run| LintOutput {
                file: &run.file,
                suiko_version: env!("CARGO_PKG_VERSION"),
                stats: &run.report.stats,
                first_fixes: lint::first_fixes(&run.report.findings, lint::FIRST_FIX_LIMIT),
                findings: finding_outputs(&run.report.findings),
                baseline: run.baseline.as_ref(),
                reading_load: run.reading_load.as_ref().map(|report| ReadingLoadOutput {
                    stats: &report.stats,
                    findings: finding_outputs(&report.findings),
                }),
            })
            .collect::<Vec<_>>();
        if output.len() == 1 {
            println!("{}", serde_json::to_string_pretty(&output[0])?);
        } else {
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    } else if args.format == Some(OutputFormat::Github) {
        for run in &runs {
            print_lint_github(run);
        }
    } else if args.format == Some(OutputFormat::Sarif) {
        println!("{}", serde_json::to_string_pretty(&sarif_output(&runs))?);
    } else {
        for run in &runs {
            print_lint_human(run);
        }
    }
    let failed = fail_on.is_some_and(|threshold| {
        runs.iter().any(|run| {
            run.report
                .findings
                .iter()
                .any(|finding| threshold.matches(&finding.severity))
        })
    });
    Ok(if failed {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    })
}

#[derive(Default, Serialize)]
struct Counts {
    natural: usize,
    reading_load: usize,
}

/// `--summary`: ファイル別・規則別の件数。
fn print_summary(runs: &[LintRun], format: SummaryFormat) -> Result<(), Error> {
    let reading_enabled = runs.iter().any(|run| run.reading_load.is_some());
    let reading_tsv = |count: usize| {
        if reading_enabled {
            count.to_string()
        } else {
            String::new()
        }
    };
    let mut total = Counts::default();
    let mut categories = BTreeMap::<String, Counts>::new();
    let mut files = Vec::new();
    for run in runs {
        let counts = Counts {
            natural: run.report.findings.len(),
            reading_load: run.reading_load.as_ref().map_or(0, |r| r.findings.len()),
        };
        total.natural += counts.natural;
        total.reading_load += counts.reading_load;
        for finding in &run.report.findings {
            categories
                .entry(finding.category.clone())
                .or_default()
                .natural += 1;
        }
        for finding in run.reading_load.iter().flat_map(|r| &r.findings) {
            categories
                .entry(finding.category.clone())
                .or_default()
                .reading_load += 1;
        }
        files.push(json!({"file": run.file, "natural": counts.natural, "reading_load": run.reading_load.as_ref().map(|r| r.findings.len())}));
    }
    match format {
        SummaryFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "schema_version": 1, "suiko_version": env!("CARGO_PKG_VERSION"), "reading_load_enabled": reading_enabled, "files": files,
                "total": {"natural": total.natural, "reading_load": reading_enabled.then_some(total.reading_load)},
                "categories": categories.iter().map(|(category, counts)| json!({"category": category, "natural": counts.natural, "reading_load": reading_enabled.then_some(counts.reading_load)})).collect::<Vec<_>>()
            }))?
        ),
        SummaryFormat::Tsv => {
            println!("kind\tfile\tcategory\tnatural\treading_load");
            for file in files {
                println!(
                    "file\t{}\t\t{}\t{}",
                    tsv(file["file"].as_str().unwrap_or_default()),
                    file["natural"],
                    file["reading_load"]
                        .as_u64()
                        .map(|n| n.to_string())
                        .unwrap_or_default()
                );
            }
            for (category, counts) in categories {
                println!(
                    "category\t\t{category}\t{}\t{}",
                    counts.natural,
                    reading_tsv(counts.reading_load)
                );
            }
            println!(
                "total\t\t\t{}\t{}",
                total.natural,
                reading_tsv(total.reading_load)
            );
        }
    }
    Ok(())
}
