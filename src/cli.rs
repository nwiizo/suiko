use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

use crate::lint::{Finding, LintStats};
use crate::morphology::Morphology;
use crate::{Error, lint, outline, read_source, terms};

#[derive(Debug, Parser)]
#[command(
    name = "suiko",
    version,
    about = "日本語文書を決定的に診断し、自然で明晰な推敲を支援する"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// AI的な定型、翻訳調、単調な構造、読解負荷を検出する
    Lint(LintArgs),
    /// 見出し、段落の先頭文、箇条書きから文書構造を抽出する
    Outline(FileArgs),
    /// 専門用語候補と初出時の説明手掛かりを抽出する
    Terms(TermsArgs),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, ValueEnum)]
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

#[derive(Debug, Args)]
struct FileArgs {
    /// 対象の Markdown/テキストファイル。複数指定可。- で標準入力
    #[arg(required = true)]
    files: Vec<String>,
    /// 機械可読な JSON で出力する
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct TermsArgs {
    /// 対象の Markdown/テキストファイル。複数指定可。- で標準入力
    #[arg(required = true)]
    files: Vec<String>,
    /// 機械可読な JSON で出力する
    #[arg(long)]
    json: bool,
    /// 複数ファイルの用語を集計し、表記揺れを一覧化する（ファイルは書き換えない）
    #[arg(long)]
    audit: bool,
}

#[derive(Debug, Args)]
struct LintArgs {
    /// lint 対象の Markdown/テキストファイル。複数指定可。- で標準入力
    #[arg(required = true)]
    files: Vec<String>,
    /// 機械可読な JSON で出力する
    #[arg(long)]
    json: bool,
    /// ジャンル別の校正済み閾値を適用する
    #[arg(long, value_enum)]
    genre: Option<Genre>,
    /// 未校正または無反応の実験的検出器も有効にする
    #[arg(long)]
    experimental: bool,
    /// 前回の JSON と比較して解消・新規・継続を分類する
    #[arg(long, value_name = "PREV.json")]
    baseline: Option<PathBuf>,
    /// 読解負荷レーンを追加する
    #[arg(long)]
    reading_load: bool,
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, ValueEnum)]
#[serde(rename_all = "lowercase")]
enum FailOn {
    Info,
    Warn,
    Critical,
}

impl FailOn {
    fn matches(self, severity: &str) -> bool {
        let rank = match severity {
            "critical" => 3,
            "warn" => 2,
            _ => 1,
        };
        let threshold = match self {
            Self::Info => 1,
            Self::Warn => 2,
            Self::Critical => 3,
        };
        rank >= threshold
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    version: u32,
    genre: Option<Genre>,
    fail_on: Option<FailOn>,
    #[serde(default)]
    disabled_rules: Vec<String>,
    #[serde(default)]
    allow: Vec<Allowance>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Allowance {
    category: String,
    text: String,
    reason: String,
}

impl Config {
    fn validate(&self, path: &Path) -> Result<(), Error> {
        if self.version != 1 {
            return Err(config_error(
                path,
                format!(
                    "version = {} は未対応です。version = 1 を指定してください",
                    self.version
                ),
            ));
        }
        for rule in &self.disabled_rules {
            if !lint::is_known_rule(rule) {
                return Err(config_error(path, format!("未知のルールです: {rule}")));
            }
        }
        for allowance in &self.allow {
            if !lint::is_known_rule(&allowance.category) {
                return Err(config_error(
                    path,
                    format!("未知のルールです: {}", allowance.category),
                ));
            }
            if allowance.text.trim().is_empty() {
                return Err(config_error(path, "allow.text は空にできません"));
            }
            if allowance.reason.trim().is_empty() {
                return Err(config_error(path, "allow.reason は空にできません"));
            }
        }
        Ok(())
    }

    fn suppresses(&self, finding: &Finding) -> bool {
        self.disabled_rules
            .iter()
            .any(|rule| rule == &finding.category)
            || self.allow.iter().any(|allowance| {
                allowance.category == finding.category && finding.excerpt.contains(&allowance.text)
            })
    }
}

fn config_error(path: &Path, message: impl Into<String>) -> Error {
    Error::Config {
        path: path.display().to_string(),
        message: message.into(),
    }
}

fn load_config(explicit: Option<&Path>, no_config: bool) -> Result<Option<Config>, Error> {
    if no_config {
        return Ok(None);
    }
    let path = if let Some(path) = explicit {
        path.to_path_buf()
    } else {
        let path = std::env::current_dir()
            .map_err(|source| config_error(Path::new(".suiko.toml"), source.to_string()))?
            .join(".suiko.toml");
        if !path.exists() {
            return Ok(None);
        }
        path
    };
    let source = read_source(&path)?;
    let config = toml::from_str::<Config>(&source)
        .map_err(|source| config_error(&path, source.to_string()))?;
    config.validate(&path)?;
    Ok(Some(config))
}

fn category_counts(findings: &[Finding]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for finding in findings {
        *counts.entry(finding.category.clone()).or_default() += 1;
    }
    counts
}

/// 設定による除外を適用し、残ったfindingの総数とカテゴリ別内訳を返す。
fn retain_allowed(
    findings: &mut Vec<Finding>,
    config: &Config,
) -> (usize, BTreeMap<String, usize>) {
    findings.retain(|finding| !config.suppresses(finding));
    (findings.len(), category_counts(findings))
}

fn apply_config(report: &mut lint::LintReport, config: Option<&Config>) {
    if let Some(config) = config {
        let (total, by_category) = retain_allowed(&mut report.findings, config);
        report.stats.total_findings = total;
        report.stats.by_category = by_category;
    }
}

fn apply_reading_load_config(report: &mut lint::ReadingLoadReport, config: Option<&Config>) {
    if let Some(config) = config {
        let (total, by_category) = retain_allowed(&mut report.findings, config);
        report.stats.total = total;
        report.stats.by_category = by_category;
    }
}

#[derive(Serialize)]
struct LintOutput<'a> {
    file: &'a str,
    suiko_version: &'static str,
    stats: &'a LintStats,
    findings: &'a [Finding],
    #[serde(skip_serializing_if = "Option::is_none")]
    baseline: Option<&'a lint::BaselineReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reading_load: Option<&'a lint::ReadingLoadReport>,
}

struct LintRun {
    file: String,
    report: lint::LintReport,
    baseline: Option<lint::BaselineReport>,
    reading_load: Option<lint::ReadingLoadReport>,
}

#[derive(Serialize)]
struct OutlineOutput<'a> {
    file: &'a str,
    #[serde(flatten)]
    report: &'a outline::OutlineReport,
}

#[derive(Serialize)]
struct TermsOutput<'a> {
    file: &'a str,
    #[serde(flatten)]
    report: &'a terms::TermsReport,
}

#[derive(Serialize)]
struct TermsAuditOutput<'a> {
    suiko_version: &'static str,
    #[serde(flatten)]
    report: &'a terms::TermsAuditReport,
}

fn print_terms_audit_human(report: &terms::TermsAuditReport) {
    println!("=== terms audit: {}ファイル ===", report.files.len());
    println!(
        "用語 {}件、表記揺れ {}組。集計は確認材料であり、置換や書き換えは行いません。\n",
        report.terms.len(),
        report.variants.len()
    );
    for group in &report.variants {
        let spellings = group
            .spellings
            .iter()
            .map(|spelling| format!("{}({}回)", spelling.term, spelling.total_count))
            .collect::<Vec<_>>()
            .join(" / ");
        println!("[表記揺れ] {spellings}  → 正規化表記: {}", group.normalized);
    }
    if !report.variants.is_empty() {
        println!();
    }
    for term in &report.terms {
        println!(
            "{} (合計{}回, {}ファイル{})",
            term.term,
            term.total_count,
            term.files.len(),
            if term.files.iter().any(|entry| entry.has_gloss_hint) {
                ", 説明手掛かりあり"
            } else {
                ""
            }
        );
        for entry in &term.files {
            println!(
                "    {} L{} ({}回)",
                entry.file, entry.first_line, entry.count
            );
        }
    }
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
            "--baseline のexperimental設定が現在と一致しません: {file} (baseline={baseline_experimental}, current={experimental})"
        )));
    }
    Ok(())
}

fn read_input(file: &str) -> Result<String, Error> {
    if file == "-" {
        let mut input = String::new();
        std::io::stdin()
            .read_to_string(&mut input)
            .map_err(|source| Error::Read {
                path: "標準入力".to_owned(),
                source,
            })?;
        Ok(input)
    } else {
        read_source(Path::new(file))
    }
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

fn github_annotation(file: &str, finding: &Finding, severity_override: Option<&str>) -> String {
    let command = match severity_override.unwrap_or(finding.severity.as_str()) {
        "critical" => "error",
        "warn" => "warning",
        _ => "notice",
    };
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
    let level = match level_override.unwrap_or(finding.severity.as_str()) {
        "critical" => "error",
        "warn" => "warning",
        _ => "note",
    };
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
        for finding in &run.report.findings {
            rule_ids.insert(finding.category.clone());
            results.push(sarif_result(&run.file, finding, None));
        }
        if let Some(reading_load) = &run.reading_load {
            for finding in &reading_load.findings {
                rule_ids.insert(finding.category.clone());
                // 読解負荷レーンは自然度と分離した指さしなので、常にnoteで出す
                results.push(sarif_result(&run.file, finding, Some("info")));
            }
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
    for finding in &run.report.findings {
        println!("{}", github_annotation(&run.file, finding, None));
    }
    if let Some(reading_load) = &run.reading_load {
        for finding in &reading_load.findings {
            // 読解負荷レーンは自然度と分離した指さしなので、常にnoticeで出す
            println!("{}", github_annotation(&run.file, finding, Some("info")));
        }
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

fn print_outline_human(file: &str, report: &outline::OutlineReport) {
    println!("=== outline: {file} ===\n");
    if report.outline.is_empty() {
        println!("(スケルトンなし)");
    }
    for entry in &report.outline {
        if entry.kind == "heading" {
            let level = entry.level.unwrap_or(1);
            println!(
                "{:>6}  {}{} {}",
                format!("L{}", entry.line),
                "  ".repeat(level.saturating_sub(1)),
                "#".repeat(level),
                entry.text
            );
        } else {
            println!("{:>6}    {}", format!("L{}", entry.line), entry.text);
        }
    }
    println!("\n=== 見出し統計（判断材料。判定はAIが行う） ===\n");
    println!("見出し総数: {}", report.heading_stats.total_headings);
}

fn print_terms_human(file: &str, report: &terms::TermsReport) {
    println!("=== terms: {file} ===");
    println!(
        "has_gloss_hint は説明済みの判定ではなく、初出近傍に説明マーカーがあるという手掛かりです。\n"
    );
    if report.terms.is_empty() {
        println!("(用語候補なし)");
        return;
    }
    for term in &report.terms {
        println!(
            "L{} {} (出現{}回, 説明手掛かり: {})",
            term.first_line,
            term.term,
            term.count,
            if term.has_gloss_hint {
                "あり"
            } else {
                "なし"
            }
        );
        println!("    近傍: {}\n", term.context);
    }
}

fn validate_inputs(files: &[String]) -> Result<(), Error> {
    if files.len() > 1 && files.iter().any(|file| file == "-") {
        return Err(Error::InvalidArguments(
            "標準入力（-）は他のファイルと同時に指定できません".to_owned(),
        ));
    }
    Ok(())
}

fn execute(cli: Cli) -> Result<ExitCode, Error> {
    match cli.command {
        Command::Lint(args) => {
            validate_inputs(&args.files)?;
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
                let mut report = lint::analyze(&text, &morphology, genre, args.experimental)?;
                apply_config(&mut report, config.as_ref());
                let baseline = match (&baseline_data, &baseline_map) {
                    (Some((baseline_file, _)), Some(records)) => {
                        if let Some(record) = records.get(file.as_str()) {
                            ensure_baseline_compatible(record, file, genre, args.experimental)?;
                            match lint::apply_baseline(
                                &mut report.findings,
                                record,
                                baseline_file.clone(),
                            ) {
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
                let reading_load = if args.reading_load {
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
            if args.json {
                let output = runs
                    .iter()
                    .map(|run| LintOutput {
                        file: &run.file,
                        suiko_version: env!("CARGO_PKG_VERSION"),
                        stats: &run.report.stats,
                        findings: &run.report.findings,
                        baseline: run.baseline.as_ref(),
                        reading_load: run.reading_load.as_ref(),
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
            return Ok(if failed {
                ExitCode::from(2)
            } else {
                ExitCode::SUCCESS
            });
        }
        Command::Outline(args) => {
            validate_inputs(&args.files)?;
            let morphology = Morphology::new()?;
            let mut reports = Vec::new();
            for file in &args.files {
                let text = read_input(file)?;
                reports.push((file, outline::analyze(&text, &morphology)?));
            }
            if args.json {
                if reports.len() == 1 {
                    println!("{}", serde_json::to_string_pretty(&reports[0].1)?);
                } else {
                    let output = reports
                        .iter()
                        .map(|(file, report)| OutlineOutput { file, report })
                        .collect::<Vec<_>>();
                    println!("{}", serde_json::to_string_pretty(&output)?);
                }
            } else {
                for (file, report) in reports {
                    print_outline_human(file, &report);
                }
            }
        }
        Command::Terms(args) => {
            validate_inputs(&args.files)?;
            let morphology = Morphology::new()?;
            if args.audit {
                let mut inputs = Vec::new();
                for file in &args.files {
                    inputs.push((file.clone(), read_input(file)?));
                }
                let report = terms::audit(&inputs, &morphology)?;
                if args.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&TermsAuditOutput {
                            suiko_version: env!("CARGO_PKG_VERSION"),
                            report: &report,
                        })?
                    );
                } else {
                    print_terms_audit_human(&report);
                }
                return Ok(ExitCode::SUCCESS);
            }
            let mut reports = Vec::new();
            for file in &args.files {
                let text = read_input(file)?;
                reports.push((file, terms::analyze(&text, &morphology)?));
            }
            if args.json {
                if reports.len() == 1 {
                    println!("{}", serde_json::to_string_pretty(&reports[0].1)?);
                } else {
                    let output = reports
                        .iter()
                        .map(|(file, report)| TermsOutput { file, report })
                        .collect::<Vec<_>>();
                    println!("{}", serde_json::to_string_pretty(&output)?);
                }
            } else {
                for (file, report) in reports {
                    print_terms_human(file, &report);
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub fn run() -> ExitCode {
    match execute(Cli::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}
