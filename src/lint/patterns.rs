//! 表層（文字列・正規表現）ベースの検出器。行単位でmasked本文を走査し、
//! 抜粋とspanはbyteレイアウトが同一のraw行から作る。

use std::collections::BTreeSet;

use regex::Regex;
use serde_json::{Value, json};

use crate::Error;
use crate::morphology::Morphology;
use crate::text::{Sentence, excerpt_around, mask_html_comments, numbered_lines};

use super::{Finding, make_span};

const FORBIDDEN_PHRASES: &[&str] = &[
    "と言えるでしょう",
    "と言えるだろう",
    "と言えます",
    "ということになるでしょう",
    "のではないでしょうか",
    "重要なのは",
    "大切なのは",
    "ポイントは",
    "結論から言うと",
    "結論として",
    "いかがでしたか",
    "いかがでしょうか",
    "まとめると",
    "総じて",
    "非常に重要",
    "極めて重要",
    "言うまでもなく",
    "言うまでもありません",
    "まさしく",
    "さて、",
    "それでは、",
    "このように",
    "このような中",
    "ここで注目したいのは",
    "見ていきましょう",
    "紹介していきます",
    "解説していきます",
    "深掘りしていきます",
    "一概には言えません",
    "個人差がありますが",
    "あくまで一例ですが",
    "正面から扱う",
    "正面から見る",
    "正面から書く",
    "正面から立てる",
    "正面から回収する",
    "不可欠",
    "核心的",
    "鍵となる",
    "根本的な",
    "多角的",
    "包括的",
    "総合的",
    "掘り下げる",
    "深掘りする",
    "言語化する",
    "について見ていく",
    "を探求する",
];

const WEAK_FORBIDDEN_PHRASES: &[&str] =
    &["重要なのは", "このように", "不可欠", "ポイントは", "さて、"];

const TRANSLATIONESE_PATTERNS: &[&str] = &[
    r"することができ(る|ます|た)",
    r"することが可能(です|だ|になる)",
    r"と言えるだろう",
    r"という点で",
    r"という観点(から|で)",
    r"にとって(重要|不可欠)",
    r"を持つ(こと|存在)",
    r"することによって",
    r"であることは間違いない",
    r"に他ならない",
];

const HYPE_EXPRESSIONS: &[&str] = &[
    "革命的",
    "画期的な",
    "劇的に",
    "圧倒的な",
    "究極の",
    "最強の",
    "魔法のよう",
    "爆発的に",
];

/// 行内の一致範囲から、raw行の抜粋とspanを持つfindingを作る共通経路。
fn spanned_line_finding(
    raw_lines: &[&str],
    line_no: usize,
    byte_start: usize,
    byte_end: usize,
    category: &str,
    severity: &str,
    detail: String,
) -> Finding {
    let raw_line = raw_lines.get(line_no - 1).copied().unwrap_or_default();
    let mut finding = Finding::new(
        line_no,
        category,
        excerpt_around(raw_line, byte_start, byte_end - byte_start, 10).trim(),
        severity,
        detail,
    );
    finding.span = make_span(raw_lines, line_no, byte_start, line_no, byte_end);
    finding
}

pub(super) fn forbidden_findings(masked: &str, raw: &str) -> Vec<Finding> {
    let raw_lines = raw.split('\n').collect::<Vec<_>>();
    let mut findings = Vec::new();
    for (line_no, line) in numbered_lines(masked) {
        for phrase in FORBIDDEN_PHRASES {
            for (byte_start, _) in line.match_indices(phrase) {
                let weak = WEAK_FORBIDDEN_PHRASES.contains(phrase);
                let mut detail = format!("禁止語/LLM常套句ヒット: 「{phrase}」");
                if weak {
                    detail.push_str("（コーパス校正で人間側にも一定数出現する弱いシグナルと判定、severity低下）");
                }
                findings.push(spanned_line_finding(
                    &raw_lines,
                    line_no,
                    byte_start,
                    byte_start + phrase.len(),
                    "forbidden_phrase",
                    if weak { "info" } else { "warn" },
                    detail,
                ));
            }
        }
    }
    findings
}

pub(super) fn translationese_findings(masked: &str, raw: &str) -> Vec<Finding> {
    let raw_lines = raw.split('\n').collect::<Vec<_>>();
    let patterns = TRANSLATIONESE_PATTERNS
        .iter()
        .map(|pattern| {
            (
                *pattern,
                Regex::new(pattern).expect("valid translationese regex"),
            )
        })
        .collect::<Vec<_>>();
    let mut findings = Vec::new();
    for (line_no, line) in numbered_lines(masked) {
        for (pattern, regex) in &patterns {
            for found in regex.find_iter(line) {
                findings.push(spanned_line_finding(
                    &raw_lines,
                    line_no,
                    found.start(),
                    found.end(),
                    "translationese",
                    "info",
                    format!("翻訳調パターン: /{pattern}/ に一致"),
                ));
            }
        }
    }
    findings
}

pub(super) fn antithesis_findings(
    masked: &str,
    raw: &str,
    sentence_count: usize,
    critical_above: f64,
) -> Vec<Finding> {
    let raw_lines = raw.split('\n').collect::<Vec<_>>();
    let patterns = [
        Regex::new(r"ではなく、?.{0,30}").expect("valid antithesis regex"),
        Regex::new(r"だけでなく.{0,10}も").expect("valid antithesis regex"),
    ];
    let mut hits = Vec::<(usize, usize, usize, String)>::new();
    for (line_no, line) in numbered_lines(masked) {
        let raw_line = raw_lines.get(line_no - 1).copied().unwrap_or(line);
        for pattern in &patterns {
            for found in pattern.find_iter(line) {
                hits.push((
                    line_no,
                    found.start(),
                    found.end(),
                    raw_line
                        .get(found.start()..found.end())
                        .unwrap_or(found.as_str())
                        .trim()
                        .to_owned(),
                ));
            }
        }
    }
    if hits.len() < 3 {
        return Vec::new();
    }
    // 母数は「一致数」で統一する。閾値判定、表示件数、比率のすべてが
    // 同一行の複数一致を含む一致数を使い、位置は文書単位の1findingへ集約する。
    let ratio = if sentence_count == 0 {
        0.0
    } else {
        hits.len() as f64 / sentence_count as f64
    };
    let severity = if ratio < 0.02 {
        "info"
    } else if ratio >= critical_above {
        "critical"
    } else {
        "warn"
    };
    let related = hits
        .iter()
        .map(|(line, _, _, _)| *line)
        .collect::<BTreeSet<_>>();
    let related_text = related
        .iter()
        .map(|line| format!("L{line}"))
        .collect::<Vec<_>>()
        .join(", ");
    let over_100 = if ratio > 1.0 {
        "。同一文内の複数一致を含むため比率は100%を超える"
    } else {
        ""
    };
    let (line, byte_start, byte_end, excerpt) = hits[0].clone();
    let mut finding = Finding::new(
        line,
        "antithesis_repetition",
        excerpt,
        severity,
        format!(
            "否定→肯定対比パターンの一致が文書内で{}回（閾値3回以上、総文数{}に対する比率={:.1}%）。文書単位の集約finding。対応箇所: {related_text}{over_100}",
            hits.len(),
            sentence_count,
            ratio * 100.0
        ),
    );
    finding.span = make_span(&raw_lines, line, byte_start, line, byte_end);
    finding.related_lines = Some(related.into_iter().collect());
    vec![finding]
}

pub(super) fn english_syntax_findings(masked: &str, raw: &str, split: &[Sentence]) -> Vec<Finding> {
    let raw_lines = raw.split('\n').collect::<Vec<_>>();
    let patterns = [
        Regex::new(r"(これ|それ|この事実|そのこと)(は|が).{0,40}(もたらす|示す|意味する|証明する|生み出す|反映する)")
            .expect("valid inanimate-subject regex"),
        Regex::new(r".{0,20}(こと|事実)(は|が).{0,40}(もたらす|示す|意味する|証明する|生み出す|反映する)")
            .expect("valid inanimate-subject regex"),
    ];
    let mut findings = Vec::new();
    for (line_no, line) in numbered_lines(masked) {
        let raw_line = raw_lines.get(line_no - 1).copied().unwrap_or(line);
        for pattern in &patterns {
            for found in pattern.find_iter(line) {
                let mut finding = Finding::new(
                    line_no,
                    "english_syntax_inanimate_subject",
                    raw_line
                        .get(found.start()..found.end())
                        .unwrap_or(found.as_str()),
                    "info",
                    "無生物主語+他動詞的述語（表層パターン、英語統語の直訳調の可能性、要人間判断）",
                );
                finding.span = make_span(&raw_lines, line_no, found.start(), line_no, found.end());
                findings.push(finding);
            }
        }
    }

    let cleft = Regex::new(r"^(それ|これ|この)は.{0,60}(である|だ)$").expect("valid cleft regex");
    let because = Regex::new(r"^(なぜなら|というのも)").expect("valid because regex");
    for pair in split.windows(2) {
        let head = pair[0].text.as_str();
        let reason = pair[1].text.as_str();
        if cleft.is_match(head) && because.is_match(reason) {
            let mut finding = Finding::new(
                pair[0].line,
                "english_syntax_cleft_because",
                format!("{}。{}", pair[0].raw_text, pair[1].raw_text),
                "warn",
                "「それは〜である。なぜなら〜だ」型の強調構文（英語 It is ... because ... の直訳調）",
            );
            finding.span = make_span(
                &raw_lines,
                pair[0].line,
                pair[0].line_byte_start,
                pair[1].line,
                pair[1].line_byte_start + pair[1].text.len(),
            );
            findings.push(finding);
        }
    }
    findings
}

pub(super) fn structural_analysis(raw: &str) -> (Vec<Finding>, Value) {
    let bold = Regex::new(r"\*\*[^*\n]+\*\*").expect("valid bold regex");
    let non_blank = raw.lines().filter(|line| !line.trim().is_empty()).count();
    let bullet_count = raw
        .lines()
        .filter(|line| crate::text::is_list_item(line))
        .count();
    let boilerplate = raw
        .lines()
        .filter_map(crate::text::heading)
        .filter(|(_, text)| is_boilerplate_heading(text))
        .count();
    let phase = Regex::new(r"(フェーズ|ステップ|段階|ステージ)\s*[0-9０-９]")
        .expect("valid numbered-phase regex");
    let chars = raw.chars().count().max(1) as f64;
    let emoji_count = raw.chars().filter(|ch| is_emoji_symbol(*ch)).count();
    let bold_hits = bold.find_iter(raw).collect::<Vec<_>>();
    let phase_hits = phase.find_iter(raw).collect::<Vec<_>>();
    let mut findings = Vec::new();
    let bold_density = bold_hits.len() as f64 / chars * 1000.0;
    if bold_hits.len() >= 3 && bold_density >= 3.0 {
        let line = raw[..bold_hits[0].start()].matches('\n').count() + 1;
        findings.push(Finding::new(
            line,
            "high_bold_density",
            format!("太字スパン{}箇所（1000字あたり{bold_density:.2}）", bold_hits.len()),
            "info",
            "太字（**...**）の使用密度が閾値（1000字あたり3）以上。強調の多用は教科書的なAI生成文に見られる傾向（実験的検出器、閾値は暫定）",
        ));
    }
    if non_blank >= 10 && bullet_count as f64 / non_blank as f64 >= 0.35 {
        findings.push(Finding::new(
            1,
            "high_bullet_ratio",
            format!("箇条書き行{bullet_count}/{non_blank}行"),
            "info",
            "箇条書き行の比率が閾値35%以上。文章より箇条書きに頼る構成の疑い",
        ));
    }
    let raw_lines = raw.split('\n').collect::<Vec<_>>();
    for (line_no, line) in numbered_lines(raw) {
        if let Some((_, text)) = crate::text::heading(line)
            && is_boilerplate_heading(&text)
        {
            let mut finding = Finding::new(
                line_no,
                "boilerplate_heading",
                line.trim().chars().take(40).collect::<String>(),
                "info",
                format!(
                    "定型見出し「{text}」系での締め。予告・構成の型のみで中身を語らない教科書的なAI生成文に見られる傾向（実験的検出器）"
                ),
            );
            let start = line.len() - line.trim_start().len();
            let end = line.trim_end().len();
            finding.span = make_span(&raw_lines, line_no, start, line_no, end);
            findings.push(finding);
        }
    }
    if phase_hits.len() >= 3 {
        let line = raw[..phase_hits[0].start()].matches('\n').count() + 1;
        findings.push(Finding::new(
            line,
            "numbered_phase_structure",
            format!("番号付きフェーズ表現が{}回出現", phase_hits.len()),
            "info",
            "「フェーズ/ステップ/段階+番号」の表現が閾値3回以上。機械的な段階分割は教科書的なAI生成文に見られる傾向（実験的検出器）",
        ));
    }
    let emoji_density = emoji_count as f64 / chars * 1000.0;
    if emoji_count >= 3 && emoji_density >= 2.0 {
        findings.push(Finding::new(
            1,
            "high_emoji_symbol_density",
            format!("絵文字/装飾記号{emoji_count}箇所（1000字あたり{emoji_density:.2}）"),
            "info",
            "絵文字・装飾記号の使用密度が閾値以上（実験的検出器）",
        ));
    }
    let stats = json!({
        "bold_span_count": bold.find_iter(raw).count(),
        "bold_per_1000_chars": bold_density,
        "bullet_line_count": bullet_count,
        "non_blank_line_count": non_blank,
        "boilerplate_heading_count": boilerplate,
        "numbered_phase_hit_count": phase_hits.len(),
        "emoji_symbol_count": emoji_count,
        "emoji_symbol_per_1000_chars": emoji_count as f64 / chars * 1000.0,
    });
    (findings, stats)
}

fn is_boilerplate_heading(text: &str) -> bool {
    matches!(
        text.trim().to_lowercase().as_str(),
        "まとめ" | "おわりに" | "終わりに" | "さいごに" | "最後に" | "結論" | "総括" | "conclusion"
    )
}

fn is_emoji_symbol(ch: char) -> bool {
    matches!(ch as u32, 0x1F300..=0x1FAFF | 0x2600..=0x27BF)
        || matches!(ch, '⭐' | '✅' | '❌' | '❗' | '❓')
}

/// 短い文書や一箇所だけ強く目立つAI記述パターンを、文書全体の密度とは
/// 独立に検出する。密度カテゴリ(high_bullet_ratio等)と同じ箇所を指す場合も
/// 双方を報告する。答える問いが「文書の均一さ」と「局所の記述」で異なるため、
/// 二重報告は抑制しない。
pub(super) fn local_pattern_findings(
    raw: &str,
    morphology: &Morphology,
) -> Result<Vec<Finding>, Error> {
    let masked = mask_html_comments(raw);
    let raw_lines = raw.split('\n').collect::<Vec<_>>();
    let bullet_marker =
        Regex::new(r"^\s*(?:[-*+]|[0-9]+[.)])\s+").expect("valid bullet-marker regex");
    let bold_label = Regex::new(r"\*\*[^*\n]+\*\*\s*[:：]").expect("valid bold-label regex");

    let mut findings = Vec::new();
    let mut bold_hits = Vec::<(usize, usize, usize)>::new();
    let mut emoji_hits = Vec::<(usize, usize, usize)>::new();
    let mut fence: Option<(char, usize)> = None;
    let lines = masked.split('\n').collect::<Vec<_>>();
    for (index, line) in lines.iter().enumerate() {
        let line_no = index + 1;
        let trimmed = line.trim_start();
        let fence_run = trimmed
            .chars()
            .next()
            .filter(|ch| *ch == '`' || *ch == '~')
            .map(|ch| (ch, trimmed.chars().take_while(|c| c == &ch).count()));
        if let Some((open_char, open_len)) = fence {
            if fence_run.is_some_and(|(ch, len)| ch == open_char && len >= open_len) {
                fence = None;
            }
            continue;
        }
        if let Some((ch, len)) = fence_run.filter(|(_, len)| *len >= 3) {
            fence = Some((ch, len));
            continue;
        }
        if trimmed.starts_with('>') {
            continue;
        }

        if crate::text::is_list_item(line)
            && let Some(marker) = bullet_marker.find(line)
        {
            if let Some(found) = bold_label.find(&line[marker.end()..])
                && found.start() == 0
            {
                bold_hits.push((line_no, marker.end(), marker.end() + found.end()));
            }
            if line[marker.end()..]
                .chars()
                .next()
                .is_some_and(is_emoji_symbol)
            {
                let ch_len = line[marker.end()..]
                    .chars()
                    .next()
                    .map_or(0, char::len_utf8);
                emoji_hits.push((line_no, marker.end(), marker.end() + ch_len));
            }
        }

        // 述語+コロンでブロックへ接続する導入行。名詞ラベル(「使用方法:」)は対象外。
        if !crate::text::is_heading(line) && !crate::text::is_list_item(line) {
            let content = line.trim_end();
            if let Some(colon_stripped) = content
                .strip_suffix('：')
                .or_else(|| content.strip_suffix(':'))
            {
                let label = colon_stripped.trim();
                let next_block_starts = lines[index + 1..]
                    .iter()
                    .find(|next| !next.trim().is_empty())
                    .is_some_and(|next| {
                        let next_trimmed = next.trim_start();
                        crate::text::is_list_item(next)
                            || next_trimmed.starts_with("```")
                            || next_trimmed.starts_with("~~~")
                            || next_trimmed.starts_with('|')
                    });
                if !label.is_empty() && label.chars().count() <= 40 && next_block_starts {
                    let tokens = morphology.tokenize(label)?;
                    let predicate_ending = tokens
                        .iter()
                        .rev()
                        .find(|token| !matches!(token.pos(0), "記号" | "補助記号" | "空白"))
                        .is_some_and(|token| matches!(token.pos(0), "動詞" | "助動詞"));
                    if predicate_ending {
                        let start = line.len() - line.trim_start().len();
                        let end = content.len();
                        let raw_line = raw_lines.get(index).copied().unwrap_or(*line);
                        let mut finding = Finding::new(
                            line_no,
                            "predicate_colon_lead",
                            raw_line.trim().chars().take(40).collect::<String>(),
                            "info",
                            "述語の直後にコロンを置いてブロックへ接続する構成。教科書的なAI生成文に多い。「使用方法:」のような名詞ラベル、または「次を実行します。」のような文への言い換えを検討する",
                        );
                        finding.span = make_span(&raw_lines, line_no, start, line_no, end);
                        findings.push(finding);
                    }
                }
            }
        }

        for phrase in HYPE_EXPRESSIONS {
            for (byte_start, _) in line.match_indices(phrase) {
                findings.push(spanned_line_finding(
                    &raw_lines,
                    line_no,
                    byte_start,
                    byte_start + phrase.len(),
                    "hype_expression",
                    "info",
                    format!(
                        "誇張表現の確認候補: 「{phrase}」。文脈なしの禁止ではない。事実や固有の主張に基づくなら、.suiko.tomlのallowで理由を記録して維持する"
                    ),
                ));
            }
        }
    }

    for (category, hits, description) in [
        (
            "bullet_bold_label",
            bold_hits,
            "太字ラベル+コロンで始まる箇条書き",
        ),
        ("bullet_emoji", emoji_hits, "絵文字で始まる箇条書き"),
    ] {
        if hits.is_empty() {
            continue;
        }
        let lines_hit = hits
            .iter()
            .map(|(line, _, _)| *line)
            .collect::<BTreeSet<_>>();
        let related = lines_hit
            .iter()
            .map(|line| format!("L{line}"))
            .collect::<Vec<_>>()
            .join(", ");
        let (line_no, byte_start, byte_end) = hits[0];
        let raw_line = raw_lines.get(line_no - 1).copied().unwrap_or_default();
        let mut finding = Finding::new(
            line_no,
            category,
            raw_line.trim().chars().take(40).collect::<String>(),
            "info",
            format!(
                "{description}が{}行ある。文書単位の集約finding。教科書的なAI生成文に多い装飾で、密度カテゴリ(high_bullet_ratio等)とは独立の局所パターンとして報告する。対応箇所: {related}",
                lines_hit.len()
            ),
        );
        finding.span = make_span(&raw_lines, line_no, byte_start, line_no, byte_end);
        finding.related_lines = Some(lines_hit.into_iter().collect());
        findings.push(finding);
    }
    Ok(findings)
}
