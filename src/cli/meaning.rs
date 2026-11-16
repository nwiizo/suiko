use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use serde_json::json;

use crate::{Error, meaning, read_source};

#[derive(Debug, Args)]
pub(super) struct MeaningArgs {
    /// 対象の文書（1ファイル）
    file: PathBuf,
    /// 指摘と照合できなかった抜き出しをJSONで出力する
    #[arg(long)]
    json: bool,
    /// 依頼文と本文を標準入力で受け取り、JSONを標準出力へ返すコマンド（`--` の後に指定）
    #[arg(last = true, required = true, value_name = "COMMAND")]
    command: Vec<String>,
}

pub(super) fn execute(args: MeaningArgs) -> Result<ExitCode, Error> {
    let body = read_source(&args.file)?;
    let raw = meaning::request(&body, &args.command)?;
    let report = meaning::findings(&body, &raw)?;
    let file = args.file.display().to_string();
    if args.json {
        let output = json!({
            "file": file,
            "suiko_version": env!("CARGO_PKG_VERSION"),
            "command": args.command,
            "deterministic": false,
            "prompt_version": meaning::PROMPT_VERSION,
            "findings": report.findings,
            "discarded": report.discarded,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(ExitCode::SUCCESS);
    }
    println!(
        "{file}: LLMによる確認候補 {}件（{}）。実行ごとに結果が変わり得ます。",
        report.findings.len(),
        args.command.join(" ")
    );
    for finding in &report.findings {
        println!(
            "L{} [{}] {}",
            finding.line, finding.category, finding.excerpt
        );
        println!("    {}", finding.detail);
    }
    if !report.discarded.is_empty() {
        println!(
            "本文と照合できなかった抜き出し: {}件",
            report.discarded.len()
        );
        for item in &report.discarded {
            println!("  - {}（{}）", item.text, item.reason);
        }
    }
    Ok(ExitCode::SUCCESS)
}
