//! 表層（文字列・正規表現）ベースの検出器。行単位でmasked本文を走査し、
//! 抜粋とspanはbyteレイアウトが同一のraw行から作る。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Value, json};

use crate::Error;
use crate::morphology::Morphology;
use crate::text::{
    Sentence, excerpt_around, fenced_lines, mask_html_comments, numbered_lines, protected_lines,
};

use super::{Finding, make_span};

const BULLET_MARKER_PATTERN: &str = r"^\s*(?:[-*+]|[0-9]+[.)])\s+";

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
    "本記事では",
];

/// 語彙の実測(suiko-eval vocab)用の読み取り専用アクセサ。
pub fn forbidden_phrase_list() -> &'static [&'static str] {
    FORBIDDEN_PHRASES
}

pub fn hype_expression_list() -> &'static [&'static str] {
    HYPE_EXPRESSIONS
}

// 「のではないでしょうか」は2026-08-19のvocab実測(現代人間dev 65文書中6文書)で
// 人間の常用と確認し、弱いシグナルへ落とした(suiko-eval/eval/calibration.md)。
const WEAK_FORBIDDEN_PHRASES: &[&str] = &[
    "重要なのは",
    "このように",
    "不可欠",
    "ポイントは",
    "さて、",
    "のではないでしょうか",
    "本記事では",
];

const TRANSLATIONESE_PATTERNS: &[&str] = &[
    r"することができ(る|ます|た)",
    r"することが可能(です|だ|になる)",
    r"と言えるだろう",
    r"という点で",
    r"という観点(から|で)",
    r"にとって(重要|不可欠)",
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
                // 「要望をまとめるときも」の「まとめると」は、話をまとめる前置きではない。
                if phrase.ends_with('と') && line[byte_start + phrase.len()..].starts_with('き') {
                    continue;
                }
                if matches!(*phrase, "いかがでしたか" | "いかがでしょうか")
                    && has_question_topic(&line[..byte_start])
                {
                    continue;
                }
                let empty_preview = matches!(*phrase, "重要なのは" | "大切なのは" | "ポイントは")
                    && empty_evaluation_preview(&line[byte_start + phrase.len()..]);
                let weak = WEAK_FORBIDDEN_PHRASES.contains(phrase) && !empty_preview;
                let mut detail = format!(
                    "「{phrase}」という定型表現があります。評価・留保・話題のつながりを担っているなら残し、なくても意味が変わらない場合だけ省けるか確認してください。"
                );
                if weak {
                    detail.push_str(
                        "人が書いた文章にも見られる表現なので、出現だけで不自然とは判断しません。",
                    );
                }
                if empty_preview {
                    detail = "評価の中身を示さず、次の説明だけを予告しています。何を重視するかを本文で述べるか、直後の説明から始められるか確認してください。".into();
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

fn empty_evaluation_preview(after: &str) -> bool {
    static PREVIEW: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"^[\s、]*(?:以下|次|下記)の(?:点|こと|とおり|通り)(?:です|だ|である)?[。！!：:\s]*$",
        )
        .expect("valid empty preview regex")
    });
    let sentence = after
        .split_inclusive(['。', '！', '!'])
        .next()
        .unwrap_or(after);
    PREVIEW.is_match(sentence)
}

/// 「見積もりは、いかがでしたか」のように質問の対象を明示した文は残す。
/// 文をまたいだ照応や、対象を省略した問いかけの意図までは判定しない。
fn has_question_topic(before: &str) -> bool {
    let head = before
        .rsplit(['。', '！', '？', '!', '?'])
        .next()
        .unwrap_or_default()
        .trim_end_matches(|ch: char| ch.is_whitespace() || ch == '、');
    head.strip_suffix('は')
        .or_else(|| head.strip_suffix("について"))
        .is_some_and(|topic| !topic.trim().is_empty())
}

/// 同じ語の左右を別々に比較し、媒体による空白の方針を決めつけない。
pub(super) fn mixed_latin_spacing_findings(masked: &str, raw_lines: &[&str]) -> Vec<Finding> {
    struct Occurrence {
        line: usize,
        start: usize,
        end: usize,
        spacing: [Option<bool>; 2],
    }

    // URL・パス等を一まとまりで取り出し、途中の英単語だけを比較しない。
    let word = Regex::new(r"[A-Za-z0-9_./:@%+?#=$~\-]+").expect("valid Latin-word regex");
    let japanese = Regex::new(r"^[\p{Hiragana}\p{Katakana}\p{Han}ー]$")
        .expect("valid Japanese-character regex");
    let mut words = BTreeMap::<&str, Vec<Occurrence>>::new();
    for (line_no, line) in numbered_lines(masked) {
        for found in word.find_iter(line) {
            let term = found.as_str();
            if term.len() < 2
                || !term.as_bytes()[0].is_ascii_alphabetic()
                || !term
                    .bytes()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'-'))
            {
                continue;
            }
            // マスク由来の空白を数えず、英語の複合語やコードとの境界も除く。
            let raw = raw_lines[line_no - 1];
            let before = &raw[..found.start()];
            let after = &raw[found.end()..];
            let left = before.trim_end_matches(' ');
            let right = after.trim_start_matches(' ');
            let neighbors = [left.chars().next_back(), right.chars().next()];
            if neighbors
                .iter()
                .flatten()
                .any(|ch| ch.is_ascii_alphanumeric() || *ch == '`')
            {
                continue;
            }
            let spaces = [before.len() != left.len(), after.len() != right.len()];
            let spacing = std::array::from_fn(|index| {
                neighbors[index]
                    .filter(|ch| japanese.is_match(&ch.to_string()))
                    .map(|_| spaces[index])
            });
            if spacing == [None, None] {
                continue;
            }
            words.entry(term).or_default().push(Occurrence {
                line: line_no,
                start: found.start(),
                end: found.end(),
                spacing,
            });
        }
    }

    let mut findings = Vec::new();
    for (term, occurrences) in words {
        let mixed: [bool; 2] = std::array::from_fn(|side| {
            occurrences
                .iter()
                .any(|hit| hit.spacing[side] == Some(true))
                && occurrences
                    .iter()
                    .any(|hit| hit.spacing[side] == Some(false))
        });
        let relevant = occurrences
            .iter()
            .filter(|hit| (0..2).any(|side| mixed[side] && hit.spacing[side].is_some()))
            .collect::<Vec<_>>();
        let Some(first) = relevant.first() else {
            continue;
        };
        let mut finding = Finding::new(
            first.line,
            "mixed_latin_spacing",
            term,
            "info",
            format!(
                "「{term}」と日本語の境界で半角空白の有無が混在する。媒体の表記方針を確認する。空白あり・なしのどちらも選べる"
            ),
        );
        finding.span = make_span(raw_lines, first.line, first.start, first.line, first.end);
        finding.related_lines = Some(
            relevant
                .iter()
                .map(|hit| hit.line)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
        );
        findings.push(finding);
    }
    findings
}

pub(super) fn boilerplate_closing_findings(
    sentences: &[Sentence],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let Some(last) = sentences.last() else {
        return Vec::new();
    };
    let closing =
        Regex::new(r"^(?:いかがでしたでしょうか|ぜひ(?:参考に|活用|試)して(?:みて)?ください)$")
            .expect("valid closing regex");
    if !closing.is_match(&last.text) || last.text != last.raw_text {
        return Vec::new();
    }
    vec![spanned_line_finding(
        raw_lines,
        last.line,
        last.line_byte_start,
        last.line_byte_start + last.text.len(),
        "boilerplate_closing",
        "info",
        "本文の最後の文が定型的な締め言葉になっている。読者への働きかけが必要か、省いても内容が伝わるかを確認する".to_owned(),
    )]
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
    tokenized: &[super::morph::TokenizedSentence],
    sentence_count: usize,
    critical_above: f64,
) -> Vec<Finding> {
    let raw_lines = raw.split('\n').collect::<Vec<_>>();
    let patterns = [
        Regex::new(r"ではなく").expect("valid antithesis regex"),
        Regex::new(r"だけでなく.{0,10}も").expect("valid antithesis regex"),
    ];
    let temporal_starts = tokenized
        .iter()
        .flat_map(|sentence| {
            sentence
                .tokens
                .iter()
                .enumerate()
                .filter_map(|(index, token)| {
                    let temporal = token.dictionary_form() == "なくなる"
                        || (token.surface == "なく"
                            && sentence.tokens.get(index + 1).is_some_and(|next| {
                                next.dictionary_form() == "なる"
                                    && token.byte_end == next.byte_start
                            }));
                    temporal.then_some((sentence.line, sentence.line_byte_start + token.byte_start))
                })
        })
        .collect::<BTreeSet<_>>();
    let mut hits = Vec::<(usize, usize, usize, String)>::new();
    for (line_no, line) in numbered_lines(masked) {
        let raw_line = raw_lines.get(line_no - 1).copied().unwrap_or(line);
        for pattern in &patterns {
            for found in pattern.find_iter(line) {
                // 状態変化は除外するが、同じ行の後続の対比は走査し続ける。
                let end = if found.as_str() == "ではなく" {
                    if temporal_starts.contains(&(line_no, found.start() + "では".len())) {
                        continue;
                    }
                    let tail = &line[found.end()..];
                    found.end()
                        + tail
                            .chars()
                            .take(30 + usize::from(tail.starts_with('、')))
                            .map(char::len_utf8)
                            .sum::<usize>()
                } else {
                    found.end()
                };
                hits.push((
                    line_no,
                    found.start(),
                    end,
                    raw_line
                        .get(found.start()..end)
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
                // 「それは、〜を証明することとは別です」「これが示すのは」のように「こと」「の」で
                // 名詞化された動詞は主節の述語ではない。説明の「のである」は述語のまま扱う。
                let rest = &line[found.end()..];
                if rest.starts_with("こと")
                    || ["のは", "のが", "のを", "のも"]
                        .iter()
                        .any(|nominal| rest.starts_with(nominal))
                {
                    continue;
                }
                let mut finding = Finding::new(
                    line_no,
                    "english_syntax_inanimate_subject",
                    raw_line
                        .get(found.start()..found.end())
                        .unwrap_or(found.as_str()),
                    "info",
                    "主語が何を指すか、述語との関係が読み取れるか確認してください。事実やデータが結果を示す客観的な説明なら、そのまま残せます。無生物主語だけを理由に人の主語へ変えないでください。",
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

pub(super) fn technical_surface_findings(raw: &str, genre: Option<&str>) -> Vec<Finding> {
    let raw_lines = raw.split('\n').collect::<Vec<_>>();
    let masked = mask_html_comments(raw);
    let protected = protected_lines(&raw_lines);
    let mut findings = Vec::new();
    for (index, line) in masked.split('\n').enumerate() {
        if protected[index] {
            continue;
        }
        if genre == Some("tech")
            && let Some((_, title)) = crate::text::heading(line)
            && title.chars().next().is_some_and(is_emoji_symbol)
        {
            let start = line.find(&title).unwrap_or(0);
            let end = start + title.split_whitespace().next().unwrap_or_default().len();
            findings.push(spanned_line_finding(&raw_lines, index + 1, start, end, "heading_emoji", "info", "見出し先頭の絵文字が情報の識別に必要か確認してください。製品名や正式な題名に含まれる場合は残せます。".to_owned()));
        }
        let visible = crate::text::mask_inline_spans(line);
        for (start, ch) in visible.char_indices() {
            if !matches!(ch, '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}')
                || index == 0 && start == 0 && ch == '\u{feff}'
            {
                continue;
            }
            if matches!(ch, '\u{200c}' | '\u{200d}') {
                let previous = visible[..start].chars().next_back();
                let next = visible[start + ch.len_utf8()..].chars().next();
                let japanese = |c: char| matches!(c, 'ぁ'..='ん' | 'ァ'..='ヶ' | '一'..='鿿');
                if previous.is_some_and(is_emoji_symbol)
                    || next.is_some_and(is_emoji_symbol)
                    || !previous.is_some_and(japanese) && !next.is_some_and(japanese)
                {
                    continue;
                }
            }
            findings.push(spanned_line_finding(&raw_lines, index + 1, start, start + ch.len_utf8(), "invisible_character", "info", format!("見えない文字 U+{:04X} があります。コピーや検索を妨げる混入なら削除してください。意図した改行・結合制御なら残せます。", ch as u32)));
        }
    }
    findings
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
    let bullet_marker = Regex::new(BULLET_MARKER_PATTERN).expect("valid bullet-marker regex");
    let bold_label = Regex::new(r"\*\*[^*\n]+\*\*\s*[:：]").expect("valid bold-label regex");

    let mut findings = Vec::new();
    let mut bold_hits = Vec::<(usize, usize, usize)>::new();
    let mut emoji_hits = Vec::<(usize, usize, usize)>::new();
    let lines = masked.split('\n').collect::<Vec<_>>();
    let fenced = fenced_lines(&lines);
    for (index, line) in lines.iter().enumerate() {
        let line_no = index + 1;
        let trimmed = line.trim_start();
        if fenced[index] {
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
                let label = found.as_str().split("**").nth(1).unwrap_or_default();
                let identifier = label.starts_with('`') && label.ends_with('`')
                    || (!label.is_empty()
                        && label.chars().all(|ch| {
                            ch.is_ascii_alphanumeric() || matches!(ch, '/' | '-' | '_' | '.' | ':')
                        }));
                if !identifier {
                    bold_hits.push((line_no, marker.end(), marker.end() + found.end()));
                }
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
                if !label.is_empty() && next_block_starts {
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
                            "述語の直後にコロンを置いてブロックへ接続しています。「使用方法:」のような名詞ラベル、または「次を実行します。」のような文に整えられるか確認してください。",
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
                "{description}が{}行あります。項目の区別に役立つ名称や方式の説明は残せます。装飾だけのラベルが続く場合に見直してください。対応箇所: {related}",
                lines_hit.len()
            ),
        );
        finding.span = make_span(&raw_lines, line_no, byte_start, line_no, byte_end);
        finding.related_lines = Some(lines_hit.into_iter().collect());
        findings.push(finding);
    }
    Ok(findings)
}

pub(super) fn uniform_bullet_structure_findings(
    raw: &str,
    morphology: &Morphology,
) -> Result<Vec<Finding>, Error> {
    let masked = mask_html_comments(raw);
    let raw_lines = raw.split('\n').collect::<Vec<_>>();
    let bullet_marker = Regex::new(BULLET_MARKER_PATTERN).expect("valid bullet-marker regex");
    let mut groups = Vec::new();
    let mut current = Vec::new();
    let lines = masked.split('\n').collect::<Vec<_>>();
    let fenced = fenced_lines(&lines);

    for (index, line) in lines.into_iter().enumerate() {
        let line_no = index + 1;
        let trimmed = line.trim_start();
        if fenced[index] {
            if !current.is_empty() {
                groups.push(std::mem::take(&mut current));
            }
            continue;
        }

        let Some(marker) = bullet_marker
            .find(line)
            .filter(|_| !trimmed.starts_with('>'))
        else {
            if !current.is_empty() {
                groups.push(std::mem::take(&mut current));
            }
            continue;
        };
        let content = line[marker.end()..].trim_end();
        let tokens = morphology.tokenize(content)?;
        let content_count = tokens
            .iter()
            .filter(|token| super::morph::CONTENT_POS.contains(&token.pos(0)))
            .count();
        let terminal_pos = tokens
            .iter()
            .rev()
            .find(|token| !matches!(token.pos(0), "記号" | "補助記号" | "空白"))
            .map(|token| token.pos(0).to_owned());
        if content_count == 0 || terminal_pos.is_none() {
            if !current.is_empty() {
                groups.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push((
            line_no,
            marker.end(),
            marker.end() + content.len(),
            content_count,
            terminal_pos.expect("checked terminal POS"),
        ));
    }
    if !current.is_empty() {
        groups.push(current);
    }

    let matches = groups
        .into_iter()
        .filter_map(|group| {
            if group.len() < 4 {
                return None;
            }
            let terminal = &group[0].4;
            if group.iter().any(|item| &item.4 != terminal) {
                return None;
            }
            let mean = group.iter().map(|item| item.3 as f64).sum::<f64>() / group.len() as f64;
            let variance = group
                .iter()
                .map(|item| {
                    let delta = item.3 as f64 - mean;
                    delta * delta
                })
                .sum::<f64>()
                / group.len() as f64;
            let cv = variance.sqrt() / mean;
            (cv <= 0.25).then_some((group, cv))
        })
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return Ok(Vec::new());
    }

    let related_lines = matches
        .iter()
        .flat_map(|(group, _)| group.iter().map(|item| item.0))
        .collect::<Vec<_>>();
    let related = related_lines
        .iter()
        .map(|line| format!("L{line}"))
        .collect::<Vec<_>>()
        .join(", ");
    let first_group = &matches[0].0;
    let first = &first_group[0];
    let last = &first_group[first_group.len() - 1];
    let excerpt = raw_lines
        .get(first.0 - 1)
        .copied()
        .unwrap_or_default()
        .trim()
        .chars()
        .take(40)
        .collect::<String>();
    let mut finding = Finding::new(
        first.0,
        "uniform_bullet_structure",
        excerpt,
        "info",
        format!(
            "4項目以上の箇条書きで文末品詞が「{}」に揃い、内容語数の変動係数が{:.3}（閾値0.25以下）。形と長さが均一すぎないか確認する実験的検出。対応箇所: {related}",
            first.4, matches[0].1
        ),
    );
    finding.related_lines = Some(related_lines);
    finding.span = make_span(&raw_lines, first.0, first.1, last.0, last.2);
    Ok(vec![finding])
}

// ---- 太字が表示されない書き方 ----

fn is_markdown_whitespace(ch: Option<char>) -> bool {
    ch.is_none_or(char::is_whitespace)
}

/// 太字の内側が句読点・括弧・記号で、外側が文字に接しているため、GitHub等の
/// CommonMark系レンダラーで`**`がそのまま表示される箇所。コードブロックと
/// インラインコードの中は見ない。
pub(super) fn bold_not_rendered_findings(raw: &str, raw_lines: &[&str]) -> Vec<Finding> {
    let protected = protected_lines(raw_lines);
    let mut findings = Vec::new();
    for (line_no, line) in numbered_lines(raw) {
        if protected[line_no - 1] {
            continue;
        }
        let unrendered = crate::text::mask_emphasis_delimiters(line);
        for bold in crate::markup::bold_ranges(line) {
            if unrendered[bold.start..bold.start + 2] == *"  "
                && unrendered[bold.end - 2..bold.end] == *"  "
            {
                continue;
            }
            let text = &line[bold.clone()];
            let underscore = text.starts_with('_');
            let inner = text.trim_matches(['*', '_']);
            let previous = line[..bold.start].chars().next_back();
            let next = line[bold.end..].chars().next();
            if crate::markup::bold_renders(
                previous,
                inner.chars().next(),
                inner.chars().next_back(),
                next,
                underscore,
            ) && unrendered == line
            {
                continue;
            }
            let detail = if is_markdown_whitespace(inner.chars().next())
                || is_markdown_whitespace(inner.chars().next_back())
            {
                "太字の内側が空白で始まるか終わるため、CommonMark系の表示では太字にならず記号がそのまま出ます。空白を太字の外へ出してください。"
            } else if underscore {
                "`__`は語の途中では強調になりません。日本語の文字に接する場合は`**`を使うか、両側に半角空白を置いてください。"
            } else {
                "太字の内側が括弧や記号で、外側が文字に接しているため、GitHubなどCommonMark系の表示では太字にならず`**`がそのまま出ます。括弧や記号を太字の外へ出すか、文字に接する側に半角空白を置いてください。"
            };
            let mut finding = Finding::new(line_no, "bold_not_rendered", text, "warn", detail);
            finding.span = make_span(raw_lines, line_no, bold.start, line_no, bold.end);
            findings.push(finding);
        }
    }
    findings
}

pub(super) fn duplicate_comma_findings(masked: &str, raw_lines: &[&str]) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut ranges = crate::markup::duplicate_commas(masked)
        .into_iter()
        .peekable();
    let mut offset = 0;
    for (line_no, line) in numbered_lines(masked) {
        while ranges
            .peek()
            .is_some_and(|range| range.start < offset + line.len())
        {
            let range = ranges.next().expect("peeked comma range");
            findings.push(spanned_line_finding(raw_lines, line_no, range.start - offset, range.end - offset,
                "duplicate_comma", "warn", "読点が連続しています。入力や修正時の重複であれば一つにしてください。意図した間や表記の例は残せます。".into()));
        }
        offset += line.len() + 1;
    }
    findings
}

// ---- 書き手への指示や作業報告の残り ----

static RESIDUE_OPENER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:この記事|本記事|本稿|この文書|本文書|このブログ|ここ|以下|今回)(?:では|は)")
        .expect("valid residue opener regex")
});
static RESIDUE_STYLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?:トーン|文体|口調|調子)(?:で|を意識|に)|",
        r"(?:専門用語|箇条書き|難しい言葉|カタカナ語)を(?:できるだけ|なるべく|極力)?(?:使わず|用いず|避け|控え)|",
        r"(?:AI|ＡＩ)(?:っぽさ|らしさ)|飾らない言葉で|語り口で|",
        r"[0-9０-９]+(?:文字|字)(?:程度|以内|前後|ほど)?で|",
        r"(?:です・ます調|だ・である調|敬体|常体)(?:で|に)",
    ))
    .expect("valid residue style regex")
});
static RESIDUE_WRITE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:書き|説明し|紹介し|解説し|まとめ|お伝えし|述べ|お届けし|話し|語り)(?:ます|ていきます|ていく|ましょう)$")
        .expect("valid residue write regex")
});
static RESIDUE_RESULT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?:推敲|改稿|校正|添削|リライト|書き換え|書き直し|修正)(?:後|版|案|結果)の",
        r"(?:文章|本文|原稿|テキスト)(?:は|を)?(?:以下|次)(?:の|に)?(?:とおり|通り|示し|掲載|記載)",
    ))
    .expect("valid residue result regex")
});
static RESIDUE_REQUEST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:ご要望|ご指示|ご依頼|指示|要望)(?:のとおり|の通り|に沿って|どおり|通り|に従い|に従って|を踏まえ)")
        .expect("valid residue request regex")
});
static RESIDUE_SOURCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?:原文|元の文章|元の文|元の原稿)(?:の(?:意味|内容|構成|趣旨|ニュアンス))?(?:は|を)",
        r"(?:保ち|保っ|保持し|変えず|維持し|損なわず|崩さず)",
    ))
    .expect("valid residue source regex")
});
static RESIDUE_EDIT_DONE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?:推敲|改稿|校正|添削|リライト|書き換え|書き直し|修正|統一|調整|整理|整え)",
        r"(?:しました|いたしました|を行いました|を加えました|しています|しております|済みです)",
    ))
    .expect("valid residue edit regex")
});
static RESIDUE_MARKER_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*【?(?:修正|推敲|改稿|校正|添削)(?:後|版|案|結果)】?\s*[:：]?\s*$")
        .expect("valid residue marker regex")
});
static RESIDUE_MARKER_INLINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"【要確認】|\[要確認\]|（要確認）|\(要確認\)|<<<")
        .expect("valid residue inline regex")
});

const RESIDUE_DETAIL: &str = "書き手への指示や推敲の作業報告が本文に残っている可能性があります。読者に向けた文でなければ、公開前に取り除いてください。";

/// 依頼文の書き方の条件を本文で述べ直した文、推敲の作業報告、修正版の見出しや
/// 確認メモの印。コード・引用・箇条書きの中は見ない。
pub(super) fn instruction_residue_findings(split: &[Sentence], raw_lines: &[&str]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for sentence in split {
        let text = sentence.text.as_str();
        let instruction = RESIDUE_OPENER.is_match(text)
            && RESIDUE_STYLE.is_match(text)
            && RESIDUE_WRITE.is_match(text);
        let report = RESIDUE_RESULT.is_match(text)
            || ((RESIDUE_REQUEST.is_match(text) || RESIDUE_SOURCE.is_match(text))
                && RESIDUE_EDIT_DONE.is_match(text));
        if !(instruction || report) {
            continue;
        }
        let start = sentence.line_byte_start;
        findings.push(spanned_line_finding(
            raw_lines,
            sentence.line,
            start,
            start + text.len(),
            "instruction_residue",
            "info",
            RESIDUE_DETAIL.to_owned(),
        ));
    }
    let protected = protected_lines(raw_lines);
    for (index, line) in raw_lines.iter().enumerate() {
        let line_no = index + 1;
        if protected[index] || line.trim_start().starts_with('>') {
            continue;
        }
        if RESIDUE_MARKER_LINE.is_match(line) {
            let start = line.len() - line.trim_start().len();
            let end = line.trim_end().len();
            let mut finding = Finding::new(
                line_no,
                "instruction_residue",
                line.trim(),
                "info",
                RESIDUE_DETAIL,
            );
            finding.span = make_span(raw_lines, line_no, start, line_no, end);
            findings.push(finding);
            continue;
        }
        for marker in RESIDUE_MARKER_INLINE.find_iter(line) {
            findings.push(spanned_line_finding(
                raw_lines,
                line_no,
                marker.start(),
                marker.end(),
                "instruction_residue",
                "info",
                RESIDUE_DETAIL.to_owned(),
            ));
        }
    }
    findings.sort_by_key(|finding| (finding.line, finding.span.map(|span| span.start_byte)));
    findings
}
