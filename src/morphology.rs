use std::borrow::Cow;

use lindera::dictionary::load_dictionary;
use lindera::mode::Mode;
use lindera::segmenter::Segmenter;

use crate::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Morpheme {
    pub surface: String,
    pub byte_start: usize,
    pub byte_end: usize,
    pub details: Vec<String>,
}

impl Morpheme {
    pub fn pos(&self, index: usize) -> &str {
        self.details.get(index).map_or("", String::as_str)
    }

    pub fn reading(&self) -> &str {
        self.details
            .get(7)
            .map_or(self.surface.as_str(), String::as_str)
    }

    pub fn dictionary_form(&self) -> &str {
        self.details
            .get(6)
            .map_or(self.surface.as_str(), String::as_str)
    }
}

pub struct Morphology {
    segmenter: Segmenter,
}

impl Morphology {
    pub fn new() -> Result<Self, Error> {
        let dictionary = load_dictionary("embedded://ipadic")
            .map_err(|error| Error::Morphology(error.to_string()))?;
        Ok(Self {
            segmenter: Segmenter::new(Mode::Normal, dictionary, None),
        })
    }

    pub fn tokenize(&self, text: &str) -> Result<Vec<Morpheme>, Error> {
        let mut tokens = self
            .segmenter
            .segment(Cow::Borrowed(text))
            .map_err(|error| Error::Morphology(error.to_string()))?;
        Ok(tokens
            .iter_mut()
            .map(|token| Morpheme {
                surface: token.surface.to_string(),
                byte_start: token.byte_start,
                byte_end: token.byte_end,
                details: token.details().into_iter().map(str::to_owned).collect(),
            })
            .collect())
    }
}
