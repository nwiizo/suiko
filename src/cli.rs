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
    Terms(FileArgs),
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
    /// 指定した設定ファイルを使用する（自動検出より優先）
    #[arg(long, value_name = "PATH", conflicts_with = "no_config")]
    config: Option<PathBuf>,
    /// カレントディレクトリの .suiko.toml を読み込まない
    #[arg(long, conflicts_with = "config")]
    no_config: bool,
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

fn apply_config(report: &mut lint::LintReport, config: Option<&Config>) {
    let Some(config) = config else {
        return;
    };
    report
        .findings
        .retain(|finding| !config.suppresses(finding));
    report.stats.total_findings = report.findings.len();
    report.stats.by_category = category_counts(&report.findings);
}

fn apply_reading_load_config(report: &mut lint::ReadingLoadReport, config: Option<&Config>) {
    let Some(config) = config else {
        return;
    };
    report
        .findings
        .retain(|finding| !config.suppresses(finding));
    report.stats.total = report.findings.len();
    report.stats.by_category = category_counts(&report.findings);
}

#[derive(Serialize)]
struct LintOutput<'a> {
    file: &'a str,
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
            if args.baseline.is_some() && args.files.len() != 1 {
                return Err(Error::InvalidArguments(
                    "--baseline は1ファイルの lint でのみ使用できます".to_owned(),
                ));
            }
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
            let mut runs = Vec::new();
            for file in &args.files {
                let text = read_input(file)?;
                let mut report = lint::analyze(&text, &morphology, genre, args.experimental)?;
                apply_config(&mut report, config.as_ref());
                let baseline = if let Some((baseline_file, data)) = &baseline_data {
                    match lint::apply_baseline(&mut report.findings, data, baseline_file.clone()) {
                        Ok(report) => Some(report),
                        Err(warning) => {
                            eprintln!("警告: {warning}");
                            None
                        }
                    }
                } else {
                    None
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
            if args.json {
                let output = runs
                    .iter()
                    .map(|run| LintOutput {
                        file: &run.file,
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
