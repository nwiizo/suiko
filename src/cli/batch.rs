//! 機能確認、文書群の集計、Gitの版との読み取り専用の比較。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode};

use clap::{Args, CommandFactory, ValueEnum};
use serde::Serialize;
use serde_json::json;

use super::{Cli, FailOn, Genre, LintRun, apply_reading_load_config, configured_lint, load_config};
use crate::{Error, lint, morphology::Morphology, read_source};

#[derive(Debug, Args)]
pub(super) struct CapabilitiesArgs {
    #[arg(long)]
    json: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(super) enum SummaryFormat {
    Json,
    Tsv,
}

pub(super) fn capabilities(args: CapabilitiesArgs) -> Result<ExitCode, Error> {
    let commands = Cli::command().get_subcommands().map(|command| {
        json!({"name": command.get_name(), "options": command.get_arguments().filter_map(|a| a.get_long()).collect::<Vec<_>>()})
    }).collect::<Vec<_>>();
    let rules = lint::rule_categories().iter().map(|category| json!({
        "category": category,
        "lane": if lint::reading_load_categories().contains(category) { "reading_load" } else { "natural" },
        "experimental": lint::is_experimental_rule(category)
    })).collect::<Vec<_>>();
    let result = json!({"schema_version": 1, "suiko_version": env!("CARGO_PKG_VERSION"), "commands": commands, "rules": rules, "check_record": {"format": super::check::RECORD_FORMAT, "schema_version": super::check::RECORD_SCHEMA_VERSION}});
    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("suiko {}", env!("CARGO_PKG_VERSION"));
        for command in Cli::command().get_subcommands() {
            println!("  {}", command.get_name());
        }
        println!("規則: {}件 / 検査記録: suiko-check v1", rules.len());
    }
    Ok(ExitCode::SUCCESS)
}

#[derive(Default, Serialize)]
struct Counts {
    natural: usize,
    reading_load: usize,
}

pub(super) fn tsv(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

pub(super) fn print_summary(runs: &[LintRun], format: SummaryFormat) -> Result<(), Error> {
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

#[derive(Debug, Args)]
pub(super) struct CompareArgs {
    #[arg(long)]
    rev: String,
    #[arg(required = true)]
    files: Vec<PathBuf>,
    #[arg(long, conflicts_with = "summary")]
    json: bool,
    #[arg(long, value_enum)]
    summary: Option<SummaryFormat>,
    #[arg(long, value_enum)]
    genre: Option<Genre>,
    #[arg(long)]
    experimental: bool,
    #[arg(long, value_enum)]
    fail_on: Option<FailOn>,
    #[arg(long, conflicts_with = "no_config")]
    config: Option<PathBuf>,
    #[arg(long, conflicts_with = "config")]
    no_config: bool,
}

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidArguments(message.into())
}

fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, Error> {
    let output = Command::new("git")
        .arg("--no-pager")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| invalid(format!("Gitを実行できません: {e}")))?;
    if !output.status.success() {
        return Err(invalid(format!(
            "Gitの読み取りに失敗しました: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output.stdout)
}

fn utf8(bytes: Vec<u8>) -> Result<String, Error> {
    String::from_utf8(bytes).map_err(|_| invalid("Gitの出力はUTF-8である必要があります"))
}

fn relative_path(root: &Path, cwd: &Path, path: &Path) -> Result<String, Error> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        cwd.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component),
        }
    }
    let relative = normalized
        .strip_prefix(root)
        .map_err(|_| invalid("比較するファイルはGitリポジトリ内で指定してください"))?;
    // Gitのツリー内のパスはWindowsでもスラッシュ区切り。
    relative
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()
        .filter(|parts| !parts.is_empty())
        .map(|parts| parts.join("/"))
        .ok_or_else(|| invalid("UTF-8のファイルパスを指定してください"))
}

fn renames(output: &str) -> BTreeMap<String, String> {
    let mut fields = output.split_terminator('\0');
    let mut result = BTreeMap::new();
    while let Some(status) = fields.next() {
        let Some(before) = fields.next() else {
            break;
        };
        if status.starts_with(['R', 'C']) {
            let Some(after) = fields.next() else {
                break;
            };
            if status.starts_with('R') {
                result.insert(after.to_owned(), before.to_owned());
            }
        }
    }
    result
}

#[derive(Default, Serialize)]
struct ChangeCounts {
    before: usize,
    after: usize,
    resolved: usize,
    new: usize,
    persisting: usize,
}

impl ChangeCounts {
    fn add(&mut self, other: &Self) {
        self.before += other.before;
        self.after += other.after;
        self.resolved += other.resolved;
        self.new += other.new;
        self.persisting += other.persisting;
    }
}

#[derive(Default, Serialize)]
struct Lanes {
    natural: ChangeCounts,
    reading_load: ChangeCounts,
}

fn changes(
    before: &[lint::Finding],
    after: &mut [lint::Finding],
    categories: &mut BTreeMap<String, Lanes>,
    reading: bool,
) -> Result<ChangeCounts, Error> {
    let baseline = lint::apply_baseline(after, &json!({"findings": before}), String::new())
        .map_err(invalid)?;
    for finding in before {
        let lanes = categories.entry(finding.category.clone()).or_default();
        let counts = if reading {
            &mut lanes.reading_load
        } else {
            &mut lanes.natural
        };
        counts.before += 1;
    }
    for finding in after.iter() {
        let lanes = categories.entry(finding.category.clone()).or_default();
        let counts = if reading {
            &mut lanes.reading_load
        } else {
            &mut lanes.natural
        };
        counts.after += 1;
        if finding.status.as_deref() == Some("persisting") {
            counts.persisting += 1;
        } else {
            counts.new += 1;
        }
    }
    for finding in &baseline.resolved {
        let lanes = categories
            .entry(finding["category"].as_str().unwrap_or_default().to_owned())
            .or_default();
        let counts = if reading {
            &mut lanes.reading_load
        } else {
            &mut lanes.natural
        };
        counts.resolved += 1;
    }
    Ok(ChangeCounts {
        before: before.len(),
        after: after.len(),
        resolved: baseline.summary.resolved,
        new: baseline.summary.new,
        persisting: baseline.summary.persisting,
    })
}

pub(super) fn compare(args: CompareArgs) -> Result<ExitCode, Error> {
    let cwd = std::env::current_dir().map_err(|e| invalid(e.to_string()))?;
    let root_output = utf8(git(&cwd, &["rev-parse", "--show-toplevel"])?)?;
    let root = PathBuf::from(root_output.strip_suffix('\n').unwrap_or(&root_output));
    let canonical_root = root.canonicalize().map_err(|e| invalid(e.to_string()))?;
    let rev = utf8(git(
        &root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{}^{{commit}}", args.rev),
        ],
    )?)?
    .trim()
    .to_owned();
    let tree = utf8(git(&root, &["ls-tree", "-r", "-z", &rev])?)?;
    let tracked = tree
        .split_terminator('\0')
        .map(|entry| {
            let (metadata, path) = entry
                .split_once('\t')
                .ok_or_else(|| invalid("Gitのツリー情報を読めません"))?;
            Ok((path, metadata.split(' ').next().unwrap_or_default()))
        })
        .collect::<Result<BTreeMap<_, _>, Error>>()?;
    let mut renamed = renames(&utf8(git(
        &root,
        &[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--cached",
            "--name-status",
            "-z",
            "--find-renames",
            &rev,
            "--",
        ],
    )?)?);
    renamed.extend(renames(&utf8(git(
        &root,
        &[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--name-status",
            "-z",
            "--find-renames",
            &rev,
            "--",
        ],
    )?)?));
    let config = load_config(args.config.as_deref(), args.no_config)?;
    let genre = args
        .genre
        .or_else(|| config.as_ref().and_then(|c| c.genre))
        .map(Genre::as_str);
    let fail_on = args
        .fail_on
        .or_else(|| config.as_ref().and_then(|c| c.fail_on));
    let morphology = Morphology::new()?;
    let mut files = Vec::new();
    let mut categories = BTreeMap::new();
    let mut total = Lanes::default();
    let mut seen = BTreeSet::new();
    let mut failed = false;
    for path in args.files {
        let selected = relative_path(&root, &cwd, &path)?;
        let file = if root
            .join(&selected)
            .try_exists()
            .map_err(|e| invalid(e.to_string()))?
        {
            selected.clone()
        } else {
            renamed
                .iter()
                .find(|(_, old)| **old == selected)
                .map_or(selected.clone(), |(new, _)| new.clone())
        };
        if !seen.insert(file.clone()) {
            continue;
        }
        let before_file = renamed.get(&file).unwrap_or(&file);
        let before_mode = tracked.get(before_file.as_str());
        let before_exists = before_mode.is_some();
        let final_path = root.join(&file);
        if before_mode.is_some_and(|mode| !matches!(*mode, "100644" | "100755"))
            || final_path
                .symlink_metadata()
                .is_ok_and(|metadata| metadata.is_symlink())
        {
            return Err(invalid(
                "比較には通常のファイルを指定してください。シンボリックリンクやサブモジュールは対象外です",
            ));
        }
        let after_exists = final_path
            .try_exists()
            .map_err(|e| invalid(e.to_string()))?;
        if !before_exists && !after_exists {
            return Err(invalid(format!(
                "指定した版にも作業ツリーにもファイルがありません: {file}"
            )));
        }
        if after_exists
            && final_path
                .canonicalize()
                .map_err(|e| invalid(e.to_string()))?
                .strip_prefix(&canonical_root)
                .is_err()
        {
            return Err(invalid(
                "リポジトリ外へのシンボリックリンクは比較できません",
            ));
        }
        let before_text = if before_exists {
            utf8(git(&root, &["show", &format!("{rev}:{before_file}")])?)?
        } else {
            String::new()
        };
        let after_text = if after_exists {
            read_source(&final_path)?
        } else {
            String::new()
        };
        let before = configured_lint(
            &before_text,
            &morphology,
            genre,
            args.experimental,
            config.as_ref(),
        )?;
        let mut after = configured_lint(
            &after_text,
            &morphology,
            genre,
            args.experimental,
            config.as_ref(),
        )?;
        let mut before_reading = lint::analyze_reading_load(&before_text, &morphology, genre)?;
        let mut after_reading = lint::analyze_reading_load(&after_text, &morphology, genre)?;
        apply_reading_load_config(&mut before_reading, config.as_ref());
        apply_reading_load_config(&mut after_reading, config.as_ref());
        let natural = changes(
            &before.findings,
            &mut after.findings,
            &mut categories,
            false,
        )?;
        let reading_load = changes(
            &before_reading.findings,
            &mut after_reading.findings,
            &mut categories,
            true,
        )?;
        failed |= fail_on.is_some_and(|threshold| {
            after
                .findings
                .iter()
                .chain(&after_reading.findings)
                .any(|f| threshold.matches(&f.severity))
        });
        total.natural.add(&natural);
        total.reading_load.add(&reading_load);
        let status = if !before_exists {
            "added"
        } else if !after_exists {
            "deleted"
        } else if before_file != &file {
            "renamed"
        } else {
            "matched"
        };
        files.push(json!({"file": file, "before_file": before_exists.then_some(before_file), "file_status": status, "natural": natural, "reading_load": reading_load}));
    }
    if args.json || matches!(args.summary, Some(SummaryFormat::Json)) {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &json!({"schema_version": 1, "suiko_version": env!("CARGO_PKG_VERSION"), "revision": rev, "genre": genre, "experimental": args.experimental, "files": files, "total": total, "categories": categories.iter().map(|(category, lanes)| json!({"category": category, "natural": lanes.natural, "reading_load": lanes.reading_load})).collect::<Vec<_>>()})
            )?
        );
    } else {
        println!("file\tnatural_before\tnatural_after\treading_load_before\treading_load_after");
        for file in files {
            println!(
                "{}\t{}\t{}\t{}\t{}",
                tsv(file["file"].as_str().unwrap_or_default()),
                file["natural"]["before"],
                file["natural"]["after"],
                file["reading_load"]["before"],
                file["reading_load"]["after"]
            );
        }
        if args.summary.is_none() {
            eprintln!(
                "合計: 自然さ {} → {} / 読解負荷 {} → {}。件数の減少は文章品質の判定ではありません。",
                total.natural.before,
                total.natural.after,
                total.reading_load.before,
                total.reading_load.after
            );
        }
    }
    Ok(if failed {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    })
}
