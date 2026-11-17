//! DOCX（OOXML）の読み取りと、体裁・保護対象・書き出し記録の監査。

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::escape::resolve_xml_entity;
use quick_xml::events::Event;
use regex::Regex;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::{Error, read_source};

use super::source::normalized;
use super::{AcademicCheck, ArtifactPaths, ExportRecord, add_check};

fn zip_entries(path: &Path) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    let file = File::open(path)
        .map_err(|error| Error::Academic(format!("{}を開けません: {error}", path.display())))?;
    let mut archive = ZipArchive::new(file).map_err(|error| {
        Error::Academic(format!(
            "{}は有効なDOCXではありません: {error}",
            path.display()
        ))
    })?;
    let mut entries = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| Error::Academic(error.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry
            .name()
            .map_err(|error| Error::Academic(format!("DOCXのエントリ名を読めません: {error}")))?
            .into_owned();
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| Error::Academic(error.to_string()))?;
        entries.insert(name, bytes);
    }
    Ok(entries)
}

pub(super) fn word_xml_text(xml: &[u8]) -> Result<String, Error> {
    let mut reader = Reader::from_reader(xml);
    let mut output = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Text(text)) => {
                output.push_str(&text);
            }
            Ok(Event::GeneralRef(reference)) => {
                if let Some(character) = reference
                    .resolve_char_ref()
                    .map_err(|error| Error::Academic(error.to_string()))?
                {
                    output.push(character);
                } else if let Some(value) = resolve_xml_entity(&reference) {
                    output.push_str(value);
                }
            }
            Ok(Event::End(end)) if end.name().as_ref() == "w:p" => output.push('\n'),
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(Error::Academic(error.to_string())),
        }
    }
    Ok(output)
}

pub(super) fn docx_text(path: &Path) -> Result<String, Error> {
    let entries = zip_entries(path)?;
    let document = entries
        .get("word/document.xml")
        .ok_or_else(|| Error::Academic("DOCXにword/document.xmlがありません".to_owned()))?;
    let mut parts = vec![word_xml_text(document)?];
    for name in ["word/footnotes.xml", "word/endnotes.xml"] {
        if let Some(xml) = entries.get(name) {
            parts.push(word_xml_text(xml)?);
        }
    }
    Ok(parts.join("\n"))
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum DocxLayoutItem {
    Paragraph(String),
    Table,
    Figure,
}

pub(super) fn docx_layout_items(path: &Path) -> Result<Vec<DocxLayoutItem>, Error> {
    let entries = zip_entries(path)?;
    let document = entries
        .get("word/document.xml")
        .ok_or_else(|| Error::Academic("DOCXにword/document.xmlがありません".to_owned()))?;
    let mut reader = Reader::from_reader(document.as_slice());
    let mut items = Vec::new();
    let mut table_depth = 0usize;
    let mut in_paragraph = false;
    let mut paragraph_text = String::new();
    let mut paragraph_has_figure = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => match start.name().as_ref() {
                "w:tbl" => {
                    if table_depth == 0 {
                        items.push(DocxLayoutItem::Table);
                    }
                    table_depth += 1;
                }
                "w:p" if table_depth == 0 => {
                    in_paragraph = true;
                    paragraph_text.clear();
                    paragraph_has_figure = false;
                }
                "w:drawing" if in_paragraph && table_depth == 0 => {
                    paragraph_has_figure = true;
                }
                _ => {}
            },
            Ok(Event::Empty(empty)) => match empty.name().as_ref() {
                "w:drawing" if in_paragraph && table_depth == 0 => {
                    paragraph_has_figure = true;
                }
                _ => {}
            },
            Ok(Event::Text(text)) if in_paragraph && table_depth == 0 => {
                paragraph_text.push_str(&text);
            }
            Ok(Event::GeneralRef(reference)) if in_paragraph && table_depth == 0 => {
                if let Some(character) = reference
                    .resolve_char_ref()
                    .map_err(|error| Error::Academic(error.to_string()))?
                {
                    paragraph_text.push(character);
                } else if let Some(value) = resolve_xml_entity(&reference) {
                    paragraph_text.push_str(value);
                }
            }
            Ok(Event::End(end)) => match end.name().as_ref() {
                "w:p" if in_paragraph && table_depth == 0 => {
                    if paragraph_has_figure {
                        items.push(DocxLayoutItem::Figure);
                    }
                    let text = paragraph_text.trim();
                    if !text.is_empty() {
                        items.push(DocxLayoutItem::Paragraph(text.to_owned()));
                    }
                    in_paragraph = false;
                }
                "w:tbl" => table_depth = table_depth.saturating_sub(1),
                _ => {}
            },
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(Error::Academic(error.to_string())),
        }
    }
    Ok(items)
}

pub(super) fn audit_docx_layout(
    checks: &mut Vec<AcademicCheck>,
    docx: &Path,
    required_order: &[String],
) -> Result<(), Error> {
    let items = docx_layout_items(docx)?;
    let caption = Regex::new(r"^(表|図)[-－ー]?[0-9０-９]+\s+").expect("valid caption regex");
    let mut table_captions = 0usize;
    let mut figure_captions = 0usize;
    let mut misplaced = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let DocxLayoutItem::Paragraph(text) = item else {
            continue;
        };
        let Some(kind) = caption
            .captures(text.trim_start())
            .map(|capture| capture[1].to_owned())
        else {
            continue;
        };
        let expected = if kind == "表" {
            table_captions += 1;
            DocxLayoutItem::Table
        } else {
            figure_captions += 1;
            DocxLayoutItem::Figure
        };
        let paired = index
            .checked_sub(1)
            .and_then(|previous| items.get(previous))
            .is_some_and(|item| item == &expected);
        if !paired {
            misplaced.push(text.clone());
        }
    }
    let tables = items
        .iter()
        .filter(|item| matches!(item, DocxLayoutItem::Table))
        .count();
    let figures = items
        .iter()
        .filter(|item| matches!(item, DocxLayoutItem::Figure))
        .count();
    let pairs_ok = misplaced.is_empty() && tables == table_captions && figures == figure_captions;
    add_check(
        checks,
        "docx_table_figure_placement",
        pairs_ok,
        if pairs_ok {
            format!("表{tables}点・図{figures}点の直後に対応する表題・図題があります")
        } else {
            format!(
                "表/表題 {tables}/{table_captions}、図/図題 {figures}/{figure_captions}、位置不整合: {}",
                misplaced.join(", ")
            )
        },
    );

    let paragraphs = items
        .iter()
        .filter_map(|item| match item {
            DocxLayoutItem::Paragraph(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut cursor = 0usize;
    let mut missing = Vec::new();
    for anchor in required_order {
        if let Some(offset) = paragraphs[cursor..]
            .iter()
            .position(|paragraph| paragraph.contains(anchor))
        {
            cursor += offset + 1;
        } else {
            missing.push(anchor.clone());
        }
    }
    add_check(
        checks,
        "docx_layout_order",
        missing.is_empty(),
        if missing.is_empty() {
            format!(
                "指定した{}個の配置基準を順に確認しました",
                required_order.len()
            )
        } else {
            format!("指定位置以降で確認できない配置基準: {}", missing.join(", "))
        },
    );
    Ok(())
}

fn protected_document_layout(xml: &[u8]) -> Result<Vec<String>, Error> {
    let xml = std::str::from_utf8(xml).map_err(|error| {
        Error::Academic(format!("word/document.xmlはUTF-8ではありません: {error}"))
    })?;
    let section = Regex::new(r#"(?s)<w:sectPr\b.*?</w:sectPr>|<w:sectPr\b[^>]*/>"#)
        .expect("valid section regex");
    let protected = Regex::new(
        r#"(?s)<w:type\b[^>]*/>|<w:pgSz\b[^>]*/>|<w:pgMar\b[^>]*/>|<w:cols\b[^>]*/>|<w:docGrid\b[^>]*/>|<w:pgBorders\b.*?</w:pgBorders>|<w:pgBorders\b[^>]*/>|<w:(?:headerReference|footerReference)\b[^>]*/>"#,
    )
    .expect("valid protected section settings regex");
    Ok(section
        .find_iter(xml)
        .flat_map(|section| protected.find_iter(section.as_str()))
        .map(|item| normalized(item.as_str()))
        .collect())
}

fn xml_elements(
    xml: &[u8],
    element_names: &[&str],
    identity_attribute: &str,
) -> Result<BTreeMap<String, String>, Error> {
    let xml = std::str::from_utf8(xml)
        .map_err(|error| Error::Academic(format!("OOXMLはUTF-8ではありません: {error}")))?;
    let elements = Regex::new(&format!(
        r#"<(?:{})\b(?P<attributes>[^>]*)/?>"#,
        element_names.join("|")
    ))
    .expect("valid OOXML element regex");
    let attributes = Regex::new(r#"([A-Za-z:]+)="([^"]*)""#).expect("valid OOXML attribute regex");
    let mut values = BTreeMap::new();
    for element in elements.captures_iter(xml) {
        let mut attributes = attributes
            .captures_iter(
                element
                    .name("attributes")
                    .map_or("", |value| value.as_str()),
            )
            .map(|attribute| (attribute[1].to_owned(), attribute[2].to_owned()))
            .collect::<BTreeMap<_, _>>();
        let identity = attributes.remove(identity_attribute).ok_or_else(|| {
            Error::Academic(format!(
                "OOXMLの{}に{}がありません",
                element_names.join("/"),
                identity_attribute
            ))
        })?;
        let content = attributes
            .into_iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join(";");
        values.insert(identity, content);
    }
    Ok(values)
}

fn preserves_xml_elements(
    before: &[u8],
    after: &[u8],
    element_names: &[&str],
    identity_attribute: &str,
) -> Result<bool, Error> {
    let before = xml_elements(before, element_names, identity_attribute)?;
    let after = xml_elements(after, element_names, identity_attribute)?;
    Ok(before
        .iter()
        .all(|(identity, value)| after.get(identity) == Some(value)))
}

pub(super) fn preserves_horizontal_vml_lines(before: &[u8], after: &[u8]) -> Result<bool, Error> {
    fn horizontal(xml: &[u8]) -> Result<BTreeMap<String, String>, Error> {
        let lines = xml_elements(xml, &["v:line"], "id")?;
        Ok(lines
            .into_iter()
            .filter(|(_, attributes)| {
                let from = attributes
                    .split(';')
                    .find_map(|item| item.strip_prefix("from="));
                let to = attributes
                    .split(';')
                    .find_map(|item| item.strip_prefix("to="));
                match (from, to) {
                    (Some(from), Some(to)) => {
                        from.split_once(',').map(|(_, y)| y) == to.split_once(',').map(|(_, y)| y)
                    }
                    _ => false,
                }
            })
            .collect())
    }
    let before = horizontal(before)?;
    let after = horizontal(after)?;
    Ok(before
        .iter()
        .all(|(identity, value)| after.get(identity) == Some(value)))
}

fn preserves_content_types(before: &[u8], after: &[u8]) -> Result<bool, Error> {
    Ok(
        preserves_xml_elements(before, after, &["Default"], "Extension")?
            && preserves_xml_elements(before, after, &["Override"], "PartName")?,
    )
}

fn mutable(name: &str, rules: &[String]) -> bool {
    rules.iter().any(|rule| {
        rule.strip_suffix('*')
            .map_or(name == rule, |prefix| name.starts_with(prefix))
    })
}

pub(super) fn audit_ooxml(
    checks: &mut Vec<AcademicCheck>,
    template: &Path,
    docx: &Path,
    mutable_entries: &[String],
) -> Result<(), Error> {
    let before = zip_entries(template)?;
    let after = zip_entries(docx)?;
    let changed = before
        .keys()
        .chain(after.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|name| {
            if name.as_str() == "word/document.xml" || mutable(name, mutable_entries) {
                return false;
            }
            match name.as_str() {
                "[Content_Types].xml" => before.get(*name).is_none_or(|before_xml| {
                    after.get(*name).is_none_or(|after_xml| {
                        !preserves_content_types(before_xml, after_xml).unwrap_or(false)
                    })
                }),
                "word/_rels/document.xml.rels" => before.get(*name).is_none_or(|before_xml| {
                    after.get(*name).is_none_or(|after_xml| {
                        !preserves_xml_elements(before_xml, after_xml, &["Relationship"], "Id")
                            .unwrap_or(false)
                    })
                }),
                name if name.starts_with("word/media/") => {
                    before.contains_key(name) && before.get(name) != after.get(name)
                }
                _ => before.get(*name) != after.get(*name),
            }
        })
        .cloned()
        .collect::<Vec<_>>();
    let before_layout = before
        .get("word/document.xml")
        .ok_or_else(|| Error::Academic("テンプレートにword/document.xmlがありません".to_owned()))
        .and_then(|xml| protected_document_layout(xml))?;
    let after_layout = after
        .get("word/document.xml")
        .ok_or_else(|| Error::Academic("DOCXにword/document.xmlがありません".to_owned()))
        .and_then(|xml| protected_document_layout(xml))?;
    let layout_changed = before_layout != after_layout;
    let template_rules_preserved = preserves_horizontal_vml_lines(
        before
            .get("word/document.xml")
            .expect("template document checked above"),
        after
            .get("word/document.xml")
            .expect("submission document checked above"),
    )?;
    add_check(
        checks,
        "ooxml_invariants",
        changed.is_empty() && !layout_changed && template_rules_preserved,
        if changed.is_empty() && !layout_changed && template_rules_preserved {
            "許可していないテンプレートOOXML、節設定及び第一頁罫線は不変です".to_owned()
        } else {
            format!(
                "許可なく変わったOOXML: {}; 節設定変更: {}; 第一頁罫線維持: {}",
                changed.join(", "),
                if layout_changed { "あり" } else { "なし" },
                if template_rules_preserved {
                    "はい"
                } else {
                    "いいえ"
                }
            )
        },
    );
    Ok(())
}

pub(super) fn file_sha256(path: &Path) -> Result<String, Error> {
    let bytes = std::fs::read(path).map_err(|source| Error::Read {
        path: path.display().to_string(),
        source,
    })?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub(super) fn audit_export_record(
    checks: &mut Vec<AcademicCheck>,
    record_path: &Path,
    paths: &ArtifactPaths<'_>,
) -> Result<(), Error> {
    let record: ExportRecord = serde_json::from_str(&read_source(record_path)?)?;
    add_check(
        checks,
        "word_exporter_declaration",
        record.exporter == "Microsoft Word",
        format!(
            "納品記録の自己申告exporter: {}（SuikoはWord出力自体を実証しません）",
            record.exporter
        ),
    );
    add_check(
        checks,
        "pdf_visual_review_declaration",
        record.pdf_visual_reviewed,
        "PDF全頁を画像として目視確認した自己申告を確認（実施事実は機械検証しません）",
    );
    let source_ok = file_sha256(paths.source)? == record.source_sha256;
    let docx_ok = paths
        .docx
        .is_some_and(|path| file_sha256(path).is_ok_and(|hash| hash == record.docx_sha256));
    let pdf_ok = paths
        .pdf
        .is_some_and(|path| file_sha256(path).is_ok_and(|hash| hash == record.pdf_sha256));
    add_check(
        checks,
        "export_record_hashes",
        source_ok && docx_ok && pdf_ok,
        "納品記録のMarkdown・DOCX・PDFハッシュを照合しました",
    );
    Ok(())
}
