use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;

use tempfile::tempdir;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use super::citations::explicit_citation_ids;
use super::docx::file_sha256;
use super::{
    AcademicContract, ArtifactPaths, academic_outline, audit, audit_citations, audit_docx_layout,
    audit_notes, audit_ooxml, audit_sync, has_coined_term_provenance, source_view,
    style_observations,
};

fn write_docx(path: &std::path::Path, text: &str, style: &str, extra: Option<(&str, &str)>) {
    write_docx_with_layout(path, text, style, "", extra);
}

#[test]
fn docx_xml_preserves_japanese_references_and_paragraphs() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("references.docx");
    write_docx_with_layout(
        &path,
        "日本語&amp;記号&#x3002;&#33;",
        "<styles/>",
        "<w:p><w:r><w:t>次の段落&lt;確認&gt;</w:t></w:r></w:p>",
        None,
    );
    assert_eq!(
        super::docx::docx_text(&path).unwrap(),
        "日本語&記号。!\n次の段落<確認>\n"
    );
    assert_eq!(
        super::docx::docx_layout_items(&path).unwrap(),
        vec![
            super::docx::DocxLayoutItem::Paragraph("日本語&記号。!".to_owned()),
            super::docx::DocxLayoutItem::Paragraph("次の段落<確認>".to_owned()),
        ]
    );
    assert!(super::docx::word_xml_text(b"<w:p>\xff</w:p>").is_err());
    assert!(super::docx::word_xml_text(b"<w:p>text</wrong>").is_err());
}

fn write_docx_with_layout(
    path: &std::path::Path,
    text: &str,
    style: &str,
    layout: &str,
    extra: Option<(&str, &str)>,
) {
    write_docx_with_entries(
        path,
        text,
        style,
        layout,
        &extra.into_iter().collect::<Vec<_>>(),
    );
}

fn write_docx_with_entries(
    path: &std::path::Path,
    text: &str,
    style: &str,
    layout: &str,
    extras: &[(&str, &str)],
) {
    let file = File::create(path).expect("create docx");
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    let content_types = extras
        .iter()
        .find_map(|(name, contents)| (*name == "[Content_Types].xml").then_some(*contents))
        .unwrap_or("<Types/>");
    zip.start_file("[Content_Types].xml", options)
        .expect("content types");
    zip.write_all(content_types.as_bytes())
        .expect("write content types");
    zip.start_file("word/styles.xml", options).expect("styles");
    zip.write_all(style.as_bytes()).expect("write styles");
    zip.start_file("word/document.xml", options)
        .expect("document");
    zip.write_all(
            format!(
                r#"<w:document xmlns:w="urn:test"><w:body><w:p><w:r><w:t>{text}</w:t></w:r></w:p>{layout}</w:body></w:document>"#
            )
            .as_bytes(),
        )
        .expect("write document");
    for (name, contents) in extras {
        if *name == "[Content_Types].xml" {
            continue;
        }
        zip.start_file(name, options).expect("extra entry");
        zip.write_all(contents.as_bytes())
            .expect("write extra entry");
    }
    zip.finish().expect("finish docx");
}

fn write_pdf(path: &std::path::Path, text: &str) {
    let text = text
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)");
    let content = format!("BT /F1 12 Tf 72 720 Td ({text}) Tj ET");
    let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_owned(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        ];
    let mut bytes = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    std::fs::write(path, bytes).expect("write PDF");
}

fn write_export_record(
    path: &std::path::Path,
    source: &std::path::Path,
    docx: &std::path::Path,
    pdf: &std::path::Path,
    pdf_visual_reviewed: bool,
) {
    let record = format!(
        r#"{{"exporter":"Microsoft Word","pdf_visual_reviewed":{pdf_visual_reviewed},"source_sha256":"{}","docx_sha256":"{}","pdf_sha256":"{}"}}"#,
        file_sha256(source).expect("source hash"),
        file_sha256(docx).expect("docx hash"),
        file_sha256(pdf).expect("pdf hash"),
    );
    std::fs::write(path, record).expect("write export record");
}

fn contract() -> AcademicContract {
    serde_json::from_str(
        r#"{
                "version": 1,
                "central_claim": "Policy body has evidence.",
                "explanandum": "Policy",
                "allow_formal_research_question": false,
                "reject_defensive_caveats": true,
                "required_order": ["Policy"],
                "terms": [{"term": "Policy", "status": "ordinary"}],
                "proper_nouns": [],
                "first_sentence_order": ["Policy body has evidence."],
                "section_bridges": [],
                "note_decisions": [],
                "style_profile": {
                    "sentence_length_mean_min": 1.0,
                    "sentence_length_mean_max": 100.0,
                    "citation_led_paragraph_ratio_max": 1.0
                }
            }"#,
    )
    .expect("contract")
}

#[test]
fn artifact_audit_checks_sync_and_template_invariants() {
    let dir = tempdir().expect("tempdir");
    let source = dir.path().join("paper.md");
    let template = dir.path().join("template.docx");
    let docx = dir.path().join("paper.docx");
    let pdf = dir.path().join("paper.pdf");
    let record = dir.path().join("delivery-record.json");
    std::fs::write(&source, "# Introduction\n\nPolicy body has evidence.\n").expect("source");
    write_docx(&template, "雛形", "<styles>fixed</styles>", None);
    write_docx(
        &docx,
        "Introduction Policy body has evidence.",
        "<styles>fixed</styles>",
        None,
    );
    write_pdf(&pdf, "Introduction Policy body has evidence.");
    write_export_record(&record, &source, &docx, &pdf, true);

    let report = audit(
        &std::fs::read_to_string(&source).expect("read source"),
        &contract(),
        &ArtifactPaths {
            source: &source,
            docx: Some(&docx),
            pdf: Some(&pdf),
            template: Some(&template),
            export_record: Some(&record),
        },
    )
    .expect("audit");
    assert!(report.passed, "{report:#?}");
    assert!(report.delivery_ready);

    write_export_record(&record, &source, &docx, &pdf, false);
    let unreviewed = audit(
        &std::fs::read_to_string(&source).expect("read source"),
        &contract(),
        &ArtifactPaths {
            source: &source,
            docx: Some(&docx),
            pdf: Some(&pdf),
            template: Some(&template),
            export_record: Some(&record),
        },
    )
    .expect("audit");
    assert!(!unreviewed.delivery_ready);
    assert!(
        unreviewed
            .checks
            .iter()
            .any(|item| item.id == "pdf_visual_review_declaration" && item.status == "fail")
    );

    let partial = audit(
        &std::fs::read_to_string(&source).expect("read source"),
        &contract(),
        &ArtifactPaths {
            source: &source,
            docx: Some(&docx),
            pdf: None,
            template: None,
            export_record: None,
        },
    )
    .expect("audit");
    assert!(!partial.passed);
    assert!(
        partial
            .checks
            .iter()
            .any(|item| item.id == "delivery_artifacts" && item.status == "fail")
    );

    write_docx(
        &docx,
        "Introduction Policy body has evidence.",
        "<styles>changed</styles>",
        None,
    );
    let changed = audit(
        &std::fs::read_to_string(&source).expect("read source"),
        &contract(),
        &ArtifactPaths {
            source: &source,
            docx: Some(&docx),
            pdf: Some(&pdf),
            template: Some(&template),
            export_record: None,
        },
    )
    .expect("audit");
    assert!(!changed.passed);
    assert!(
        changed
            .checks
            .iter()
            .any(|item| item.id == "ooxml_invariants" && item.status == "fail")
    );

    write_docx(
        &docx,
        "Introduction Policy body has evidence.",
        "<styles>fixed</styles>",
        Some(("word/header1.xml", "<header>new</header>")),
    );
    let added = audit(
        &std::fs::read_to_string(&source).expect("read source"),
        &contract(),
        &ArtifactPaths {
            source: &source,
            docx: Some(&docx),
            pdf: Some(&pdf),
            template: Some(&template),
            export_record: None,
        },
    )
    .expect("audit");
    assert!(
        added
            .checks
            .iter()
            .any(|item| item.id == "ooxml_invariants" && item.status == "fail")
    );
}

#[test]
fn document_layout_changes_fail_while_text_replacement_passes() {
    let dir = tempdir().expect("tempdir");
    let source = dir.path().join("paper.md");
    let template = dir.path().join("template.docx");
    let docx = dir.path().join("paper.docx");
    let layout = r#"<w:sectPr><w:pgMar w:left="100"/><w:cols w:num="2"/><w:pgBorders/></w:sectPr>"#;
    std::fs::write(&source, "# Introduction\n\nPolicy body has evidence.\n").expect("source");
    write_docx_with_layout(
        &template,
        "placeholder",
        "<styles>fixed</styles>",
        layout,
        None,
    );
    write_docx_with_layout(
        &docx,
        "Introduction Policy body has evidence.",
        "<styles>fixed</styles>",
        layout,
        None,
    );
    let unchanged = audit(
        &std::fs::read_to_string(&source).expect("read source"),
        &contract(),
        &ArtifactPaths {
            source: &source,
            docx: Some(&docx),
            pdf: None,
            template: Some(&template),
            export_record: None,
        },
    )
    .expect("audit");
    assert!(
        unchanged
            .checks
            .iter()
            .any(|item| item.id == "ooxml_invariants" && item.status == "pass")
    );

    write_docx_with_layout(
        &docx,
        "Introduction Policy body has evidence.",
        "<styles>fixed</styles>",
        r#"<w:sectPr><w:pgMar w:left="200"/><w:cols w:num="2"/><w:pgBorders/></w:sectPr>"#,
        None,
    );
    let changed = audit(
        &std::fs::read_to_string(&source).expect("read source"),
        &contract(),
        &ArtifactPaths {
            source: &source,
            docx: Some(&docx),
            pdf: None,
            template: Some(&template),
            export_record: None,
        },
    )
    .expect("audit");
    assert!(
        changed
            .checks
            .iter()
            .any(|item| item.id == "ooxml_invariants" && item.status == "fail")
    );
}

#[test]
fn sync_is_bidirectional_and_normalizes_links_tables_notes_and_references() {
    let dir = tempdir().expect("tempdir");
    let source_path = dir.path().join("paper.md");
    let docx = dir.path().join("paper.docx");
    let pdf = dir.path().join("paper.pdf");
    let source = "# Paper\n\nBody [link](https://example.com).[^1]\n\n| A | B |\n| - | - |\n| one | two |\n\n[^1]: Note text\n  continues here.\n\n# References\n\n1) Sano 2024a.\n";
    std::fs::write(&source_path, source).expect("source");
    let view = source_view(source, &[]);
    write_docx(&docx, &view.sync_text, "<styles>fixed</styles>", None);
    write_pdf(&pdf, &view.sync_text);
    let extracted = pdf_extract::extract_text(&pdf).expect("extract PDF");
    assert_eq!(
        super::source::normalized(&view.sync_text),
        super::source::normalized(&extracted),
        "PDF text extraction must preserve the fixture"
    );
    let mut checks = Vec::new();
    audit_sync(
        &mut checks,
        &view,
        &contract(),
        &ArtifactPaths {
            source: &source_path,
            docx: Some(&docx),
            pdf: Some(&pdf),
            template: None,
            export_record: None,
        },
    )
    .expect("sync");
    assert!(
        checks.iter().all(|item| item.status == "pass"),
        "{checks:#?}"
    );

    write_docx(
        &docx,
        &format!("{} Old duplicate paragraph.", view.sync_text),
        "<styles>fixed</styles>",
        None,
    );
    write_pdf(&pdf, &format!("{} Old PDF paragraph.", view.sync_text));
    let mut mismatched = Vec::new();
    audit_sync(
        &mut mismatched,
        &view,
        &contract(),
        &ArtifactPaths {
            source: &source_path,
            docx: Some(&docx),
            pdf: Some(&pdf),
            template: None,
            export_record: None,
        },
    )
    .expect("sync");
    assert!(mismatched.iter().all(|item| item.status == "fail"));
}

#[test]
fn citations_are_heading_scoped_bidirectional_and_detect_duplicates() {
    let good = source_view(
        "本文では参考文献を整理する。Sano & Tanaka (2024a) と 佐野・田中（2024b）を使う。\n\n# References\n\nSano, Tanaka (2024a).\n佐野・田中（2024b）.\n",
        &[],
    );
    let mut checks = Vec::new();
    audit_citations(&mut checks, &good, &std::collections::BTreeMap::new());
    assert!(
        checks.iter().all(|item| item.status == "pass"),
        "{checks:#?}"
    );

    let bad = source_view(
        "Sano (2024a) を使う。\n\n# References\n\n1) Sano (2024b).\n2) Other (2024a).\n3) Other (2024a).\n",
        &[],
    );
    let mut checks = Vec::new();
    audit_citations(&mut checks, &bad, &std::collections::BTreeMap::new());
    assert!(
        checks
            .iter()
            .any(|item| item.id == "citation_reference_match" && item.status == "fail")
    );
    assert!(
        checks
            .iter()
            .any(|item| item.id == "uncited_references" && item.status == "fail")
    );
    assert!(
        checks
            .iter()
            .any(|item| item.id == "duplicate_references" && item.status == "fail")
    );
}

#[test]
fn citations_accept_decorated_headings_list_markers_surnames_and_single_publication_year() {
    let good = source_view(
        "本文中の一般語として参考文献を説明する。秋吉（2000）、Sano (2024a)、田中（2024b）を使う。\n\n| 層 | 原典 |\n| - | - |\n| 戦略 | van de Velde（1999） |\n\n## ―――参考文献―――\n\n- 秋吉貴雄（2000）『行政計画』。doi:10.1000/2010.2025\n1) Sano (2024a). https://example.test/2011\n１）田中太郎（2024b）.\n[1] van de Velde (1999).\n[2] Other (1998).\n",
        &[],
    );
    let mut checks = Vec::new();
    audit_citations(&mut checks, &good, &std::collections::BTreeMap::new());
    assert!(
        checks
            .iter()
            .find(|item| item.id == "citation_reference_match")
            .is_some_and(|item| item.status == "pass"),
        "{checks:#?}"
    );
    assert!(
        checks
            .iter()
            .find(|item| item.id == "uncited_references")
            .is_some_and(|item| item.status == "fail"),
        "the uncited [1] entry must remain a real bibliography entry: {checks:#?}"
    );
    assert!(
        !good.references[0].starts_with('-')
            && !good.references[1].starts_with('1')
            && !good.references[2].starts_with('１'),
        "list markers must not become author text: {:?}",
        good.references
    );
}

#[test]
fn citation_keys_keep_japanese_authors_after_prose_and_in_tables() {
    let keys = super::citations::citation_keys(
        "政策規範である。佐野亘ほか（2021）は基準を示す。\n| 出典 | 内閣官房地域未来戦略本部事務局・内閣府地方創生推進室（2025、2026） |",
    );
    assert!(keys.contains("佐野亘:2021"), "{keys:?}");
    assert!(
        keys.contains("内閣官房地域未来戦略本部事務局:2025"),
        "{keys:?}"
    );
}

#[test]
fn parenthetical_citations_match_references_and_detect_missing_entries() {
    for citation in [
        "先行研究（佐野, 2021）を参照した。",
        "先行研究(佐野, 2021)を参照した。",
    ] {
        let source = format!("{citation}\n\n# 参考文献\n\n佐野（2021）『研究』。\n");
        let mut checks = Vec::new();
        audit_citations(&mut checks, &source_view(&source, &[]), &BTreeMap::new());
        assert!(
            checks.iter().all(|item| item.status == "pass"),
            "{checks:#?}"
        );

        let mut missing = Vec::new();
        audit_citations(&mut missing, &source_view(citation, &[]), &BTreeMap::new());
        assert!(
            missing
                .iter()
                .any(|item| item.id == "citation_reference_match" && item.status == "fail"),
            "a missing reference must be detected: {missing:#?}"
        );
    }

    let source =
        "Evidence (Smith, 2021; Jones, 2022).\n\n# References\n\nSmith (2021).\nJones (2022).\n";
    let mut checks = Vec::new();
    audit_citations(&mut checks, &source_view(source, &[]), &BTreeMap::new());
    assert!(
        checks.iter().all(|item| item.status == "pass"),
        "{checks:#?}"
    );
}

#[test]
fn word_xml_text_preserves_ampersands() {
    let text = super::docx::word_xml_text(
            br#"<w:document xmlns:w="urn:test"><w:body><w:p><w:r><w:t>Operations &amp; Production</w:t></w:r></w:p></w:body></w:document>"#,
        )
        .expect("Word XML text");
    assert_eq!(text.trim(), "Operations & Production");
}

#[test]
fn ooxml_allows_article_content_additions_but_preserves_template_assets_and_sections() {
    let dir = tempdir().expect("tempdir");
    let template = dir.path().join("template.docx");
    let docx = dir.path().join("submission.docx");
    let layout = r#"<w:sectPr><w:pgSz w:w="11906"/><w:pgMar w:left="100"/><w:cols w:num="2"/><w:pgBorders><w:top w:val="single"/></w:pgBorders><w:headerReference w:id="rId1"/></w:sectPr>"#;
    let template_entries = [
        (
            "[Content_Types].xml",
            r#"<Types><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/main"/></Types>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            r#"<Relationships><Relationship Id="rId1" Type="header" Target="header1.xml"/></Relationships>"#,
        ),
        ("word/header1.xml", "<header>fixed</header>"),
        ("word/media/template.png", "template image"),
    ];
    write_docx_with_entries(
        &template,
        "placeholder",
        "<styles>fixed</styles>",
        layout,
        &template_entries,
    );
    let submission_entries = [
        (
            "[Content_Types].xml",
            r#"<Types><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/main"/><Override PartName="/word/media/article.png" ContentType="image/png"/></Types>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            r#"<Relationships><Relationship Id="rId1" Type="header" Target="header1.xml"/><Relationship Id="rId2" Type="image" Target="media/article.png"/></Relationships>"#,
        ),
        ("word/header1.xml", "<header>fixed</header>"),
        ("word/media/template.png", "template image"),
        ("word/media/article.png", "article image"),
    ];
    write_docx_with_entries(
        &docx,
        "Article body",
        "<styles>fixed</styles>",
        &format!(
            "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>table</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:drawing/>{layout}"
        ),
        &submission_entries,
    );
    let mut checks = Vec::new();
    audit_ooxml(
        &mut checks,
        &template,
        &docx,
        &contract().docx_mutable_entries,
    )
    .expect("OOXML audit");
    assert!(
        checks.iter().all(|item| item.status == "pass"),
        "{checks:#?}"
    );

    write_docx_with_entries(
        &docx,
        "Article body",
        "<styles>fixed</styles>",
        &layout.replace("w:left=\"100\"", "w:left=\"200\""),
        &submission_entries,
    );
    let mut section_changed = Vec::new();
    audit_ooxml(
        &mut section_changed,
        &template,
        &docx,
        &contract().docx_mutable_entries,
    )
    .expect("OOXML audit");
    assert!(section_changed.iter().any(|item| item.status == "fail"));

    let changed_header = [
        ("[Content_Types].xml", submission_entries[0].1),
        ("word/_rels/document.xml.rels", submission_entries[1].1),
        ("word/header1.xml", "<header>changed</header>"),
        ("word/media/template.png", "template image"),
        ("word/media/article.png", "article image"),
    ];
    write_docx_with_entries(
        &docx,
        "Article body",
        "<styles>fixed</styles>",
        layout,
        &changed_header,
    );
    let mut header_changed = Vec::new();
    audit_ooxml(
        &mut header_changed,
        &template,
        &docx,
        &contract().docx_mutable_entries,
    )
    .expect("OOXML audit");
    assert!(header_changed.iter().any(|item| item.status == "fail"));
}

#[test]
fn docx_footnotes_sync_without_order_dependence_and_reject_extra_duplicates() {
    let dir = tempdir().expect("tempdir");
    let source_path = dir.path().join("paper.md");
    let docx = dir.path().join("paper.docx");
    let source = "Body.[^a] Also body.[^b]\n\n[^a]: Note A.\n[^b]: Note B.\n";
    std::fs::write(&source_path, source).expect("source");
    let notes = r#"<w:footnotes xmlns:w="urn:test"><w:footnote w:id="1"><w:p><w:r><w:t>Note B.</w:t></w:r></w:p></w:footnote><w:footnote w:id="2"><w:p><w:r><w:t>Note A.</w:t></w:r></w:p></w:footnote></w:footnotes>"#;
    write_docx_with_entries(
        &docx,
        "Body. Also body.",
        "<styles>fixed</styles>",
        "",
        &[("word/footnotes.xml", notes)],
    );
    let view = source_view(source, &[]);
    let mut checks = Vec::new();
    audit_sync(
        &mut checks,
        &view,
        &contract(),
        &ArtifactPaths {
            source: &source_path,
            docx: Some(&docx),
            pdf: None,
            template: None,
            export_record: None,
        },
    )
    .expect("sync");
    assert!(
        checks.iter().all(|item| item.status == "pass"),
        "{checks:#?}"
    );

    let duplicate_notes = notes.replace("</w:footnotes>", "<w:footnote w:id=\"3\"><w:p><w:r><w:t>Note A.</w:t></w:r></w:p></w:footnote></w:footnotes>");
    write_docx_with_entries(
        &docx,
        "Body. Also body.",
        "<styles>fixed</styles>",
        "",
        &[("word/footnotes.xml", &duplicate_notes)],
    );
    let mut duplicated = Vec::new();
    audit_sync(
        &mut duplicated,
        &view,
        &contract(),
        &ArtifactPaths {
            source: &source_path,
            docx: Some(&docx),
            pdf: None,
            template: None,
            export_record: None,
        },
    )
    .expect("sync");
    assert!(duplicated.iter().all(|item| item.status == "fail"));
}

#[test]
fn body_checks_notes_and_style_exclude_markdown_outside_the_body() {
    let source = "---\nclaim: Policy body has evidence.\n---\n<!-- Policy -->\n```text\nPolicy\n```\n\n本文。[^missing]\n\n[^1]: Note text\n  continues here.\n\n# References\n\n1) Long Reference Sentence That Must Not Change Style Measurements 2024.\n";
    let view = source_view(source, &[]);
    assert!(!view.main_text.contains("Policy"));
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("paper.md");
    std::fs::write(&path, source).expect("source");
    let report = audit(
        source,
        &contract(),
        &ArtifactPaths {
            source: &path,
            docx: None,
            pdf: None,
            template: None,
            export_record: None,
        },
    )
    .expect("audit");
    for id in [
        "central_claim",
        "explanandum",
        "required_order",
        "term_provenance",
    ] {
        assert!(
            report
                .checks
                .iter()
                .any(|item| item.id == id && item.status == "fail"),
            "{id} must not pass from front matter, comments, code, notes, or references"
        );
    }
    let style = style_observations(&view, &contract().style_profile);
    let baseline_style = style_observations(&source_view("本文。", &[]), &contract().style_profile);
    assert_eq!(style.paragraph_count, 1);
    assert_eq!(style.sentence_length_mean, 3.0);
    assert_eq!(
        style.sentence_length_mean,
        baseline_style.sentence_length_mean
    );
    let mut note_checks = Vec::new();
    audit_notes(
        &mut note_checks,
        &view,
        &[super::NoteDecision {
            marker: "[^1]".to_owned(),
            disposition: "note".to_owned(),
            text: "Note text continues here.".to_owned(),
        }],
    );
    assert!(
        note_checks
            .iter()
            .any(|item| item.id == "note_disposition" && item.status == "fail")
    );
    assert!(
        note_checks
            .iter()
            .any(|item| item.id == "note_references" && item.status == "fail")
    );

    let valid = source_view(
        "本文の補足[^1]。\n\n[^1]: Note text\n  continues here.\n",
        &[],
    );
    let decision = super::NoteDecision {
        marker: "[^1]".to_owned(),
        disposition: "note".to_owned(),
        text: "Note text continues here.".to_owned(),
    };
    let mut valid_checks = Vec::new();
    audit_notes(&mut valid_checks, &valid, std::slice::from_ref(&decision));
    assert!(valid_checks.iter().all(|item| item.status == "pass"));

    let duplicate = source_view(
        "Note text continues here.[^1]\n\n[^1]: Note text\n  continues here.\n",
        &[],
    );
    let mut duplicate_checks = Vec::new();
    audit_notes(&mut duplicate_checks, &duplicate, &[decision]);
    assert!(
        duplicate_checks
            .iter()
            .any(|item| item.id == "note_disposition" && item.status == "fail")
    );
}

#[test]
fn japanese_note_sections_are_matched_to_inline_references() {
    let valid = source_view(
        "本文の補足を注1）に示す。\n\n## ―――注―――\n\n注1）日本語注記の本文である。\n\n## ―――参考文献―――\n",
        &[],
    );
    assert!(!valid.main_text.contains("日本語注記の本文"));
    let decision = super::NoteDecision {
        marker: "注１）".to_owned(),
        disposition: "note".to_owned(),
        text: "日本語注記の本文である。".to_owned(),
    };
    let mut checks = Vec::new();
    audit_notes(&mut checks, &valid, std::slice::from_ref(&decision));
    assert!(
        checks.iter().all(|item| item.status == "pass"),
        "{checks:#?}"
    );

    let unused = source_view(
        "本文。\n\n## ―――注―――\n\n注1）日本語注記の本文である。\n",
        &[],
    );
    let mut unused_checks = Vec::new();
    audit_notes(&mut unused_checks, &unused, &[decision]);
    assert!(
        unused_checks
            .iter()
            .any(|item| item.id == "note_references" && item.status == "fail")
    );
}

#[test]
fn ooxml_rejects_first_page_rule_changes() {
    let dir = tempdir().expect("tempdir");
    let template = dir.path().join("template.docx");
    let docx = dir.path().join("submission.docx");
    let layout = r#"<w:p><w:r><w:pict><v:line id="first-page-rule" style="width:10pt" from="0pt,1pt" to="100pt,1pt"/></w:pict></w:r></w:p><w:sectPr><w:type w:val="continuous"/><w:pgSz w:w="11906"/></w:sectPr>"#;
    write_docx_with_layout(
        &template,
        "placeholder",
        "<styles>fixed</styles>",
        layout,
        None,
    );
    write_docx_with_layout(&docx, "article", "<styles>fixed</styles>", layout, None);
    let mut unchanged = Vec::new();
    audit_ooxml(
        &mut unchanged,
        &template,
        &docx,
        &contract().docx_mutable_entries,
    )
    .expect("OOXML audit");
    assert!(unchanged.iter().all(|item| item.status == "pass"));

    write_docx_with_layout(
        &docx,
        "article",
        "<styles>fixed</styles>",
        &layout.replace("width:10pt", "width:20pt"),
        None,
    );
    assert!(
        !super::docx::preserves_horizontal_vml_lines(
            layout.as_bytes(),
            layout.replace("width:10pt", "width:20pt").as_bytes(),
        )
        .expect("rule comparison")
    );
    let mut changed = Vec::new();
    audit_ooxml(
        &mut changed,
        &template,
        &docx,
        &contract().docx_mutable_entries,
    )
    .expect("OOXML audit");
    assert!(changed.iter().any(|item| item.status == "fail"));
}

#[test]
fn docx_layout_checks_caption_pairing_and_contract_order() {
    let dir = tempdir().expect("tempdir");
    let docx = dir.path().join("paper.docx");
    let layout = r#"<w:tbl><w:tr><w:tc><w:p><w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p><w:r><w:t>表-1　比較</w:t></w:r></w:p><w:p><w:r><w:drawing/></w:r></w:p><w:p><w:r><w:t>図-1　流れ</w:t></w:r></w:p><w:p><w:r><w:t>結論</w:t></w:r></w:p>"#;
    write_docx_with_layout(
        &docx,
        "執政の創造性",
        "<styles>fixed</styles>",
        layout,
        None,
    );
    let mut checks = Vec::new();
    audit_docx_layout(
        &mut checks,
        &docx,
        &[
            "執政の創造性".to_owned(),
            "表-1".to_owned(),
            "図-1".to_owned(),
            "結論".to_owned(),
        ],
    )
    .expect("layout audit");
    assert!(
        checks.iter().all(|item| item.status == "pass"),
        "{checks:#?}"
    );

    let broken = layout.replace(
            "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p><w:r><w:t>表-1　比較</w:t></w:r></w:p>",
            "<w:p><w:r><w:t>表-1　比較</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
        );
    write_docx_with_layout(
        &docx,
        "執政の創造性",
        "<styles>fixed</styles>",
        &broken,
        None,
    );
    let mut broken_checks = Vec::new();
    audit_docx_layout(&mut broken_checks, &docx, &[]).expect("layout audit");
    assert!(
        broken_checks
            .iter()
            .any(|item| item.id == "docx_table_figure_placement" && item.status == "fail")
    );
}

#[test]
fn coined_term_provenance_accepts_research_authorship_and_operational_definition() {
    assert!(has_coined_term_provenance(
        "算出した人口割合を本研究では公共交通人口カバー率と呼ぶ。"
    ));
    assert!(has_coined_term_provenance(
        "七分類は、本研究プロジェクトが分析用に定めた操作的分類である。"
    ));
    assert!(!has_coined_term_provenance(
        "公共交通人口カバー率を分析に用いる。"
    ));
}

#[test]
fn academic_outline_ignores_html_only_wrapper_lines() {
    let view = source_view(
        "# 結果\n\n結果の本文。\n\n<div>\n\n</div>\n\n# 考察\n\n考察の本文。\n",
        &[],
    );
    let outline = academic_outline(&view);
    assert!(
        outline
            .iter()
            .all(|entry| !matches!(entry.text.as_str(), "<div>" | "</div>")),
        "{outline:#?}"
    );
}

#[test]
fn citations_accept_explicit_markdown_ids() {
    let view = source_view(
        "本文で第一資料[@source-a]と第二資料[@source_b]を使う。\n\n# 参考文献\n\n- [@source-a] 第一資料。\n- [@source_b] 第二資料。\n",
        &[],
    );
    let mut checks = Vec::new();
    audit_citations(&mut checks, &view, &BTreeMap::new());
    assert!(
        checks.iter().all(|item| item.status == "pass"),
        "{checks:#?}"
    );
    assert!(explicit_citation_ids("連絡先はwriter@example.jpである。").is_empty());
}

#[test]
fn accepted_labels_do_not_become_unregistered_label_reviews() {
    let source = "# Introduction\n\nPolicy body has evidence. 「単独」\n";
    let mut contract = contract();
    contract.accepted_labels = vec!["単独".to_owned()];
    let path = std::path::Path::new("paper.md");
    let report = audit(
        source,
        &contract,
        &ArtifactPaths {
            source: path,
            docx: None,
            pdf: None,
            template: None,
            export_record: None,
        },
    )
    .expect("academic audit");
    assert!(
        report
            .checks
            .iter()
            .all(|item| item.id != "unregistered_label"),
        "{report:#?}"
    );
}
