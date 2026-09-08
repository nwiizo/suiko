use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::Error;
use crate::lint::forbidden_phrase_list;
use crate::morphology::{Morpheme, Morphology};
use crate::text::{mask_html_comments, mask_markdown_structure};

const GLOSS_MARKERS: &[&str] = &["とは", "と呼ぶ", "という", "を指す", "と定義"];

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LexicalReference {
    pub version: u32,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub known_compounds: BTreeSet<String>,
    #[serde(default)]
    pub corpus_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub register_sets: Vec<RegisterSet>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterSet {
    pub id: String,
    pub terms: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LexicalComponent {
    pub surface: String,
    pub dictionary_form: String,
    pub normalized: String,
    pub reading: String,
    pub pos: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct LexicalFinding {
    pub term: String,
    pub components: Vec<LexicalComponent>,
    pub file: String,
    pub line: usize,
    pub context: String,
    pub category: String,
    pub rationale: String,
    pub document_frequency: usize,
    pub corpus_frequency: Option<usize>,
    pub severity: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct LexicalAuditReport {
    pub files: Vec<String>,
    pub reference_source: String,
    pub findings: Vec<LexicalFinding>,
}

#[derive(Clone, Debug)]
struct Candidate {
    term: String,
    components: Vec<LexicalComponent>,
    file: String,
    line: usize,
    context: String,
    normalized: String,
    count: usize,
    has_gloss: bool,
}

impl LexicalReference {
    pub fn validate(&self) -> Result<(), Error> {
        if self.version != 1 {
            return Err(Error::InvalidArguments(format!(
                "lexical reference version {} は未対応です",
                self.version
            )));
        }
        if self
            .register_sets
            .iter()
            .any(|set| set.id.trim().is_empty() || set.terms.len() < 2)
        {
            return Err(Error::InvalidArguments(
                "register_setsにはidと2語以上のtermsが必要です".to_owned(),
            ));
        }
        Ok(())
    }
}

fn compound_component(token: &Morpheme) -> bool {
    (token.pos(0) == "名詞" && token.pos(1) == "普通名詞")
        || token.pos(0) == "接尾辞"
        || token.pos(0) == "形状詞"
}

fn valid_surface(surface: &str) -> bool {
    let count = surface.chars().count();
    (2..=20).contains(&count)
        && surface.chars().all(|ch| {
            matches!(
                ch,
                '一'..='龠' | '々' | 'ヶ' | 'ヵ' | 'ぁ'..='ゖ' | 'ァ'..='ヺ' | 'ー'
            )
        })
}

fn component(token: &Morpheme) -> LexicalComponent {
    LexicalComponent {
        surface: token.surface.clone(),
        dictionary_form: token.dictionary_form().to_owned(),
        normalized: token.normalized().to_owned(),
        reading: token.reading().to_owned(),
        pos: format!("{}-{}-{}", token.pos(0), token.pos(1), token.pos(2)),
    }
}

fn extract_candidates(
    file: &str,
    raw: &str,
    morphology: &Morphology,
) -> Result<Vec<Candidate>, Error> {
    let comments = mask_html_comments(raw);
    let masked = mask_markdown_structure(&comments);
    let raw_lines = raw.lines().collect::<Vec<_>>();
    let mut candidates = BTreeMap::<String, Candidate>::new();
    for (line_index, line) in masked.lines().enumerate() {
        let tokens = morphology.tokenize(line)?;
        let mut index = 0;
        while index < tokens.len() {
            if !compound_component(&tokens[index]) || tokens[index].pos(0) == "接尾辞" {
                index += 1;
                continue;
            }
            let mut end = index + 1;
            while end < tokens.len()
                && compound_component(&tokens[end])
                && tokens[end - 1].byte_end == tokens[end].byte_start
                && end - index < 4
            {
                end += 1;
            }
            if end - index >= 2 {
                let start = tokens[index].byte_start;
                let finish = tokens[end - 1].byte_end;
                let term = &line[start..finish];
                if valid_surface(term) {
                    let original = raw_lines.get(line_index).copied().unwrap_or(line).trim();
                    let context = original.chars().take(160).collect::<String>();
                    let has_gloss = GLOSS_MARKERS
                        .iter()
                        .any(|marker| line.contains(&format!("{term}{marker}")));
                    let parts = tokens[index..end].iter().map(component).collect::<Vec<_>>();
                    let normalized = parts
                        .iter()
                        .map(|part| part.normalized.as_str())
                        .collect::<String>();
                    candidates
                        .entry(term.to_owned())
                        .and_modify(|candidate| {
                            candidate.count += 1;
                            candidate.has_gloss |= has_gloss;
                        })
                        .or_insert(Candidate {
                            term: term.to_owned(),
                            components: parts,
                            file: file.to_owned(),
                            line: line_index + 1,
                            context,
                            normalized,
                            count: 1,
                            has_gloss,
                        });
                }
            }
            index = end.max(index + 1);
        }
    }
    Ok(candidates.into_values().collect())
}

fn finding(
    candidate: &Candidate,
    category: &str,
    rationale: String,
    corpus_frequency: Option<usize>,
) -> LexicalFinding {
    LexicalFinding {
        term: candidate.term.clone(),
        components: candidate.components.clone(),
        file: candidate.file.clone(),
        line: candidate.line,
        context: candidate.context.clone(),
        category: category.to_owned(),
        rationale,
        document_frequency: candidate.count,
        corpus_frequency,
        severity: "info".to_owned(),
    }
}

pub fn audit(
    inputs: &[(String, String)],
    morphology: &Morphology,
    reference: &LexicalReference,
) -> Result<LexicalAuditReport, Error> {
    reference.validate()?;
    let mut candidates = Vec::new();
    for (file, text) in inputs {
        candidates.extend(extract_candidates(file, text, morphology)?);
    }
    let mut findings = Vec::new();

    for (file, raw) in inputs {
        let visible = mask_markdown_structure(&mask_html_comments(raw));
        for phrase in forbidden_phrase_list() {
            for (line_index, line) in visible.lines().enumerate() {
                if line.contains(phrase) {
                    findings.push(LexicalFinding {
                        term: (*phrase).to_owned(),
                        components: morphology.tokenize(phrase)?.iter().map(component).collect(),
                        file: file.clone(),
                        line: line_index + 1,
                        context: raw
                            .lines()
                            .nth(line_index)
                            .unwrap_or(line)
                            .trim()
                            .chars()
                            .take(160)
                            .collect(),
                        category: "forbidden_match".to_owned(),
                        rationale: "既存lintの禁止語リストに完全一致".to_owned(),
                        document_frequency: visible.matches(phrase).count(),
                        corpus_frequency: None,
                        severity: "warn".to_owned(),
                    });
                    break;
                }
            }
        }
    }

    let mut normalized_groups = BTreeMap::<String, Vec<&Candidate>>::new();
    for candidate in &candidates {
        normalized_groups
            .entry(candidate.normalized.clone())
            .or_default()
            .push(candidate);
    }
    for group in normalized_groups.values() {
        let spellings = group
            .iter()
            .map(|candidate| candidate.term.as_str())
            .collect::<BTreeSet<_>>();
        if spellings.len() > 1 {
            let detail = spellings.into_iter().collect::<Vec<_>>().join(" / ");
            for candidate in group {
                findings.push(finding(
                    candidate,
                    "orthographic_variation",
                    format!("Sudachi正規形が同一: {detail}"),
                    reference.corpus_counts.get(&candidate.term).copied(),
                ));
            }
        }
    }

    let visible_inputs = inputs
        .iter()
        .map(|(file, text)| (file, mask_markdown_structure(&mask_html_comments(text))))
        .collect::<Vec<_>>();
    for set in &reference.register_sets {
        let present = set
            .terms
            .iter()
            .filter(|term| {
                visible_inputs
                    .iter()
                    .any(|(_, text)| text.contains(term.as_str()))
            })
            .collect::<Vec<_>>();
        if present.len() >= 2 {
            let detail = present
                .iter()
                .map(|term| term.as_str())
                .collect::<Vec<_>>()
                .join(" / ");
            for term in present {
                if let Some(candidate) = candidates.iter().find(|candidate| candidate.term == *term)
                {
                    findings.push(finding(
                        candidate,
                        "register_variation",
                        format!("参照資源のレジスター集合 {} で共起: {detail}", set.id),
                        reference.corpus_counts.get(&candidate.term).copied(),
                    ));
                } else if let Some((input_index, (file, text))) = visible_inputs
                    .iter()
                    .enumerate()
                    .find(|(_, (_, text))| text.contains(term.as_str()))
                {
                    let line = text
                        .lines()
                        .position(|line| line.contains(term.as_str()))
                        .unwrap_or_default();
                    let context = inputs[input_index]
                        .1
                        .lines()
                        .nth(line)
                        .unwrap_or_default()
                        .trim()
                        .chars()
                        .take(160)
                        .collect();
                    let components = morphology.tokenize(term)?.iter().map(component).collect();
                    findings.push(LexicalFinding {
                        term: term.clone(),
                        components,
                        file: (*file).clone(),
                        line: line + 1,
                        context,
                        category: "register_variation".to_owned(),
                        rationale: format!("参照資源のレジスター集合 {} で共起: {detail}", set.id),
                        document_frequency: visible_inputs
                            .iter()
                            .map(|(_, text)| text.matches(term.as_str()).count())
                            .sum(),
                        corpus_frequency: reference.corpus_counts.get(term.as_str()).copied(),
                        severity: "info".to_owned(),
                    });
                }
            }
        }
    }

    for candidate in &candidates {
        let Some(corpus_frequency) = reference.corpus_counts.get(&candidate.term).copied() else {
            continue;
        };
        if candidate.count <= 5
            && corpus_frequency <= 1
            && !candidate.has_gloss
            && !reference.known_compounds.contains(&candidate.term)
            && !findings.iter().any(|item| item.term == candidate.term)
        {
            findings.push(finding(
                candidate,
                "novel_compound",
                "一般名詞系の連続、本文5回以下、基準コーパス1回以下、定義手掛かりなし、登録なし"
                    .to_owned(),
                Some(corpus_frequency),
            ));
        }
    }
    findings.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then(a.line.cmp(&b.line))
            .then(a.term.cmp(&b.term))
    });
    Ok(LexicalAuditReport {
        files: inputs.iter().map(|(file, _)| file.clone()).collect(),
        reference_source: reference.source.clone(),
        findings,
    })
}
