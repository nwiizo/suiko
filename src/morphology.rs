use std::sync::Arc;

use sudachi::analysis::stateless_tokenizer::StatelessTokenizer;
use sudachi::analysis::{Mode, Tokenize};
use sudachi::config::Config;
use sudachi::dic::dictionary::JapaneseDictionary;
use sudachi::dic::storage::{Storage, SudachiDicData};

use crate::Error;

// build.rs がSHA-256を検証した SudachiDict をバイナリへ埋め込む。
static SYSTEM_DICTIONARY: &[u8] = include_bytes!(env!("SUIKO_SUDACHI_DICT_FILE"));

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Morpheme {
    pub surface: String,
    pub byte_start: usize,
    pub byte_end: usize,
    pos: Vec<String>,
    dictionary_form: String,
    normalized: String,
    reading: String,
}

fn known(value: &str) -> Option<&str> {
    if value.is_empty() || value == "*" {
        None
    } else {
        Some(value)
    }
}

impl Morpheme {
    pub fn pos(&self, index: usize) -> &str {
        self.pos.get(index).map_or("", String::as_str)
    }

    pub fn reading(&self) -> &str {
        known(&self.reading).unwrap_or(self.surface.as_str())
    }

    pub fn dictionary_form(&self) -> &str {
        known(&self.dictionary_form).unwrap_or(self.surface.as_str())
    }

    /// SudachiDictの表記統制に基づく正規化表記。異表記(サーバ/サーバー等)が
    /// 同じ値になる。未知語は表層をそのまま返す。
    pub fn normalized(&self) -> &str {
        known(&self.normalized).unwrap_or(self.surface.as_str())
    }
}

pub struct Morphology {
    tokenizer: StatelessTokenizer<Arc<JapaneseDictionary>>,
}

impl Morphology {
    pub fn new() -> Result<Self, Error> {
        let config =
            Config::new_embedded().map_err(|error| Error::Morphology(error.to_string()))?;
        let data = SudachiDicData::new(Storage::Borrowed(SYSTEM_DICTIONARY));
        let dictionary = JapaneseDictionary::from_cfg_storage_with_embedded_chardef(&config, data)
            .map_err(|error| Error::Morphology(error.to_string()))?;
        Ok(Self {
            tokenizer: StatelessTokenizer::new(Arc::new(dictionary)),
        })
    }

    pub fn tokenize(&self, text: &str) -> Result<Vec<Morpheme>, Error> {
        if text.is_empty() {
            return Ok(Vec::new());
        }
        let morphemes = self
            .tokenizer
            .tokenize(text, Mode::C, false)
            .map_err(|error| Error::Morphology(error.to_string()))?;
        Ok(morphemes
            .iter()
            .map(|morpheme| Morpheme {
                surface: morpheme.surface().to_string(),
                byte_start: morpheme.begin(),
                byte_end: morpheme.end(),
                pos: morpheme.part_of_speech().to_vec(),
                dictionary_form: morpheme.dictionary_form().to_owned(),
                normalized: morpheme.normalized_form().to_owned(),
                reading: morpheme.reading_form().to_owned(),
            })
            .collect())
    }
}
