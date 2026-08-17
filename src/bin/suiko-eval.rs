use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use suiko::evaluation::{self, SweepRule};

#[derive(Debug, Parser)]
#[command(
    name = "suiko-eval",
    version,
    about = "Suikoの検出器を再現可能な評価集合で校正する"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// カテゴリ別の文書発火率とfinding件数を表示する
    Report {
        manifest: PathBuf,
        #[arg(long)]
        experimental: bool,
    },
    /// 選択した検出器の閾値候補を比較する
    Sweep {
        manifest: PathBuf,
        #[arg(long, value_enum)]
        rule: SweepRule,
        #[arg(long, value_delimiter = ',', required = true)]
        values: Vec<f64>,
        #[arg(long)]
        experimental: bool,
    },
    /// 文書長別に文書数とfinding件数を表示する
    LengthAnalysis {
        manifest: PathBuf,
        #[arg(long)]
        experimental: bool,
    },
}

fn execute(cli: Cli) -> Result<String, evaluation::EvaluationError> {
    match cli.command {
        Command::Report {
            manifest,
            experimental,
        } => evaluation::report(&manifest, experimental),
        Command::Sweep {
            manifest,
            rule,
            values,
            experimental,
        } => evaluation::sweep(&manifest, rule, &values, experimental),
        Command::LengthAnalysis {
            manifest,
            experimental,
        } => evaluation::length_analysis(&manifest, experimental),
    }
}

fn main() -> ExitCode {
    match execute(Cli::parse()) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("エラー: {error}");
            ExitCode::from(1)
        }
    }
}
