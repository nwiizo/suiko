//! 著者が指定した文書の観測値。文体からの差を誤りとは判定しない。

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Error, diff, morphology::Morphology, read_source, text};

#[derive(Debug, Args)]
pub(super) struct StyleArgs {
    #[arg(required = true)]
    files: Vec<PathBuf>,
    /// style-profile --jsonで保存した観測値との差を示す
    #[arg(long)]
    against: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct Observations {
    sentences: usize,
    polite_sentences: usize,
    sentence_length_mean: f64,
    sentence_leads: BTreeMap<String, usize>,
    sentence_endings: BTreeMap<String, usize>,
    vocabulary: BTreeMap<String, usize>,
}

#[derive(Serialize, Deserialize)]
struct Profile {
    format: String,
    schema_version: u32,
    suiko_version: String,
    sources: Vec<Value>,
    observations: Observations,
    #[serde(skip_serializing_if = "Option::is_none")]
    comparison: Option<Value>,
}

pub(super) fn execute(args: StyleArgs) -> Result<ExitCode, Error> {
    let morphology = Morphology::new()?;
    let mut observations = Observations::default();
    let mut sources = Vec::new();
    let mut seen = BTreeSet::new();
    let mut characters = 0;
    for file in args.files {
        let source = read_source(&file)?;
        let canonical = file
            .canonicalize()
            .map_err(|e| Error::InvalidArguments(e.to_string()))?;
        if !seen.insert(canonical.clone()) {
            continue;
        }
        sources.push(json!({"path": canonical, "sha256": diff::sha256_hex(&source)}));
        let (prepared, _) = text::prepare_lint_text(&source);
        for sentence in text::sentences(&text::mask_markdown_structure(&prepared)) {
            let tokens = morphology.tokenize(&sentence.text)?;
            let significant = tokens
                .iter()
                .filter(|t| !matches!(t.pos(0), "補助記号" | "記号" | "空白"))
                .collect::<Vec<_>>();
            if significant.is_empty() {
                continue;
            }
            observations.sentences += 1;
            characters += sentence.text.chars().filter(|c| !c.is_whitespace()).count();
            observations.polite_sentences += usize::from(
                significant
                    .iter()
                    .rev()
                    .take(3)
                    .any(|t| matches!(t.dictionary_form(), "です" | "ます")),
            );
            let lead = significant
                .iter()
                .take(2)
                .map(|t| t.surface.as_str())
                .collect::<String>();
            let ending = significant
                .iter()
                .rev()
                .take(3)
                .rev()
                .map(|t| t.surface.as_str())
                .collect::<String>();
            *observations.sentence_leads.entry(lead).or_default() += 1;
            *observations.sentence_endings.entry(ending).or_default() += 1;
            for token in &tokens {
                if matches!(token.pos(0), "名詞" | "動詞" | "形容詞" | "副詞")
                    && !matches!(
                        token.dictionary_form(),
                        "する" | "ある" | "いる" | "こと" | "もの"
                    )
                {
                    *observations
                        .vocabulary
                        .entry(token.normalized().to_owned())
                        .or_default() += 1;
                }
            }
        }
    }
    if observations.sentences == 0 {
        return Err(Error::InvalidArguments(
            "文体を観測できる本文がありません".to_owned(),
        ));
    }
    observations.sentence_length_mean = characters as f64 / observations.sentences as f64;
    let comparison = if let Some(path) = args.against {
        let reference: Profile = serde_json::from_str(&read_source(&path)?)?;
        if reference.format != "suiko-style-profile"
            || reference.schema_version != 1
            || reference.observations.sentences == 0
            || reference.observations.polite_sentences > reference.observations.sentences
        {
            return Err(Error::InvalidArguments(
                "文体プロファイルの形式または集計値が不正です".to_owned(),
            ));
        }
        Some(
            json!({"reference": path, "sentence_length_mean_delta": observations.sentence_length_mean - reference.observations.sentence_length_mean, "polite_ratio_delta": observations.polite_sentences as f64 / observations.sentences as f64 - reference.observations.polite_sentences as f64 / reference.observations.sentences as f64, "interpretation": "文体の差を示す観測値です。修正の必要性は判定しません。"}),
        )
    } else {
        None
    };
    let profile = Profile {
        format: "suiko-style-profile".to_owned(),
        schema_version: 1,
        suiko_version: env!("CARGO_PKG_VERSION").to_owned(),
        sources,
        observations,
        comparison,
    };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&profile)?);
    } else {
        println!(
            "{}文書・{}文 / 平均{:.1}字 / 敬体{:.1}%",
            profile.sources.len(),
            profile.observations.sentences,
            profile.observations.sentence_length_mean,
            100.0 * profile.observations.polite_sentences as f64
                / profile.observations.sentences as f64
        );
        if let Some(comparison) = profile.comparison {
            println!("{}", serde_json::to_string_pretty(&comparison)?);
        }
        println!("文体の観測値です。傾向から外れることを誤りとは判定しません。");
    }
    Ok(ExitCode::SUCCESS)
}
