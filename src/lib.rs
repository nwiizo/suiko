pub mod cli;
#[cfg(feature = "evaluation")]
pub mod evaluation;
pub mod lint;
pub mod morphology;
pub mod outline;
pub mod terms;
mod text;

use std::path::Path;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("エラー: ファイルが見つかりません: {0}")]
    NotFound(String),
    #[error("エラー: ファイルではありません: {0}")]
    NotAFile(String),
    #[error("エラー: ファイルを読み込めません: {path} ({source})")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("エラー: 形態素解析器を初期化できません: {0}")]
    Morphology(String),
    #[error("エラー: JSONを処理できません: {0}")]
    Json(#[from] serde_json::Error),
    #[error("エラー: 設定ファイルを処理できません: {path} ({message})")]
    Config { path: String, message: String },
    #[error("エラー: {0}")]
    InvalidArguments(String),
}

pub fn read_source(path: &Path) -> Result<String, Error> {
    if !path.exists() {
        return Err(Error::NotFound(path.display().to_string()));
    }
    if !path.is_file() {
        return Err(Error::NotAFile(path.display().to_string()));
    }
    std::fs::read_to_string(path).map_err(|source| Error::Read {
        path: path.display().to_string(),
        source,
    })
}
