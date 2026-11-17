//! 原稿と成果物（DOCX・PDFから取り出した本文）の同期の照合。

use std::collections::BTreeMap;

use crate::Error;

use super::docx::docx_text;
use super::source::{SourceView, normalized};
use super::{AcademicCheck, AcademicContract, ArtifactPaths, add_check};

#[derive(Debug, PartialEq, Eq)]
struct SyncInventory {
    blocks: BTreeMap<String, usize>,
    characters: BTreeMap<char, usize>,
    character_count: usize,
    duplicate_blocks: usize,
}

fn sync_normalized(text: &str, omitted_fragments: &[String]) -> String {
    let mut text = normalized(text);
    for fragment in omitted_fragments {
        text = text.replace(&normalized(fragment), "");
    }
    text
}

fn sync_inventory(text: &str, omitted_fragments: &[String]) -> SyncInventory {
    let normalized_text = sync_normalized(text, omitted_fragments);
    let mut blocks = BTreeMap::<String, usize>::new();
    for block in text
        .split_inclusive(['。', '！', '？', '.', '!', '?'])
        .map(|block| sync_normalized(block, omitted_fragments))
        .filter(|block| !block.is_empty())
    {
        *blocks.entry(block).or_default() += 1;
    }
    let mut characters = BTreeMap::<char, usize>::new();
    for character in normalized_text.chars() {
        *characters.entry(character).or_default() += 1;
    }
    let duplicate_blocks = blocks
        .values()
        .filter(|count| **count > 1)
        .map(|count| count - 1)
        .sum();
    SyncInventory {
        blocks,
        characters,
        character_count: normalized_text.chars().count(),
        duplicate_blocks,
    }
}

fn inventories_match(source: &SyncInventory, artifact: &SyncInventory) -> bool {
    source.blocks == artifact.blocks
        && source.characters == artifact.characters
        && source.character_count == artifact.character_count
        && source.duplicate_blocks == artifact.duplicate_blocks
}

fn inventory_difference(source: &SyncInventory, artifact: &SyncInventory) -> String {
    fn character_delta(left: &BTreeMap<char, usize>, right: &BTreeMap<char, usize>) -> Vec<String> {
        left.iter()
            .filter_map(|(character, count)| {
                let difference = count.saturating_sub(*right.get(character).unwrap_or(&0));
                (difference > 0).then(|| format!("{character:?}×{difference}"))
            })
            .take(20)
            .collect()
    }
    let source_only = character_delta(&source.characters, &artifact.characters);
    let artifact_only = character_delta(&artifact.characters, &source.characters);
    format!(
        "Markdownのみ [{}] / 成果物のみ [{}]",
        source_only.join(", "),
        artifact_only.join(", ")
    )
}

pub(super) fn audit_sync(
    checks: &mut Vec<AcademicCheck>,
    view: &SourceView,
    contract: &AcademicContract,
    paths: &ArtifactPaths<'_>,
) -> Result<(), Error> {
    let source = sync_inventory(&view.sync_text, &contract.sync_omit_fragments);
    if let Some(docx) = paths.docx {
        let text = sync_inventory(&docx_text(docx)?, &contract.sync_omit_fragments);
        let matches = inventories_match(&source, &text);
        add_check(
            checks,
            "markdown_docx_sync",
            matches,
            format!(
                "MarkdownとDOCXの本文・表・注・参考文献を順序非依存で双方向照合（文字数 {} / {}、重複ブロック {} / {}）: {}",
                source.character_count,
                text.character_count,
                source.duplicate_blocks,
                text.duplicate_blocks,
                if matches {
                    "一致".to_owned()
                } else {
                    format!("不一致; {}", inventory_difference(&source, &text))
                }
            ),
        );
    }
    if let Some(pdf) = paths.pdf {
        let extracted = pdf_extract::extract_text(pdf)
            .map_err(|error| Error::Academic(format!("PDF本文を抽出できません: {error}")))?;
        let text = sync_inventory(&extracted, &contract.sync_omit_fragments);
        let matches = inventories_match(&source, &text);
        add_check(
            checks,
            "markdown_pdf_sync",
            matches,
            format!(
                "MarkdownとPDFの本文・表・注・参考文献を順序非依存で双方向照合（文字数 {} / {}、重複ブロック {} / {}）: {}",
                source.character_count,
                text.character_count,
                source.duplicate_blocks,
                text.duplicate_blocks,
                if matches {
                    "一致".to_owned()
                } else {
                    format!("不一致; {}", inventory_difference(&source, &text))
                }
            ),
        );
    }
    Ok(())
}
