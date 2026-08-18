use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use suiko::evaluation::{self, SplitFilter, SweepRule};

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
        /// 対象split。holdoutは閾値確定後の一度きり評価にだけ使う
        #[arg(long, value_enum)]
        split: Option<SplitFilter>,
        /// sources.tomlの外部取得文書(ローカル、lock一致分)を含める
        #[arg(long)]
        external: bool,
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
    /// 正解ラベル付きサンプルからカテゴリ別の検出率と誤検知率を出す
    Labeled { manifest: PathBuf },
    /// 人間fprのWilson上限を制約に閾値を探索し、推奨値を出す(dev splitのみ)
    Calibrate {
        manifest: PathBuf,
        #[arg(long, value_enum)]
        rule: SweepRule,
        #[arg(
            long,
            value_delimiter = ',',
            required = true,
            allow_hyphen_values = true
        )]
        values: Vec<f64>,
        /// 人間文書fprのWilson 95%上限の許容値(必須。実行前に決めて記録する)
        #[arg(long)]
        max_human_fpr_upper: f64,
        /// sources.tomlの外部取得文書(ローカル、lock一致分)を含める
        #[arg(long)]
        external: bool,
        /// このid接頭辞の文書を除外する(例: 時代の異なる aozora-)
        #[arg(long)]
        exclude_id_prefix: Option<String>,
        #[arg(long)]
        experimental: bool,
    },
    /// 禁止語・誇張語彙の人間/AI出現実測と、AI側に偏る内容語の候補を出す
    Vocab {
        manifest: PathBuf,
        /// sources.tomlの外部取得文書(ローカル、lock一致分)を含める
        #[arg(long)]
        external: bool,
        /// このid接頭辞の文書を除外する(例: 時代の異なる aozora-)
        #[arg(long)]
        exclude_id_prefix: Option<String>,
    },
}

fn execute(cli: Cli) -> Result<String, evaluation::EvaluationError> {
    match cli.command {
        Command::Report {
            manifest,
            experimental,
            split,
            external,
        } => evaluation::report(&manifest, experimental, split, external),
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
        Command::Labeled { manifest } => evaluation::labeled(&manifest),
        Command::Calibrate {
            manifest,
            rule,
            values,
            max_human_fpr_upper,
            external,
            exclude_id_prefix,
            experimental,
        } => evaluation::calibrate(
            &manifest,
            rule,
            &values,
            max_human_fpr_upper,
            external,
            exclude_id_prefix.as_deref(),
            experimental,
        ),
        Command::Vocab {
            manifest,
            external,
            exclude_id_prefix,
        } => evaluation::vocab(&manifest, external, exclude_id_prefix.as_deref()),
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
