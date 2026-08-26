//! Remedy Media向けの隔離lint profile。
//!
//! 通常のSuiko解析とは入力・出力契約を分離し、stdinのHTMLを本文ブロックへ
//! 正規化して、本文を保存・出力せずに安定したfingerprintだけを返す。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::Error;
use crate::lint::{self, Finding, Span};
use crate::morphology::Morphology;

// Production corpus observation (2026-08-26): <=67 KiB and <=1,928 openers.
// Keep explicit headroom while rejecting parser-amplification inputs before DOM construction.
pub const MAX_INPUT_BYTES: usize = 256 * 1024;
pub const MAX_MARKUP_OPENERS: usize = 4_096;
pub const REMEDY_VERSION: &str = "0.3.3-remedy.2";

pub const ADVISORY_PROFILE: &str = "remedy-seo-advisory";
pub const ADVISORY_RULES: &[&str] = &["redundant_light_verb"];

const EXCLUDED_TAGS: &[&str] = &[
    "table",
    "blockquote",
    "figcaption",
    "script",
    "style",
    "template",
    "noscript",
    "pre",
    "code",
    "kbd",
    "samp",
    "svg",
    "math",
];

const FILLER_PATTERNS: &[&str] = &[
    r"することが(?:でき|可能)",
    r"を行うことが(?:でき|可能)",
    r"(?:確認|検討|比較|分析|準備|提供|判断|選定|整理)を行(?:う|い|った|います|って)",
    r"を実施(?:する|します|した|して)",
    r"ということです",
    r"のような形(?:で|に)",
    r"することにより",
    r"というふうに",
    r"していきます",
    r"させていただ(?:き|く|いた)",
    r"ことになり(?:ます|)",
    r"やすく(?:なり|なる|なっ)",
    r"合わせて(?:知りたい|確認|ご覧|お読み|見たい)",
    r"(?:知りたい|気になる|お考えの)方は",
    r"必要があります",
    r"ことが(?:大切|重要|肝心|ポイント)(?:です|になり(?:ます|))",
];

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct RemedyFinding {
    pub rule_id: &'static str,
    pub category: &'static str,
    pub severity: &'static str,
    pub evidence_sha256: String,
}

#[derive(Debug, Serialize)]
pub struct RemedyOutput {
    pub schema_version: &'static str,
    pub findings: Vec<RemedyFinding>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct AdvisoryLocation {
    pub block: usize,
    pub start_byte: usize,
    pub end_byte: usize,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct AdvisoryFinding {
    pub rule: String,
    pub severity: &'static str,
    pub location: AdvisoryLocation,
    pub evidence_sha256: String,
}

#[derive(Debug, Serialize)]
pub struct AdvisorySource {
    pub format: &'static str,
    pub visible_sha256: String,
    pub block_count: usize,
}

#[derive(Debug, Serialize)]
pub struct AdvisorySummary {
    pub total: usize,
    pub by_rule: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
pub struct AdvisoryOutput {
    pub schema_version: &'static str,
    pub suiko_version: &'static str,
    pub commit: &'static str,
    pub profile: &'static str,
    pub source: AdvisorySource,
    pub rules: &'static [&'static str],
    pub summary: AdvisorySummary,
    pub findings: Vec<AdvisoryFinding>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProseBlock {
    element: &'static str,
    text: String,
}

fn valid_commit(commit: &str) -> bool {
    commit.len() == 40
        && commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn remedy_commit() -> Result<&'static str, &'static str> {
    let commit = env!("SUIKO_REMEDY_COMMIT");
    if !valid_commit(commit) {
        Err("SUIKO_REMEDY_COMMIT must be exactly 40 lowercase hexadecimal characters")
    } else {
        Ok(commit)
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(&mut output, "{byte:02x}").expect("write to String");
    }
    output
}

fn final_inline_style_value<'a>(style: &'a str, property: &str) -> Option<&'a str> {
    style
        .split(';')
        .filter_map(|declaration| {
            let (name, value) = declaration.split_once(':')?;
            let name = name.trim();
            (!name.starts_with("--") && name.eq_ignore_ascii_case(property)).then(|| value.trim())
        })
        .next_back()
}

fn css_keyword(value: &str) -> &str {
    let trimmed = value.trim();
    let suffix = b"!important";
    let suffix_start = trimmed.len().saturating_sub(suffix.len());
    if trimmed.as_bytes()[suffix_start..].eq_ignore_ascii_case(suffix) {
        trimmed[..suffix_start].trim()
    } else {
        trimmed
    }
}

fn attr_is_hidden(element: &ElementRef<'_>) -> bool {
    let value = element.value();
    value.attr("hidden").is_some()
        || value
            .attr("aria-hidden")
            .is_some_and(|attribute| attribute.eq_ignore_ascii_case("true"))
        || value.attr("style").is_some_and(|style| {
            final_inline_style_value(style, "display")
                .is_some_and(|display| css_keyword(display).eq_ignore_ascii_case("none"))
                || final_inline_style_value(style, "visibility").is_some_and(|visibility| {
                    css_keyword(visibility).eq_ignore_ascii_case("hidden")
                })
        })
}

pub fn validate_html_input(html: &str) -> Result<(), &'static str> {
    let bytes = html.as_bytes();
    let markup_openers = bytes
        .windows(2)
        .filter(|pair| {
            pair[0] == b'<' && (pair[1].is_ascii_alphabetic() || matches!(pair[1], b'!' | b'?'))
        })
        .take(MAX_MARKUP_OPENERS + 1)
        .count();
    if markup_openers > MAX_MARKUP_OPENERS {
        Err("remedy-seo input has too many markup openers")
    } else {
        Ok(())
    }
}

fn is_swell_rendered_root(element: &ElementRef<'_>) -> bool {
    let value = element.value();
    let class_match = value.classes().any(|class| {
        matches!(
            class,
            "swell-block-button"
                | "swell-block-fullWide"
                | "p-blogParts"
                | "wp-block-buttons"
                | "wp-block-button"
                | "remedy-cta"
                | "cta"
        )
    });
    class_match
        || value.attr("data-remedy-cta").is_some()
        || value
            .attr("data-block")
            .is_some_and(|kind| kind.eq_ignore_ascii_case("loos/blog-parts"))
}

fn is_excluded_element(element: &ElementRef<'_>) -> bool {
    EXCLUDED_TAGS.contains(&element.value().name())
        || attr_is_hidden(element)
        || is_swell_rendered_root(element)
}

fn excluded_by_ancestor(element: &ElementRef<'_>) -> bool {
    element
        .ancestors()
        .filter_map(ElementRef::wrap)
        .any(|ancestor| is_excluded_element(&ancestor))
}

fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn extract_blocks(html: &str) -> Vec<ProseBlock> {
    let document = Html::parse_fragment(html);
    let selector = Selector::parse("p, li").expect("static selector");
    let mut blocks = Vec::new();
    for element in document.select(&selector) {
        if excluded_by_ancestor(&element) {
            continue;
        }
        let current_id = element.id();
        let mut text = String::new();
        for descendant in element.descendants() {
            if ElementRef::wrap(descendant).is_some_and(|child| {
                child.value().name() == "br"
                    && child
                        .ancestors()
                        .filter_map(ElementRef::wrap)
                        .find(|ancestor| matches!(ancestor.value().name(), "p" | "li"))
                        .is_some_and(|ancestor| ancestor.id() == current_id)
            }) {
                text.push(' ');
                continue;
            }
            let Some(raw) = descendant.value().as_text() else {
                continue;
            };
            let mut blocked = false;
            let mut nearest_block = None;
            for ancestor in descendant.ancestors().filter_map(ElementRef::wrap) {
                if is_excluded_element(&ancestor) {
                    blocked = true;
                    break;
                }
                if matches!(ancestor.value().name(), "p" | "li") {
                    nearest_block = Some(ancestor.id());
                    break;
                }
            }
            if !blocked && nearest_block == Some(current_id) {
                text.push_str(raw);
            }
        }
        let text = normalize_whitespace(&text);
        if !text.is_empty() {
            blocks.push(ProseBlock {
                element: if element.value().name() == "p" {
                    "p"
                } else {
                    "li"
                },
                text,
            });
        }
    }
    blocks
}

fn advisory_span(finding: &Finding, blocks: &[ProseBlock]) -> Option<(AdvisoryLocation, String)> {
    let block = finding.span.map_or(finding.line, |span| span.start_line);
    let text = blocks.get(block.checked_sub(1)?)?.text.as_str();
    let (start_byte, end_byte) = match finding.span {
        Some(Span {
            start_line,
            end_line,
            start_byte,
            end_byte,
            ..
        }) if start_line == block
            && end_line == block
            && start_byte < end_byte
            && end_byte <= text.len()
            && text.is_char_boundary(start_byte)
            && text.is_char_boundary(end_byte) =>
        {
            (start_byte, end_byte)
        }
        _ => {
            let excerpt = finding.excerpt.as_str();
            if excerpt.is_empty() {
                (0, text.len())
            } else if let Some(start) = text.find(excerpt) {
                (start, start + excerpt.len())
            } else {
                (0, text.len())
            }
        }
    };
    Some((
        AdvisoryLocation {
            block,
            start_byte,
            end_byte,
        },
        text[start_byte..end_byte].to_owned(),
    ))
}

fn advisory_evidence_hash(
    visible_sha256: &str,
    rule: &str,
    location: &AdvisoryLocation,
    preimage: &str,
) -> String {
    let domain = "suiko-remedy-advisory-evidence-v1";
    let span = format!(
        "{}:{}:{}",
        location.block, location.start_byte, location.end_byte
    );
    let mut evidence = Vec::new();
    for part in [domain, visible_sha256, rule, span.as_str(), preimage] {
        evidence.extend_from_slice(part.as_bytes());
        evidence.push(0);
    }
    sha256_hex(&evidence)
}

/// SWELLの可視本文だけを一般Suikoから選定したruleへ渡す、非blockingのPoC profile。
pub fn analyze_advisory_html(html: &str, morphology: &Morphology) -> Result<AdvisoryOutput, Error> {
    let blocks = extract_blocks(html);
    let visible = blocks
        .iter()
        .map(|block| block.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let visible_sha256 = sha256_hex(visible.as_bytes());
    let normal = lint::analyze(&visible, morphology, Some("business"), false)?;
    let reading = lint::analyze_reading_load(&visible, morphology, Some("business"))?;
    let mut findings = normal
        .findings
        .into_iter()
        .chain(reading.findings)
        .filter(|finding| ADVISORY_RULES.contains(&finding.category.as_str()))
        .filter_map(|finding| {
            let (location, preimage) = advisory_span(&finding, &blocks)?;
            Some(AdvisoryFinding {
                evidence_sha256: advisory_evidence_hash(
                    &visible_sha256,
                    &finding.category,
                    &location,
                    &preimage,
                ),
                rule: finding.category,
                severity: "info",
                location,
            })
        })
        .collect::<Vec<_>>();
    findings.sort();
    findings.dedup();

    let mut by_rule = BTreeMap::new();
    for finding in &findings {
        *by_rule.entry(finding.rule.clone()).or_default() += 1;
    }
    Ok(AdvisoryOutput {
        schema_version: "1",
        suiko_version: env!("CARGO_PKG_VERSION"),
        commit: remedy_commit().map_err(|message| Error::InvalidArguments(message.to_owned()))?,
        profile: ADVISORY_PROFILE,
        source: AdvisorySource {
            format: "html",
            visible_sha256,
            block_count: blocks.len(),
        },
        rules: ADVISORY_RULES,
        summary: AdvisorySummary {
            total: findings.len(),
            by_rule,
        },
        findings,
    })
}

fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("static Remedy regex")
}

fn finding(rule_id: &'static str, category: &'static str, evidence: &str) -> RemedyFinding {
    RemedyFinding {
        rule_id,
        category,
        severity: "warn",
        evidence_sha256: sha256_hex(evidence.as_bytes()),
    }
}

fn masu_streak(blocks: &[ProseBlock]) -> Option<RemedyFinding> {
    // Legacy contract: pだけを「。」で分け、ています。を含む「ます。」終止を4連続で検出。
    let mut run = Vec::new();
    for block in blocks.iter().filter(|block| block.element == "p") {
        for part in block.text.split('。') {
            let sentence = part.trim();
            if sentence.is_empty() {
                continue;
            }
            if sentence.ends_with("ます") {
                run.push(format!("{sentence}。"));
                if run.len() == 4 {
                    return Some(finding("masu-streak", "style", &run.join("\n")));
                }
            } else {
                run.clear();
            }
        }
    }
    None
}

fn filler(blocks: &[ProseBlock]) -> Option<RemedyFinding> {
    let text = blocks
        .iter()
        .map(|block| block.text.as_str())
        .collect::<String>();
    let mut hits = Vec::new();
    for pattern in FILLER_PATTERNS {
        hits.extend(
            regex(pattern)
                .find_iter(&text)
                .map(|hit| hit.as_str().to_owned()),
        );
    }
    // Python oracleの固定長lookbehind/lookaheadを、線形時間の前後文字判定で再現する。
    let state = regex(r"(?:状態|環境|構造|領域|状況)(?:です|になり(?:ます|))");
    hits.extend(state.find_iter(&text).filter_map(|hit| {
        let previous = text[..hit.start()].chars().next_back();
        (previous != Some('変')).then(|| hit.as_str().to_owned())
    }));
    let emphasis = regex(r"(?:基本的に|しっかりと|しっかり|非常に|きちんと)");
    hits.extend(emphasis.find_iter(&text).filter_map(|hit| {
        let next = text[hit.end()..].chars().next();
        (!(hit.as_str() == "しっかり" && matches!(next, Some('と' | '一'))))
            .then(|| hit.as_str().to_owned())
    }));
    let density = if text.is_empty() {
        0.0
    } else {
        hits.len() as f64 / text.chars().count() as f64 * 1000.0
    };
    (hits.len() >= 8 && density >= 2.5).then(|| {
        hits.sort();
        finding("filler", "readability", &hits.join("\n"))
    })
}

fn push_matches(evidence: &mut Vec<String>, text: &str, pattern: &str) {
    evidence.extend(
        regex(pattern)
            .find_iter(text)
            .map(|hit| hit.as_str().to_owned()),
    );
}

fn connector_repetition(blocks: &[ProseBlock]) -> Vec<String> {
    let pattern = regex(r"(?:その)?一方(?:で|、)");
    let start_pattern = regex(r"^(?:その)?一方(?:で|、)");

    // Oracle route 1: 同一blockに3回以上密集する。
    for block in blocks {
        let matches = pattern
            .find_iter(&block.text)
            .map(|hit| hit.as_str().to_owned())
            .collect::<Vec<_>>();
        if matches.len() >= 3 {
            return matches;
        }
    }

    // Oracle route 2: 隣接する3 blockが同じconnectorで始まる。
    for window in blocks.windows(3) {
        let matches = window
            .iter()
            .filter_map(|block| start_pattern.find(&block.text))
            .map(|hit| hit.as_str().to_owned())
            .collect::<Vec<_>>();
        if matches.len() == 3 {
            return matches;
        }
    }

    // Oracle route 3: 記事全体で8回以上出現する。
    let article_matches = blocks
        .iter()
        .flat_map(|block| pattern.find_iter(&block.text))
        .map(|hit| hit.as_str().to_owned())
        .collect::<Vec<_>>();
    if article_matches.len() >= 8 {
        article_matches
    } else {
        Vec::new()
    }
}

fn translationese(blocks: &[ProseBlock]) -> Option<RemedyFinding> {
    // High-confidence subset of check_translationese.py。connector_repetitionはblock identityを
    // 保った集約を移植済みで、その他の未移植categoryはREADMEに明記する。
    let mut evidence = connector_repetition(blocks);
    for block in blocks {
        let text = &block.text;
        for pattern in [
            r"ことが可能(?:です|になります|となります)",
            r"こと(?:可能|重要|必要|有効|大切)(?:です|になります|となります)",
            r"(?:大切|重要)になります",
            r"(?:担う|得る|積む|身につける|活かす)ことができ、",
            r"(?:中|もと|場面)で、(?:同社|当社|[A-Za-z][A-Za-z0-9.&-]{1,30}|[ァ-ヶー]{2,24}|[一-龥々]{2,12})(?:は|が)、",
            r"(?:評価|確認|実施|推進)される(?:構造|状態|環境|形)になります",
            r"整(?:った|備された)(?:環境|体制)[^。！？]{0,20}整備され",
            r"に対して[^。！？]{0,24}(?:重大な|大きな|重要な)?(?:意味|影響|重要性)を持ち(?:ます|つ)",
            r"という以上の(?:意味|価値|重要性)を持ち(?:ます|つ)",
            r"(?:責任|役割|権限|担当|経験)の幅を(?:読み取れる|読める|分かる)状態に(?:し|する|でき)",
            r"(?:判断|意思決定)(?:と|や)(?:実行|施策|行動)を前へ進め",
        ] {
            push_matches(&mut evidence, text, pattern);
        }
    }
    if evidence.is_empty() {
        None
    } else {
        evidence.sort();
        Some(finding("translationese", "style", &evidence.join("\n")))
    }
}

pub fn analyze_html(html: &str) -> RemedyOutput {
    let blocks = extract_blocks(html);
    let mut findings = BTreeSet::new();
    if let Some(item) = masu_streak(&blocks) {
        findings.insert(item);
    }
    if let Some(item) = filler(&blocks) {
        findings.insert(item);
    }
    if let Some(item) = translationese(&blocks) {
        findings.insert(item);
    }
    RemedyOutput {
        schema_version: "1",
        findings: findings.into_iter().collect(),
    }
}

pub fn analyze_filler_html(html: &str) -> RemedyOutput {
    let blocks = extract_blocks(html);
    let findings = filler(&blocks).into_iter().collect();
    RemedyOutput {
        schema_version: "1",
        findings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_excludes_non_prose_hidden_and_nested_list_duplicates() {
        let blocks = extract_blocks(
            r#"<!-- <p>コメントです。</p> --><H2>見出しです。</H2><P>A&amp;B<br>改行です。</P>
            <table><tbody><tr><td><p>表です。</p></td></tr></tbody></table><div hidden><p>秘密です。</p></div>
            <div style="DISPLAY: none"><p>不可視です。</p></div><SCRIPT><p>scriptです。</p></SCRIPT>
            <div class="p-blogParts"><p>CTAです。</p></div>
            <ul><li>親です。<ul><li>子です。</li></ul></li></ul>"#,
        );
        assert_eq!(
            blocks,
            vec![
                ProseBlock {
                    element: "p",
                    text: "A&B 改行です。".into()
                },
                ProseBlock {
                    element: "li",
                    text: "親です。".into()
                },
                ProseBlock {
                    element: "li",
                    text: "子です。".into()
                },
            ]
        );
    }

    #[test]
    fn inline_style_uses_declarations_and_last_value_wins() {
        let blocks = extract_blocks(
            r#"<div style="--note: display:none; display:none; display:block"><p>表示です。</p></div>
            <div style="display:block; DISPLAY: none !important"><p>非表示です。</p></div>
            <div style="visibility:hidden; visibility:visible"><p>表示2です。</p></div>"#,
        );
        assert_eq!(
            blocks
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>(),
            vec!["表示です。", "表示2です。"]
        );
    }

    #[test]
    fn excessive_markup_is_rejected_before_parsing() {
        let html = "<div>".repeat(MAX_MARKUP_OPENERS + 1);
        let started = std::time::Instant::now();
        assert_eq!(
            validate_html_input(&html),
            Err("remedy-seo input has too many markup openers")
        );
        assert!(started.elapsed() < std::time::Duration::from_millis(100));
        assert!(validate_html_input(&"<div>".repeat(MAX_MARKUP_OPENERS)).is_ok());
    }

    #[test]
    fn malformed_fragment_and_entities_are_deterministic() {
        let html = "<p>一つ&amp;二つです。<p>三つです。";
        assert_eq!(
            serde_json::to_string(&analyze_html(html)).unwrap(),
            serde_json::to_string(&analyze_html(html)).unwrap()
        );
    }

    #[test]
    fn legacy_masu_is_p_only_and_includes_teimasu() {
        let output = analyze_html(
            "<li>します。します。します。します。</li><p>しています。確認します。進めます。終えます。</p>",
        );
        assert_eq!(
            output
                .findings
                .iter()
                .filter(|item| item.rule_id == "masu-streak")
                .count(),
            1
        );
    }

    #[test]
    fn finding_schema_is_redacted() {
        let output =
            serde_json::to_value(analyze_html("<p>します。します。します。します。</p>")).unwrap();
        let finding = &output["findings"][0];
        assert_eq!(finding.as_object().unwrap().len(), 4);
        assert!(
            finding["evidence_sha256"]
                .as_str()
                .unwrap()
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        );
    }

    #[test]
    fn remedy_commit_contract_rejects_noncanonical_values() {
        assert!(valid_commit("3651215fee9409afe016de8d8347c442e1b5c88d"));
        assert!(!valid_commit("3651215FEE9409AFE016DE8D8347C442E1B5C88D"));
        assert!(!valid_commit("main"));
        assert!(!valid_commit(""));
    }

    #[test]
    fn filler_matches_oracle_lookaround_boundaries_without_backtracking_regex() {
        let normal = extract_blocks("<p>状態です。しっかり準備します。</p>");
        let excluded =
            extract_blocks("<p>変状態です。しっかりと進め、しっかり一歩ずつ行います。</p>");
        let normal_text = normal
            .iter()
            .map(|block| block.text.as_str())
            .collect::<String>();
        let excluded_text = excluded
            .iter()
            .map(|block| block.text.as_str())
            .collect::<String>();
        let count = |text: &str| {
            let state = regex(r"(?:状態|環境|構造|領域|状況)(?:です|になり(?:ます|))");
            let states = state
                .find_iter(text)
                .filter(|hit| !text[..hit.start()].ends_with('変'))
                .count();
            let emphasis = regex(r"(?:基本的に|しっかりと|しっかり|非常に|きちんと)");
            let emphases = emphasis
                .find_iter(text)
                .filter(|hit| {
                    !(hit.as_str() == "しっかり"
                        && matches!(text[hit.end()..].chars().next(), Some('と' | '一')))
                })
                .count();
            states + emphases
        };
        assert_eq!(count(&normal_text), 2);
        assert_eq!(count(&excluded_text), 1); // 「しっかりと」だけ。oracleと同じ。
    }

    #[test]
    fn translationese_profile_documents_subset_boundary() {
        // check_translationese.py の単発high-confidence categoryは移植済み。
        assert!(
            analyze_html("<p>この確認を行うことが可能です。</p>")
                .findings
                .iter()
                .any(|item| item.rule_id == "translationese")
        );
        // connector_repetitionは文書集約categoryとして移植済み。
        assert!(
            analyze_html("<p>一方で、Aです。</p><p>一方で、Bです。</p><p>一方で、Cです。</p>")
                .findings
                .iter()
                .any(|item| item.rule_id == "translationese")
        );
    }

    #[test]
    fn filler_profile_does_not_run_other_analyzers() {
        let non_filler = analyze_filler_html(
            "<p>進めます。確認します。整えます。終えます。</p>\
             <p>一方で、Aです。</p><p>一方で、Bです。</p><p>一方で、Cです。</p>",
        );
        assert!(non_filler.findings.is_empty());

        let filler = analyze_filler_html(&format!("<p>{}</p>", "必要があります。".repeat(8)));
        assert_eq!(filler.findings.len(), 1);
        assert_eq!(filler.findings[0].rule_id, "filler");
    }

    #[test]
    fn connector_repetition_matches_all_three_oracle_routes() {
        let dense = extract_blocks("<p>一方でAです。一方でBです。その一方でCです。</p>");
        assert_eq!(connector_repetition(&dense).len(), 3);

        let adjacent =
            extract_blocks("<p>一方で、Aです。</p><p>その一方でBです。</p><li>一方、Cです。</li>");
        assert_eq!(connector_repetition(&adjacent).len(), 3);

        let article = extract_blocks(
            "<p>一方でAです。一方でBです。</p><p>Cです。</p>\
             <p>一方でDです。一方でEです。</p><p>Fです。</p>\
             <p>一方でGです。一方でHです。</p><p>Iです。</p>\
             <p>一方でJです。一方でKです。</p>",
        );
        assert_eq!(connector_repetition(&article).len(), 8);
    }

    #[test]
    fn connector_repetition_does_not_expand_below_oracle_thresholds() {
        let below = extract_blocks(
            "<p>一方でAです。一方でBです。</p><p>通常です。</p>\
             <p>一方でCです。一方でDです。</p><p>通常です。</p>\
             <p>一方でEです。一方でFです。</p><p>通常です。</p>\
             <p>一方でGです。</p>",
        );
        assert!(connector_repetition(&below).is_empty());
    }

    #[test]
    fn advisory_allowlist_has_a_positive_and_negative_example() {
        let morphology = Morphology::new().expect("initialize morphology");
        let positive = analyze_advisory_html("<p>結合部分の検証を行います。</p>", &morphology)
            .expect("analyze positive");
        assert_eq!(positive.findings.len(), 1);
        assert_eq!(positive.findings[0].rule, "redundant_light_verb");
        let negative = analyze_advisory_html("<p>地域の祭りを行います。</p>", &morphology)
            .expect("analyze negative");
        assert!(negative.findings.is_empty());
    }

    #[test]
    fn advisory_allowlist_excludes_discovery_rules_that_did_not_advance() {
        let morphology = Morphology::new().expect("initialize morphology");
        let cases = [
            (
                "no_comma_sentence",
                "本文書は昨年度に実施した全社的な業務プロセス改革の結果を踏まえて策定された次年度の重点施策と実行体制を体系的に整理した参考資料です。",
            ),
            ("double_negative", "ないわけではありません。"),
            ("no_chain", "東京の本社の営業部の担当者が資料を送ります。"),
            ("kanji_run", "来月から本番環境設定変更手順書を更新します。"),
            (
                "buried_list",
                "顧客管理、売上分析、在庫管理、採用計画について、各部門の担当者が現在の課題を確認したうえで改善を進めます。",
            ),
            ("inanimate_subject_morph", "この事実が成果をもたらします。"),
            (
                "abstract_metaphor",
                "この方針は実装判断の羅針盤になります。",
            ),
        ];
        for (rule, text) in cases {
            let normal = lint::analyze(text, &morphology, Some("business"), false)
                .expect("analyze normal lint");
            let reading = lint::analyze_reading_load(text, &morphology, Some("business"))
                .expect("analyze reading load");
            assert!(
                normal
                    .findings
                    .iter()
                    .chain(&reading.findings)
                    .any(|finding| finding.category == rule),
                "fixture must fire excluded rule {rule}"
            );
            let advisory = analyze_advisory_html(&format!("<p>{text}</p>"), &morphology)
                .expect("analyze advisory");
            assert!(
                advisory.findings.iter().all(|finding| finding.rule != rule),
                "excluded rule leaked from advisory: {rule}"
            );
        }
    }
}
