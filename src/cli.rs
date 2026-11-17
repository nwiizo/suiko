use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

use crate::morphology::Morphology;
use crate::{Error, academic, diff, lexical, outline, read_source, terms};

mod batch;
mod check;
mod config;
mod fix;
mod lint_command;
mod meaning;
mod style;

use lint_command::LintArgs;

/// v0.4.3から、評価中の規則（`lint::is_experimental_rule`）も含めて全規則を既定で表示する。
/// `--experimental`は互換のため受け付けるだけで、規則の確度は`first_fixes`の`confidence`で示す。
const ALL_RULES: bool = true;

/// `--experimental`を指定されたら、不要になったことを標準エラーへ1行で伝える。
fn note_experimental_flag(given: bool) {
    if given {
        eprintln!("注意: --experimental は不要になりました。v0.4.3から全規則を既定で表示します。");
    }
}

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
    /// 太字の表示を修正してプレビューする。--writeで保存する
    Fix(fix::FixArgs),
    /// 利用可能なコマンド・規則・出力形式を確認する
    Capabilities(batch::CapabilitiesArgs),
    /// Gitの指定した版と作業ツリーの指摘を同じ設定で比較する
    Compare(batch::CompareArgs),
    /// 指定した過去文書から文体の観測値を集計する
    StyleProfile(style::StyleArgs),
    /// 表現・反復・読解負荷を確認し、読み直す行と理由を示す
    Lint(LintArgs),
    /// 見出し・段落の先頭文・箇条書きで説明の順序を見直す
    Outline(FileArgs),
    /// 専門用語の初出説明と、複数ファイルの表記揺れを確認する
    Terms(TermsArgs),
    /// 一般名詞複合語、表記・レジスターの揺れを読み取り専用で監査する
    LexicalAudit(LexicalAuditArgs),
    /// 学術稿の論証・引用・Word/PDFを、執筆者が記録した方針と照合する
    Academic(AcademicArgs),
    /// 改稿前後の文書を文単位で対応づけ、数値・否定・文末の働き・条件・識別子の変化を候補として示す
    Diff(DiffArgs),
    /// 最終稿・原文・資料をまとめて検査し、提出前の照合用に記録する
    Check(check::CheckArgs),
    /// 検査記録と現在の本文・資料・設定・実行ファイルを照合する
    Verify(check::VerifyArgs),
    /// 精読で意味が定まらない表現を、指定したLLMのコマンドで確認する（任意・非決定的）
    Meaning(meaning::MeaningArgs),
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, ValueEnum)]
#[serde(rename_all = "lowercase")]
enum Genre {
    Essay,
    Tech,
    Business,
    /// 品詞・人称・数詞・文末の分布から随筆・技術・業務を推定して適用する
    Auto,
}

impl Genre {
    fn as_str(self) -> &'static str {
        match self {
            Self::Essay => "essay",
            Self::Tech => "tech",
            Self::Business => "business",
            Self::Auto => "auto",
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
struct LexicalAuditArgs {
    /// 対象の Markdown/テキストファイル。複数指定可
    #[arg(required = true)]
    files: Vec<String>,
    /// 固定コーパス頻度、登録語、レジスター集合を記したJSON参照資源
    #[arg(long)]
    reference: PathBuf,
    /// 機械可読なJSONで出力する
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct AcademicArgs {
    /// 監査するMarkdown原稿
    source: PathBuf,
    /// 中心命題、説明対象、用語来歴、章間接続、注分類を記したJSON方針ファイル
    #[arg(long)]
    contract: PathBuf,
    /// 同期とOOXML不変条件を確認するDOCX
    #[arg(long)]
    docx: Option<PathBuf>,
    /// Microsoft Wordから書き出した最終PDF
    #[arg(long)]
    pdf: Option<PathBuf>,
    /// 成果物の書式を照合する公式DOCXテンプレート
    #[arg(long, requires = "docx")]
    template: Option<PathBuf>,
    /// Word出力、PDF全頁目視、三成果物のSHA-256を記録したJSON
    #[arg(long, requires_all = ["docx", "pdf"])]
    export_record: Option<PathBuf>,
    /// 機械可読なJSONで出力する
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct DiffArgs {
    /// 改稿前の Markdown/テキストファイル。- で標準入力（片方だけ）
    before: String,
    /// 改稿後の Markdown/テキストファイル。- で標準入力（片方だけ）
    after: String,
    /// 機械可読な JSON で出力する
    #[arg(long)]
    json: bool,
    /// 指定 severity 以上の候補があれば終了コード2を返す
    #[arg(long, value_enum)]
    fail_on: Option<FailOn>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, ValueEnum)]
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

fn print_lexical_audit_human(report: &lexical::LexicalAuditReport) {
    println!("=== lexical audit: {}ファイル ===", report.files.len());
    println!("参照資源: {}", report.reference_source);
    println!(
        "候補 {}件。Suikoは本文を書き換えません。\n",
        report.findings.len()
    );
    for finding in &report.findings {
        println!(
            "[{}:{}] {} L{} {} — {}",
            finding.severity,
            finding.category,
            finding.file,
            finding.line,
            finding.term,
            finding.rationale
        );
    }
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

#[derive(Serialize)]
struct DiffInput<'a> {
    file: &'a str,
    sha256: String,
}

#[derive(Serialize)]
struct DiffOutput<'a> {
    suiko_version: &'static str,
    before: DiffInput<'a>,
    after: DiffInput<'a>,
    #[serde(flatten)]
    report: &'a diff::DiffReport,
}

fn print_diff_human(before: &str, after: &str, report: &diff::DiffReport) {
    let stats = &report.stats;
    println!("=== diff: {before} → {after} ===");
    println!(
        "文の単位: 変更前 {} / 変更後 {} / 一致 {} / 移動 {} / 変更領域 {}",
        stats.before_units,
        stats.after_units,
        stats.matched_units,
        stats.moved_units,
        stats.changed_regions
    );
    println!(
        "候補 {}件。意味が同じかは判定していません。候補のない変更領域も読み直してください。\n",
        stats.total_findings
    );
    for finding in &report.findings {
        let label = match finding.severity.as_str() {
            "info" => "情報",
            "warn" => "警告",
            other => other,
        };
        let position = match (finding.before_line, finding.after_line) {
            (Some(before), Some(after)) => format!("前L{before} → 後L{after}"),
            (Some(before), None) => format!("前L{before} → なし"),
            (None, Some(after)) => format!("なし → 後L{after}"),
            (None, None) => String::new(),
        };
        println!("[{label}] {position} ({})", finding.category);
        println!("    変更前: {}", finding.before_excerpt);
        println!("    変更後: {}", finding.after_excerpt);
        println!("    詳細  : {}\n", finding.detail);
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

fn print_academic_human(report: &academic::AcademicReport) {
    println!(
        "=== 原稿・指定成果物の監査: {} ===",
        if report.passed { "PASS" } else { "FAIL" }
    );
    println!(
        "提出準備完了: {}\n",
        if report.delivery_ready {
            "YES（自己申告のWord出力・PDF目視記録を含む）"
        } else {
            "NO（原稿PASSだけでは提出準備完了ではありません）"
        }
    );
    for item in &report.checks {
        println!("[{}] {}: {}", item.status, item.id, item.detail);
    }
    println!("\n=== 段落第一文と見出し ===\n");
    for entry in &report.first_sentences {
        println!("L{} {}: {}", entry.line, entry.kind, entry.text);
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
        Command::Fix(args) => return fix::execute(args),
        Command::Capabilities(args) => return batch::capabilities(args),
        Command::Compare(args) => return batch::compare(args),
        Command::StyleProfile(args) => return style::execute(args),
        Command::Check(args) => return check::execute_check(args),
        Command::Verify(args) => return check::execute_verify(args),
        Command::Meaning(args) => return meaning::execute(args),
        Command::Lint(args) => return lint_command::execute(args),
        Command::Diff(args) => {
            if args.before == "-" && args.after == "-" {
                return Err(Error::InvalidArguments(
                    "標準入力（-）は変更前と変更後の片方だけに指定できます".to_owned(),
                ));
            }
            let morphology = Morphology::new()?;
            let before = read_input(&args.before)?;
            let after = read_input(&args.after)?;
            let report = diff::compare(&before, &after, &morphology)?;
            if args.json {
                let output = DiffOutput {
                    suiko_version: env!("CARGO_PKG_VERSION"),
                    before: DiffInput {
                        file: &args.before,
                        sha256: diff::sha256_hex(&before),
                    },
                    after: DiffInput {
                        file: &args.after,
                        sha256: diff::sha256_hex(&after),
                    },
                    report: &report,
                };
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                print_diff_human(&args.before, &args.after, &report);
            }
            let failed = args.fail_on.is_some_and(|threshold| {
                report
                    .findings
                    .iter()
                    .any(|finding| threshold.matches(&finding.severity))
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
        Command::LexicalAudit(args) => {
            validate_inputs(&args.files)?;
            let morphology = Morphology::new()?;
            let reference =
                serde_json::from_str::<lexical::LexicalReference>(&read_source(&args.reference)?)?;
            let mut inputs = Vec::new();
            for file in &args.files {
                inputs.push((file.clone(), read_input(file)?));
            }
            let report = lexical::audit(&inputs, &morphology, &reference)?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_lexical_audit_human(&report);
            }
            return Ok(ExitCode::SUCCESS);
        }
        Command::Academic(args) => {
            let source = read_source(&args.source)?;
            let contract =
                serde_json::from_str::<academic::AcademicContract>(&read_source(&args.contract)?)?;
            let paths = academic::ArtifactPaths {
                source: &args.source,
                docx: args.docx.as_deref(),
                pdf: args.pdf.as_deref(),
                template: args.template.as_deref(),
                export_record: args.export_record.as_deref(),
            };
            let report = academic::audit(&source, &contract, &paths)?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_academic_human(&report);
            }
            return Ok(if report.passed {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(2)
            });
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
