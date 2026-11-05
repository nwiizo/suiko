use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;

use crate::{Error, markup, read_source};

#[derive(Debug, Args)]
pub(super) struct FixArgs {
    /// 対象のMarkdownファイル（1ファイル）
    file: PathBuf,
    /// 表示されない太字のうち、開閉が明確な箇所を修正する
    #[arg(long, required = true)]
    bold: bool,
    /// プレビューの代わりに対象ファイルへ保存する
    #[arg(long)]
    write: bool,
}

pub(super) fn execute(args: FixArgs) -> Result<ExitCode, Error> {
    let fail = |error: std::io::Error| {
        Error::InvalidArguments(format!(
            "書式修正を保存できません: {} ({error})",
            args.file.display()
        ))
    };
    let metadata = fs::symlink_metadata(&args.file).map_err(&fail)?;
    if !metadata.is_file() || (args.write && metadata.permissions().readonly()) {
        return Err(Error::InvalidArguments(
            "通常の書き込み可能なファイルを指定してください。リンクは保存対象にできません。".into(),
        ));
    }
    #[cfg(unix)]
    if args.write {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(Error::InvalidArguments(
                "ハードリンクを持つファイルは保存対象にできません。".into(),
            ));
        }
    }
    let before = read_source(&args.file)?;
    let after = markup::fix_bold(&before);
    if !args.write {
        std::io::stdout()
            .lock()
            .write_all(after.as_bytes())
            .map_err(&fail)?;
    } else if before != after {
        let parent = args
            .file
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(&fail)?;
        temporary.write_all(after.as_bytes()).map_err(&fail)?;
        temporary
            .as_file()
            .set_permissions(metadata.permissions())
            .map_err(&fail)?;
        temporary.as_file().sync_all().map_err(&fail)?;
        // 一時ファイルの準備中に原文が変わっていれば上書きを中止する。
        if !fs::symlink_metadata(&args.file).map_err(&fail)?.is_file()
            || read_source(&args.file)? != before
        {
            return Err(Error::InvalidArguments(
                "処理中に原文が変わったため保存を中止しました。".into(),
            ));
        }
        temporary
            .persist(&args.file)
            .map_err(|error| fail(error.error))?;
    }
    Ok(ExitCode::SUCCESS)
}
