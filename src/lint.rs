use std::collections::{BTreeMap, BTreeSet};

use regex::Regex;
use serde::Serialize;
use serde_json::{Value, json};

use crate::Error;
use crate::morphology::{Morpheme, Morphology};
use crate::text::{
    Sentence, excerpt_around, mask_html_comments, mask_markdown_structure, numbered_lines,
    sentences, sentences_with_raw,
};

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

const EXPERIMENTAL_CATEGORIES: &[&str] = &[
    "high_length_autocorrelation",
    "paragraph_lead_conjunction",
    "repeated_syntax_template",
    "english_syntax_cleft_because",
    "high_bold_density",
    "high_bullet_ratio",
    "boilerplate_heading",
    "numbered_phase_structure",
    "high_emoji_symbol_density",
];

const RULE_CATEGORIES: &[&str] = &[
    "antithesis_repetition",
    "boilerplate_heading",
    "buried_list",
    "double_negative",
    "english_syntax_cleft_because",
    "english_syntax_inanimate_subject",
    "forbidden_phrase",
    "high_bold_density",
    "high_bullet_ratio",
    "high_emoji_symbol_density",
    "high_length_autocorrelation",
    "inanimate_subject_morph",
    "kanji_run",
    "low_burstiness",
    "low_lexical_diversity_mtld",
    "low_lexical_diversity_ttr",
    "low_sentence_variance",
    "low_specificity",
    "no_chain",
    "nominal_ending",
    "numbered_phase_structure",
    "paragraph_lead_conjunction",
    "repeated_sentence_lead",
    "repeated_syntax_template",
    "sentence_too_long",
    "translationese",
    "translationese_morph",
    "uniform_paragraph_structure",
];

const READING_LOAD_CATEGORIES: &[&str] = &[
    "buried_list",
    "double_negative",
    "kanji_run",
    "no_chain",
    "sentence_too_long",
];

pub fn is_known_rule(category: &str) -> bool {
    RULE_CATEGORIES.contains(&category)
}

pub fn rule_categories() -> &'static [&'static str] {
    RULE_CATEGORIES
}

pub fn reading_load_categories() -> &'static [&'static str] {
    READING_LOAD_CATEGORIES
}

const CONTENT_POS: &[&str] = &["名詞", "動詞", "形容詞", "副詞"];
const TRANSITIVE_SMELL_VERBS: &[&str] = &[
    "もたらす",
    "示す",
    "意味する",
    "証明する",
    "生み出す",
    "反映する",
    "示唆する",
    "物語る",
    "浮き彫りにする",
    "後押しする",
];
const ABSTRACT_NOUNS: &[&str] = &[
    "側面",
    "観点",
    "重要性",
    "可能性",
    "あり方",
    "存在",
    "意味",
    "本質",
    "価値",
    "意義",
    "課題",
    "問題",
    "要素",
    "要因",
    "背景",
    "傾向",
    "姿勢",
    "視点",
    "概念",
    "特徴",
    "性質",
    "状況",
    "状態",
    "変化",
];
const EXAMPLE_MARKERS: &[&str] = &[
    "たとえば",
    "例えば",
    "実際に",
    "実際には",
    "具体的には",
    "具体例として",
    "一例として",
    "先日",
    "昨日",
    "現に",
    "実例として",
];
const PARAGRAPH_CONJUNCTIONS: &[&str] = &[
    "しかし",
    "また",
    "そして",
    "そのため",
    "さらに",
    "つまり",
    "一方",
    "一方で",
    "このように",
    "なぜなら",
    "したがって",
    "ただし",
];

#[derive(Clone, Debug, Serialize)]
pub struct Finding {
    pub line: usize,
    pub category: String,
    pub excerpt: String,
    pub severity: String,
    pub detail: String,
    pub related_lines: Option<Vec<usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

impl Finding {
    fn new(
        line: usize,
        category: &str,
        excerpt: impl Into<String>,
        severity: &str,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            line,
            category: category.to_owned(),
            excerpt: excerpt.into(),
            severity: severity.to_owned(),
            detail: detail.into(),
            related_lines: None,
            status: None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct LintStats {
    pub total_findings: usize,
    pub by_category: BTreeMap<String, usize>,
    pub genre: Option<String>,
    pub experimental: bool,
    #[serde(flatten)]
    pub measurements: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LintReport {
    pub stats: LintStats,
    pub findings: Vec<Finding>,
}

#[derive(Clone, Copy, Debug)]
pub struct AnalysisThresholds {
    pub repeated_sentence_lead: Option<usize>,
    pub lexical_ttr: f64,
    pub lexical_mtld: f64,
}

impl Default for AnalysisThresholds {
    fn default() -> Self {
        Self {
            repeated_sentence_lead: None,
            lexical_ttr: 0.45,
            lexical_mtld: 40.0,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ReadingLoadStats {
    pub total: usize,
    pub sentences: usize,
    pub genre: Option<String>,
    pub by_category: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReadingLoadReport {
    pub stats: ReadingLoadStats,
    pub findings: Vec<Finding>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BaselineSummary {
    pub resolved: usize,
    pub new: usize,
    pub persisting: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct BaselineReport {
    pub file: String,
    pub summary: BaselineSummary,
    pub resolved: Vec<Value>,
}

fn baseline_key(category: &str, excerpt: &str) -> (String, String) {
    const CATEGORY_ONLY: &[&str] = &[
        "low_burstiness",
        "high_length_autocorrelation",
        "low_sentence_variance",
        "uniform_paragraph_structure",
        "low_lexical_diversity_ttr",
        "low_lexical_diversity_mtld",
    ];
    if CATEGORY_ONLY.contains(&category) {
        return (category.to_owned(), String::new());
    }
    let normalized = excerpt
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .take(20)
        .collect::<String>();
    (category.to_owned(), normalized)
}

pub fn apply_baseline(
    findings: &mut [Finding],
    baseline: &Value,
    file: String,
) -> Result<BaselineReport, String> {
    let Some(object) = baseline.as_object() else {
        return Err("--baseline の内容が JSON オブジェクトではありません。baseline比較を無視して通常のlintを実行します。".to_owned());
    };
    let Some(items) = object.get("findings").and_then(Value::as_array) else {
        return Err("--baseline に 'findings' 配列が見つかりません。baseline比較を無視して通常のlintを実行します。".to_owned());
    };
    let mut buckets = BTreeMap::<(String, String), Vec<Value>>::new();
    let mut skipped = 0;
    for item in items {
        let Some(category) = item.get("category").and_then(Value::as_str) else {
            skipped += 1;
            continue;
        };
        let Some(excerpt) = item.get("excerpt").and_then(Value::as_str) else {
            skipped += 1;
            continue;
        };
        buckets
            .entry(baseline_key(category, excerpt))
            .or_default()
            .push(item.clone());
    }
    if skipped > 0 {
        eprintln!(
            "警告: --baseline の findings 配列内に不正な要素が{skipped}件あったため読み飛ばしました。"
        );
    }
    for finding in findings.iter_mut() {
        let key = baseline_key(&finding.category, &finding.excerpt);
        if let Some(bucket) = buckets.get_mut(&key)
            && !bucket.is_empty()
        {
            bucket.remove(0);
            finding.status = Some("persisting".to_owned());
            continue;
        }
        finding.status = Some("new".to_owned());
    }
    let resolved = buckets.into_values().flatten().collect::<Vec<_>>();
    let summary = BaselineSummary {
        resolved: resolved.len(),
        new: findings
            .iter()
            .filter(|finding| finding.status.as_deref() == Some("new"))
            .count(),
        persisting: findings
            .iter()
            .filter(|finding| finding.status.as_deref() == Some("persisting"))
            .count(),
    };
    Ok(BaselineReport {
        file,
        summary,
        resolved,
    })
}

#[derive(Clone, Debug)]
struct TokenizedSentence {
    line: usize,
    text: String,
    raw_text: String,
    tokens: Vec<Morpheme>,
}

fn forbidden_findings(masked: &str, raw: &str) -> Vec<Finding> {
    let raw_lines = raw.split('\n').collect::<Vec<_>>();
    let mut findings = Vec::new();
    for (line_no, line) in numbered_lines(masked) {
        let raw_line = raw_lines.get(line_no - 1).copied().unwrap_or(line);
        for phrase in FORBIDDEN_PHRASES {
            if let Some(byte_start) = line.find(phrase) {
                let weak = WEAK_FORBIDDEN_PHRASES.contains(phrase);
                let mut detail = format!("禁止語/LLM常套句ヒット: 「{phrase}」");
                if weak {
                    detail.push_str("（コーパス校正で人間側にも一定数出現する弱いシグナルと判定、severity低下）");
                }
                findings.push(Finding::new(
                    line_no,
                    "forbidden_phrase",
                    excerpt_around(raw_line, byte_start, phrase.len(), 10).trim(),
                    if weak { "info" } else { "warn" },
                    detail,
                ));
            }
        }
    }
    findings
}

fn translationese_findings(masked: &str, raw: &str) -> Vec<Finding> {
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
        let raw_line = raw_lines.get(line_no - 1).copied().unwrap_or(line);
        for (pattern, regex) in &patterns {
            for found in regex.find_iter(line) {
                findings.push(Finding::new(
                    line_no,
                    "translationese",
                    excerpt_around(raw_line, found.start(), found.len(), 10).trim(),
                    "info",
                    format!("翻訳調パターン: /{pattern}/ に一致"),
                ));
            }
        }
    }
    findings
}

fn antithesis_findings(
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
    let mut hits = Vec::<(usize, String)>::new();
    for (line_no, line) in numbered_lines(masked) {
        let raw_line = raw_lines.get(line_no - 1).copied().unwrap_or(line);
        for pattern in &patterns {
            for found in pattern.find_iter(line) {
                hits.push((
                    line_no,
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
    let related = hits.iter().map(|(line, _)| *line).collect::<BTreeSet<_>>();
    let related_text = related
        .iter()
        .map(|line| format!("L{line}"))
        .collect::<Vec<_>>()
        .join(", ");
    let all_lines = related.iter().copied().collect::<Vec<_>>();
    hits.into_iter()
        .map(|(line, excerpt)| {
            let mut finding = Finding::new(
                line,
                "antithesis_repetition",
                excerpt,
                severity,
                format!(
                    "否定→肯定対比パターンが文書内で{}回検出（閾値3回以上、総文数に対する比率={:.1}%）。対応箇所: {related_text}",
                    all_lines.len(),
                    ratio * 100.0
                ),
            );
            finding.related_lines = Some(all_lines.clone());
            finding
        })
        .collect()
}

fn english_syntax_findings(masked: &str, raw: &str, split: &[Sentence]) -> Vec<Finding> {
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
                findings.push(Finding::new(
                    line_no,
                    "english_syntax_inanimate_subject",
                    raw_line
                        .get(found.start()..found.end())
                        .unwrap_or(found.as_str()),
                    "info",
                    "無生物主語+他動詞的述語（表層パターン、英語統語の直訳調の可能性、要人間判断）",
                ));
            }
        }
    }

    let cleft = Regex::new(r"^(それ|これ|この)は.{0,60}(である|だ)$").expect("valid cleft regex");
    let because = Regex::new(r"^(なぜなら|というのも)").expect("valid because regex");
    for pair in split.windows(2) {
        let head = pair[0].text.as_str();
        let reason = pair[1].text.as_str();
        if cleft.is_match(head) && because.is_match(reason) {
            findings.push(Finding::new(
                pair[0].line,
                "english_syntax_cleft_because",
                format!("{}。{}", pair[0].raw_text, pair[1].raw_text),
                "warn",
                "「それは〜である。なぜなら〜だ」型の強調構文（英語 It is ... because ... の直訳調）",
            ));
        }
    }
    findings
}

fn tokenize(split: &[Sentence], morphology: &Morphology) -> Result<Vec<TokenizedSentence>, Error> {
    split
        .iter()
        .map(|sentence| {
            Ok(TokenizedSentence {
                line: sentence.line,
                text: sentence.text.clone(),
                raw_text: sentence.raw_text.clone(),
                tokens: morphology.tokenize(&sentence.text)?,
            })
        })
        .collect()
}

fn translationese_morph_findings(tokenized: &[TokenizedSentence]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for sentence in tokenized {
        for (index, token) in sentence.tokens.iter().enumerate() {
            let Some(particle) = sentence.tokens.get(index + 1) else {
                continue;
            };
            let Some(verb) = sentence.tokens.get(index + 2) else {
                continue;
            };
            if token.surface == "こと"
                && token.pos(0) == "名詞"
                && particle.pos(0) == "助詞"
                && matches!(particle.surface.as_str(), "が" | "は")
                && verb.pos(0) == "動詞"
                && verb.surface.starts_with("でき")
            {
                let start = sentence.tokens[index.saturating_sub(4)].byte_start;
                let excerpt = sentence
                    .raw_text
                    .get(start..verb.byte_end)
                    .unwrap_or(&sentence.text[start..verb.byte_end])
                    .to_owned();
                findings.push(Finding::new(
                    sentence.line,
                    "translationese_morph",
                    excerpt,
                    "info",
                    "品詞列マッチ: 名詞/動詞+こと+が/は+できる型の翻訳調構文",
                ));
            }
        }
    }
    findings
}

fn inanimate_morph_findings(tokenized: &[TokenizedSentence]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for sentence in tokenized {
        let mut skip_until = None;
        for index in 0..sentence.tokens.len() {
            if skip_until.is_some_and(|skip| index <= skip) {
                continue;
            }
            let token = &sentence.tokens[index];
            let mut subject_end = index;
            let mut abstract_subject =
                matches!(token.surface.as_str(), "これ" | "それ" | "あれ" | "それら")
                    || (token.pos(0) == "名詞"
                        && matches!(token.surface.as_str(), "こと" | "事実"));
            if !abstract_subject && let Some(next) = sentence.tokens.get(index + 1) {
                let phrase = format!("{}{}", token.surface, next.surface);
                if matches!(phrase.as_str(), "この事実" | "そのこと") {
                    abstract_subject = true;
                    subject_end = index + 1;
                }
            }
            if !abstract_subject {
                continue;
            }
            skip_until = Some(subject_end);
            let Some(particle) = sentence.tokens.get(subject_end + 1) else {
                continue;
            };
            if particle.pos(0) != "助詞" || !matches!(particle.surface.as_str(), "が" | "は") {
                continue;
            }
            let verb = sentence.tokens[subject_end + 2..].iter().find(|candidate| {
                candidate.pos(0) == "動詞"
                    && TRANSITIVE_SMELL_VERBS.contains(&candidate.dictionary_form())
            });
            let Some(verb) = verb else {
                continue;
            };
            let byte_start = sentence.tokens[index.saturating_sub(3)].byte_start;
            let fallback = &sentence.text[byte_start..verb.byte_end];
            let excerpt = sentence
                .raw_text
                .get(byte_start..verb.byte_end)
                .unwrap_or(fallback)
                .to_owned();
            let subject = sentence.tokens[index..=subject_end]
                .iter()
                .map(|token| token.surface.as_str())
                .collect::<String>();
            findings.push(Finding::new(
                sentence.line,
                "inanimate_subject_morph",
                excerpt,
                "info",
                format!(
                    "品詞列マッチ: 抽象主語「{subject}」+ {} + 他動詞的述語「{}」（英語統語の直訳調の疑い）",
                    particle.surface,
                    verb.dictionary_form()
                ),
            ));
        }
    }
    findings
}

fn mean_and_stdev(values: &[f64]) -> Option<(f64, f64)> {
    if values.is_empty() {
        return None;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance = values
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / values.len() as f64;
    Some((mean, variance.sqrt()))
}

fn mora_length(tokens: &[Morpheme]) -> usize {
    tokens
        .iter()
        .map(|token| {
            token
                .reading()
                .chars()
                .filter(|ch| {
                    !matches!(
                        ch,
                        'ァ' | 'ィ' | 'ゥ' | 'ェ' | 'ォ' | 'ャ' | 'ュ' | 'ョ' | 'ヮ'
                    )
                })
                .count()
        })
        .sum()
}

fn rhythm_analysis(tokenized: &[TokenizedSentence]) -> (Vec<Finding>, Value) {
    if tokenized.len() < 6 {
        return (Vec::new(), json!({}));
    }
    let lengths = tokenized
        .iter()
        .map(|sentence| mora_length(&sentence.tokens) as f64)
        .collect::<Vec<_>>();
    let (mean, stdev) = mean_and_stdev(&lengths).expect("non-empty lengths");
    let burstiness = if stdev + mean == 0.0 {
        0.0
    } else {
        (stdev - mean) / (stdev + mean)
    };
    let xs = &lengths[..lengths.len() - 1];
    let ys = &lengths[1..];
    let autocorrelation = if xs.len() >= 4 {
        let (x_mean, x_stdev) = mean_and_stdev(xs).expect("non-empty x series");
        let (y_mean, y_stdev) = mean_and_stdev(ys).expect("non-empty y series");
        if x_stdev > 0.0 && y_stdev > 0.0 {
            let covariance = xs
                .iter()
                .zip(ys)
                .map(|(x, y)| (x - x_mean) * (y - y_mean))
                .sum::<f64>()
                / xs.len() as f64;
            Some(covariance / (x_stdev * y_stdev))
        } else {
            None
        }
    } else {
        None
    };
    let mut findings = Vec::new();
    if burstiness < -0.24 {
        findings.push(Finding::new(
            tokenized[0].line,
            "low_burstiness",
            format!(
                "burstiness={burstiness:.3} (モーラ近似長 平均={mean:.1}, 標準偏差={stdev:.1})"
            ),
            "warn",
            "burstiness が閾値(-0.24)未満。文の長短のメリハリが乏しく機械的なリズムの疑い",
        ));
    }
    if autocorrelation.is_some_and(|value| value > 0.6) {
        findings.push(Finding::new(
            tokenized[0].line,
            "high_length_autocorrelation",
            format!("lag-1 自己相関={:.3}", autocorrelation.unwrap_or_default()),
            "info",
            "隣接する文の長さが強く相関（閾値0.6超）。文長パターンが単調に繰り返されている疑い",
        ));
    }
    (
        findings,
        json!({
            "mora_mean": mean,
            "mora_stdev": stdev,
            "burstiness": burstiness,
            "length_autocorrelation_lag1": autocorrelation,
        }),
    )
}

fn significant_tokens(tokens: &[Morpheme]) -> &[Morpheme] {
    let start = tokens
        .iter()
        .position(|token| !matches!(token.pos(0), "記号" | "補助記号" | "空白"))
        .unwrap_or(tokens.len());
    &tokens[start..]
}

fn ngram_analysis(
    tokenized: &[TokenizedSentence],
    genre: Option<&str>,
    repeated_sentence_lead: Option<usize>,
) -> (Vec<Finding>, Value) {
    let lead_threshold = repeated_sentence_lead.unwrap_or(match genre {
        Some("essay") => 5,
        Some("tech" | "business") => 7,
        _ => 6,
    });
    let mut findings = Vec::new();
    let leads = tokenized
        .iter()
        .filter_map(|sentence| {
            let tokens = significant_tokens(&sentence.tokens);
            if tokens.len() < 2 {
                return None;
            }
            let lead = format!("{}{}", tokens[0].surface, tokens[1].surface);
            let tech_lead = (tokens[0].pos(0) == "名詞" && tokens[0].pos(1) == "固有名詞")
                || (tokens[0]
                    .surface
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_alphabetic())
                    && tokens[0]
                        .surface
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')));
            Some((sentence, lead, tech_lead))
        })
        .collect::<Vec<_>>();
    let mut lead_counts = BTreeMap::<String, usize>::new();
    for (_, lead, _) in &leads {
        *lead_counts.entry(lead.clone()).or_default() += 1;
    }
    for (sentence, lead, tech_lead) in &leads {
        let count = lead_counts[lead];
        if count < lead_threshold {
            continue;
        }
        let lines = leads
            .iter()
            .filter(|(_, candidate, _)| candidate == lead)
            .map(|(sentence, _, _)| sentence.line)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let related = lines
            .iter()
            .map(|line| format!("L{line}"))
            .collect::<Vec<_>>()
            .join(", ");
        let qualifier = if *tech_lead {
            "固有名詞/技術用語由来の可能性が高い"
        } else {
            "人間の意図的な反復技法との区別がつかないため参考情報として提示"
        };
        let mut finding = Finding::new(
            sentence.line,
            "repeated_sentence_lead",
            sentence.raw_text.chars().take(20).collect::<String>(),
            "info",
            format!(
                "文頭2形態素「{lead}」が{count}回反復（閾値{lead_threshold}回以上）。{qualifier}。対応箇所: {related}"
            ),
        );
        finding.related_lines = Some(lines);
        findings.push(finding);
    }

    let pos_ngrams = tokenized
        .iter()
        .filter_map(|sentence| {
            let tokens = significant_tokens(&sentence.tokens);
            if tokens.len() < 4 {
                return None;
            }
            Some((
                sentence,
                tokens[..4]
                    .iter()
                    .map(|token| token.pos(0).to_owned())
                    .collect::<Vec<_>>(),
            ))
        })
        .collect::<Vec<_>>();
    let mut ordered_counts = Vec::<(Vec<String>, usize)>::new();
    for (_, sequence) in &pos_ngrams {
        if let Some((_, count)) = ordered_counts
            .iter_mut()
            .find(|(candidate, _)| candidate == sequence)
        {
            *count += 1;
        } else {
            ordered_counts.push((sequence.clone(), 1));
        }
    }
    let top = ordered_counts
        .iter()
        .max_by_key(|(_, count)| *count)
        .cloned();
    let mut top_text: Option<String> = None;
    let mut top_ratio: Option<f64> = None;
    if pos_ngrams.len() >= 6
        && let Some((top_sequence, top_count)) = top
    {
        let ratio = top_count as f64 / pos_ngrams.len() as f64;
        top_text = Some(top_sequence.join("/"));
        top_ratio = Some(ratio);
        if ratio >= 0.4 {
            let lines = pos_ngrams
                .iter()
                .filter(|(_, sequence)| sequence == &top_sequence)
                .map(|(sentence, _)| sentence.line)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let related = lines
                .iter()
                .map(|line| format!("L{line}"))
                .collect::<Vec<_>>()
                .join(", ");
            for (sentence, sequence) in &pos_ngrams {
                if sequence != &top_sequence {
                    continue;
                }
                let mut finding = Finding::new(
                    sentence.line,
                    "repeated_syntax_template",
                    sentence.raw_text.chars().take(20).collect::<String>(),
                    "info",
                    format!(
                        "文頭品詞4-gram「{}」が全文の{:.1}%で一致（閾値40%以上）。構文テンプレートの使い回しの疑い。対応箇所: {related}",
                        top_sequence.join("/"),
                        ratio * 100.0
                    ),
                );
                finding.related_lines = Some(lines.clone());
                findings.push(finding);
            }
        }
    }
    (
        findings,
        json!({"lead_pos_4gram_top": top_text, "lead_pos_4gram_ratio": top_ratio}),
    )
}

fn mtld(tokens: &[String], threshold: f64) -> Option<f64> {
    if tokens.len() < 20 {
        return None;
    }
    fn direction(tokens: impl Iterator<Item = String>, length: usize, threshold: f64) -> f64 {
        let mut factor_count = 0.0;
        let mut types = BTreeSet::new();
        let mut token_count = 0;
        for token in tokens {
            types.insert(token);
            token_count += 1;
            if types.len() as f64 / token_count as f64 <= threshold {
                factor_count += 1.0;
                types.clear();
                token_count = 0;
            }
        }
        if token_count > 0 {
            let ttr = types.len() as f64 / token_count as f64;
            if ttr < 1.0 {
                factor_count += ((1.0 - ttr) / (1.0 - threshold)).min(1.0);
            }
        }
        if factor_count > 0.0 {
            length as f64 / factor_count
        } else {
            length as f64
        }
    }
    let forward = direction(tokens.iter().cloned(), tokens.len(), threshold);
    let backward = direction(tokens.iter().rev().cloned(), tokens.len(), threshold);
    Some((forward + backward) / 2.0)
}

fn lexical_diversity_analysis(
    tokenized: &[TokenizedSentence],
    ttr_threshold: f64,
    mtld_threshold: f64,
) -> (Vec<Finding>, Value) {
    let content = tokenized
        .iter()
        .flat_map(|sentence| sentence.tokens.iter())
        .filter(|token| CONTENT_POS.contains(&token.pos(0)))
        .map(|token| token.dictionary_form().to_owned())
        .collect::<Vec<_>>();
    let doc_chars = tokenized
        .iter()
        .map(|sentence| sentence.raw_text.chars().count())
        .sum::<usize>();
    if doc_chars < 4000 {
        return (
            Vec::new(),
            json!({
                "ttr": Value::Null,
                "mtld": Value::Null,
                "content_token_count": content.len(),
                "doc_char_count": doc_chars,
                "skipped_too_short": true,
            }),
        );
    }
    let unique = content.iter().collect::<BTreeSet<_>>().len();
    let ttr = if content.is_empty() {
        None
    } else {
        Some(unique as f64 / content.len() as f64)
    };
    let mtld_value = mtld(&content, 0.72);
    let mut findings = Vec::new();
    if ttr.is_some_and(|value| value < ttr_threshold) {
        findings.push(Finding::new(
            tokenized.first().map_or(1, |sentence| sentence.line),
            "low_lexical_diversity_ttr",
            format!(
                "TTR={:.3} (内容語 {} 語中 {unique} 種類)",
                ttr.unwrap_or_default(),
                content.len()
            ),
            "info",
            format!(
                "TTR(Type-Token Ratio)が閾値{ttr_threshold:.2}未満。同じ語彙の使い回しが多い疑い"
            ),
        ));
    }
    if mtld_value.is_some_and(|value| value < mtld_threshold) {
        findings.push(Finding::new(
            tokenized.first().map_or(1, |sentence| sentence.line),
            "low_lexical_diversity_mtld",
            format!("MTLD={:.1}", mtld_value.unwrap_or_default()),
            "info",
            format!("MTLD が閾値{mtld_threshold:.1}未満。文章長で正規化した語彙多様性が低い疑い"),
        ));
    }
    (
        findings,
        json!({
            "ttr": ttr,
            "mtld": mtld_value,
            "content_token_count": content.len(),
            "doc_char_count": doc_chars,
            "skipped_too_short": false,
        }),
    )
}

fn reading_length(text: &str) -> usize {
    let whitespace = Regex::new(r"\s{2,}").expect("valid whitespace regex");
    whitespace.replace_all(text, " ").trim().chars().count()
}

fn punctuation_between(tokens: &[Morpheme], first: usize, second: usize) -> bool {
    tokens[first + 1..second]
        .iter()
        .any(|token| matches!(token.pos(0), "記号" | "補助記号"))
}

fn noun_ended(tokens: &[Morpheme]) -> bool {
    tokens
        .iter()
        .rev()
        .find(|token| !matches!(token.pos(0), "記号" | "補助記号" | "空白"))
        .is_some_and(|token| token.pos(0) == "名詞")
}

fn buried_list(tokens: &[Morpheme]) -> Option<(usize, usize, usize)> {
    let mut bounds = Vec::new();
    let mut start = 0;
    for (index, token) in tokens.iter().enumerate() {
        if token.surface == "、" {
            bounds.push((start, index));
            start = index + 1;
        }
    }
    bounds.push((start, tokens.len()));
    let mut run = Vec::new();
    let mut best = None;
    for (index, (start, end)) in bounds.iter().copied().enumerate() {
        if end > start && noun_ended(&tokens[start..end]) {
            run.push((start, end));
        } else {
            run.clear();
        }
        if run.len() >= 2 && index + 1 < bounds.len() {
            let items = run.len() + 1;
            if best.is_none_or(|(_, _, best_items)| items > best_items) {
                best = Some((run[0].0, bounds[index + 1].1, items));
            }
        }
    }
    best
}

pub fn analyze_reading_load(
    raw: &str,
    morphology: &Morphology,
    genre: Option<&str>,
) -> Result<ReadingLoadReport, Error> {
    let masked = mask_markdown_structure(raw);
    let split = sentences_with_raw(&masked, raw);
    let tokenized = tokenize(&split, morphology)?;
    let sentence_max = if genre == Some("essay") { 110 } else { 90 };
    let kanji = Regex::new(r"[一-龿々]{7,}").expect("valid kanji-run regex");
    let conditional_negative =
        Regex::new(r"^(ない|なけれ|なく)(と|ば|ければ)").expect("valid conditional-negation regex");
    let mut findings = Vec::new();

    for sentence in &tokenized {
        let length = reading_length(&sentence.text);
        let excerpt = sentence.raw_text.chars().take(40).collect::<String>();
        if length > sentence_max {
            findings.push(Finding::new(
                sentence.line,
                "sentence_too_long",
                excerpt,
                "info",
                format!(
                    "一文が{length}字（目安{sentence_max}字）。カタログ B1。一文一義になっているか確認する（分割の結果、字数が増えるのは正しい）"
                ),
            ));
        }

        for found in kanji.find_iter(&sentence.text) {
            let includes_proper_noun = sentence.tokens.iter().any(|token| {
                token.byte_end > found.start()
                    && token.byte_start < found.end()
                    && token.pos(1) == "固有名詞"
            });
            if includes_proper_noun {
                continue;
            }
            findings.push(Finding::new(
                sentence.line,
                "kanji_run",
                found.as_str(),
                "info",
                format!(
                    "漢字が{}字連続（目安6字）。カタログ C1。語の切れ目が読み取れるか確認する",
                    found.as_str().chars().count()
                ),
            ));
        }

        if let Some((start, end, items)) = buried_list(&sentence.tokens) {
            let min_chars = if items <= 3 { 80 } else { 50 };
            if length >= min_chars {
                let phrase = sentence.tokens[start..end]
                    .iter()
                    .map(|token| token.surface.as_str())
                    .collect::<String>();
                findings.push(Finding::new(
                    sentence.line,
                    "buried_list",
                    phrase.chars().take(40).collect::<String>(),
                    "info",
                    format!(
                        "同格の名詞句が読点で{items}個並んでいる（一文{length}字）。カタログ F1。箇条書きに開くと並列関係を読み手が再構成せずに済む"
                    ),
                ));
            }
        }

        let negative_indices = sentence
            .tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| {
                matches!(token.pos(0), "助動詞" | "形容詞")
                    && matches!(
                        token.dictionary_form(),
                        "ない" | "無い" | "ぬ" | "ず" | "ん"
                    )
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        for pair in negative_indices.windows(2) {
            let first = pair[0];
            let second = pair[1];
            let phrase = sentence.tokens[first..=second]
                .iter()
                .map(|token| token.surface.as_str())
                .collect::<String>();
            let obligation = [
                "といけ",
                "とだめ",
                "とダメ",
                "ばならな",
                "ばなりま",
                "ばいけな",
                "てはならな",
                "てはなりま",
                "てはいけな",
                "ざるを得",
                "ざるをえ",
            ]
            .iter()
            .any(|pattern| phrase.contains(pattern));
            if second - first <= 6
                && !punctuation_between(&sentence.tokens, first, second)
                && !obligation
                && !conditional_negative.is_match(&phrase)
            {
                findings.push(Finding::new(
                    sentence.line,
                    "double_negative",
                    phrase,
                    "info",
                    "否定が二重に掛かっている可能性。カタログ A1/A2。肯定に畳むなら真偽が反転していないか必ず確認する。控えめな肯定が本質的な箇所は触らない",
                ));
                break;
            }
        }

        let no_indices = sentence
            .tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| token.surface == "の" && token.pos(0) == "助詞")
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        for window in no_indices.windows(3) {
            if window.windows(2).all(|pair| pair[1] - pair[0] <= 3)
                && !punctuation_between(&sentence.tokens, window[0], window[2])
            {
                let phrase = sentence.tokens[window[0]..=window[2]]
                    .iter()
                    .map(|token| token.surface.as_str())
                    .collect::<String>();
                findings.push(Finding::new(
                    sentence.line,
                    "no_chain",
                    phrase,
                    "info",
                    "格助詞「の」が3連以上。カタログ C2。どこかを動詞・連用に開く",
                ));
                break;
            }
        }
    }
    findings.sort_by_key(|finding| finding.line);
    let mut by_category = BTreeMap::new();
    for finding in &findings {
        *by_category.entry(finding.category.clone()).or_default() += 1;
    }
    Ok(ReadingLoadReport {
        stats: ReadingLoadStats {
            total: findings.len(),
            sentences: tokenized.len(),
            genre: genre.map(str::to_owned),
            by_category,
        },
        findings,
    })
}

fn paragraphs(text: &str) -> Vec<Vec<(usize, &str)>> {
    let mut output = Vec::new();
    let mut current = Vec::new();
    for (line_no, line) in numbered_lines(text) {
        if line.trim().is_empty() {
            if !current.is_empty() {
                output.push(std::mem::take(&mut current));
            }
        } else {
            current.push((line_no, line));
        }
    }
    if !current.is_empty() {
        output.push(current);
    }
    output
}

fn low_specificity_analysis(
    masked: &str,
    raw: &str,
    morphology: &Morphology,
) -> Result<(Vec<Finding>, Value), Error> {
    let raw_lines = raw.split('\n').collect::<Vec<_>>();
    let numeric = Regex::new(
        r"[0-9０-９]+(年代|年間|世紀|年|月|日|時間|時|分|秒|人|円|%|％|kg|km|cm|mm|g|m|回|件|個|つ|割|倍|台|社|名|冊|本|杯|軒)?",
    )
    .expect("valid numeric quantity regex");
    let mut findings = Vec::new();
    let mut evaluated = 0;
    let mut fired = 0;
    for paragraph in paragraphs(masked) {
        let first_line = paragraph[0].0;
        let text = paragraph
            .iter()
            .map(|(_, line)| *line)
            .collect::<Vec<_>>()
            .join("\n");
        if text.chars().count() < 80 {
            continue;
        }
        let mut tokens = Vec::new();
        for (_, line) in &paragraph {
            tokens.extend(morphology.tokenize(line)?);
        }
        let content = tokens
            .iter()
            .filter(|token| CONTENT_POS.contains(&token.pos(0)))
            .collect::<Vec<_>>();
        if content.len() < 15 {
            continue;
        }
        evaluated += 1;
        let proper = content
            .iter()
            .filter(|token| token.pos(0) == "名詞" && token.pos(1) == "固有名詞")
            .count();
        let abstract_count = content
            .iter()
            .filter(|token| {
                token.pos(0) == "名詞" && ABSTRACT_NOUNS.contains(&token.dictionary_form())
            })
            .count();
        let numeric_count = numeric.find_iter(&text).count();
        let has_example = EXAMPLE_MARKERS.iter().any(|marker| text.contains(marker));
        let count = content.len() as f64;
        let proper_density = proper as f64 / count;
        let numeric_density = numeric_count as f64 / count;
        let abstract_ratio = abstract_count as f64 / count;
        let score = proper_density + numeric_density + if has_example { 0.1 } else { 0.0 }
            - abstract_ratio * 1.5;
        if score < -0.15 {
            fired += 1;
            let excerpt = raw_lines
                .get(first_line - 1)
                .copied()
                .unwrap_or(paragraph[0].1)
                .trim()
                .chars()
                .take(40)
                .collect::<String>();
            findings.push(Finding::new(
                first_line,
                "low_specificity",
                excerpt,
                "info",
                format!(
                    "段落の具体性スコア={score:.3}（閾値-0.15未満）。固有名詞密度={proper_density:.3}, 数値密度={numeric_density:.3}, 抽象名詞率={abstract_ratio:.3}, 例示マーカー={}。固有名詞・数値・実例が乏しく一般論に留まっている疑い。素材不足のサインであり、文体の修正でなく情報収集を検討する（revision-guide.md の素材不足の分岐を参照）",
                    if has_example { "あり" } else { "なし" }
                ),
            ));
        }
    }
    Ok((
        findings,
        json!({"paragraphs_evaluated": evaluated, "paragraphs_fired": fired}),
    ))
}

fn structural_analysis(raw: &str) -> (Vec<Finding>, Value) {
    let bold = Regex::new(r"\*\*[^*\n]+\*\*").expect("valid bold regex");
    let non_blank = raw.lines().filter(|line| !line.trim().is_empty()).count();
    let bullet_count = raw
        .lines()
        .filter(|line| crate::text::is_list_item(line))
        .count();
    let boilerplate = raw
        .lines()
        .filter_map(crate::text::heading)
        .filter(|(_, text)| {
            matches!(
                text.trim().to_lowercase().as_str(),
                "まとめ"
                    | "おわりに"
                    | "終わりに"
                    | "さいごに"
                    | "最後に"
                    | "結論"
                    | "総括"
                    | "conclusion"
            )
        })
        .count();
    let phase = Regex::new(r"(フェーズ|ステップ|段階|ステージ)\s*[0-9０-９]")
        .expect("valid numbered-phase regex");
    let chars = raw.chars().count().max(1) as f64;
    let emoji_count = raw
        .chars()
        .filter(|ch| {
            matches!(*ch as u32, 0x1F300..=0x1FAFF | 0x2600..=0x27BF)
                || matches!(ch, '⭐' | '✅' | '❌' | '❗' | '❓')
        })
        .count();
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
    for (line_no, line) in numbered_lines(raw) {
        if let Some((_, text)) = crate::text::heading(line) {
            let lower = text.to_lowercase();
            if matches!(
                lower.as_str(),
                "まとめ"
                    | "おわりに"
                    | "終わりに"
                    | "さいごに"
                    | "最後に"
                    | "結論"
                    | "総括"
                    | "conclusion"
            ) {
                findings.push(Finding::new(
                    line_no,
                    "boilerplate_heading",
                    line.trim().chars().take(40).collect::<String>(),
                    "info",
                    format!("定型見出し「{text}」系での締め。予告・構成の型のみで中身を語らない教科書的なAI生成文に見られる傾向（実験的検出器）"),
                ));
            }
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

struct ParagraphAnalysis {
    findings: Vec<Finding>,
    total: usize,
    conjunction_count: usize,
    conjunction_ratio: f64,
    sentence_counts: Vec<usize>,
    sentence_count_cv: Option<f64>,
}

fn analyze_paragraphs(masked: &str, raw: &str) -> ParagraphAnalysis {
    let raw_lines = raw.split('\n').collect::<Vec<_>>();
    let groups = paragraphs(masked);
    let total = groups.len();
    let sentence_counts = groups
        .iter()
        .map(|group| {
            let text = group
                .iter()
                .map(|(_, line)| *line)
                .collect::<Vec<_>>()
                .join("\n");
            sentences(&text).len()
        })
        .collect::<Vec<_>>();
    let conjunctions = groups
        .iter()
        .filter_map(|group| {
            let (line_no, line) = group[0];
            let trimmed = line.trim();
            PARAGRAPH_CONJUNCTIONS
                .iter()
                .find(|conjunction| trimmed.starts_with(**conjunction))
                .map(|conjunction| (line_no, *conjunction))
        })
        .collect::<Vec<_>>();
    let conjunction_ratio = if total == 0 {
        0.0
    } else {
        conjunctions.len() as f64 / total as f64
    };
    let mut findings = Vec::new();
    if total >= 3 && conjunction_ratio >= 0.3 {
        let lines = conjunctions
            .iter()
            .map(|(line, _)| *line)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let related = lines
            .iter()
            .map(|line| format!("L{line}"))
            .collect::<Vec<_>>()
            .join(", ");
        for (line, conjunction) in &conjunctions {
            let excerpt = raw_lines
                .get(line - 1)
                .copied()
                .unwrap_or_default()
                .chars()
                .take(40)
                .collect::<String>();
            let mut finding = Finding::new(
                *line,
                "paragraph_lead_conjunction",
                excerpt,
                "info",
                format!(
                    "段落頭が接続詞「{conjunction}」で始まる（文書全体の段落頭接続詞率={:.1}%、閾値30%以上で警告）。対応箇所: {related}",
                    conjunction_ratio * 100.0
                ),
            );
            finding.related_lines = Some(lines.clone());
            findings.push(finding);
        }
    }
    let sentence_count_cv = if sentence_counts.len() >= 4 {
        let values = sentence_counts
            .iter()
            .map(|count| *count as f64)
            .collect::<Vec<_>>();
        mean_and_stdev(&values).map(|(mean, stdev)| if mean == 0.0 { 0.0 } else { stdev / mean })
    } else {
        None
    };
    if sentence_count_cv.is_some_and(|cv| cv < 0.15) {
        findings.push(Finding::new(
            1,
            "uniform_paragraph_structure",
            format!("段落数={}, 各段落の文数={sentence_counts:?}", sentence_counts.len()),
            "info",
            format!(
                "段落あたり文数の変動係数={:.3}（閾値0.15未満）。どの段落もほぼ同じ文数=定型段落の疑い",
                sentence_count_cv.unwrap_or_default()
            ),
        ));
    }
    ParagraphAnalysis {
        findings,
        total,
        conjunction_count: conjunctions.len(),
        conjunction_ratio,
        sentence_counts,
        sentence_count_cv,
    }
}

pub fn analyze(
    raw: &str,
    morphology: &Morphology,
    genre: Option<&str>,
    experimental: bool,
) -> Result<LintReport, Error> {
    analyze_with_thresholds(
        raw,
        morphology,
        genre,
        experimental,
        AnalysisThresholds::default(),
    )
}

pub fn analyze_with_thresholds(
    raw: &str,
    morphology: &Morphology,
    genre: Option<&str>,
    experimental: bool,
    thresholds: AnalysisThresholds,
) -> Result<LintReport, Error> {
    let masked = mask_markdown_structure(raw);
    let split = sentences_with_raw(&masked, raw);
    let tokenized = tokenize(&split, morphology)?;
    let critical_above = if genre == Some("tech") { 0.045 } else { 0.03 };

    let (structural_findings, structural_stats) = structural_analysis(&mask_html_comments(raw));
    let (rhythm_findings, rhythm_stats) = rhythm_analysis(&tokenized);
    let (ngram_findings, ngram_stats) =
        ngram_analysis(&tokenized, genre, thresholds.repeated_sentence_lead);
    let (lexical_findings, lexical_stats) =
        lexical_diversity_analysis(&tokenized, thresholds.lexical_ttr, thresholds.lexical_mtld);
    let (specificity_findings, specificity_stats) =
        low_specificity_analysis(&masked, raw, morphology)?;
    let paragraph_analysis = analyze_paragraphs(&masked, raw);

    let mut findings = structural_findings;
    findings.extend(forbidden_findings(&masked, raw));
    findings.extend(translationese_findings(&masked, raw));
    findings.extend(antithesis_findings(
        &masked,
        raw,
        split.len(),
        critical_above,
    ));
    findings.extend(english_syntax_findings(&masked, raw, &split));
    findings.extend(translationese_morph_findings(&tokenized));
    findings.extend(inanimate_morph_findings(&tokenized));
    findings.extend(rhythm_findings);
    findings.extend(ngram_findings);
    findings.extend(lexical_findings);
    findings.extend(specificity_findings);
    findings.extend(paragraph_analysis.findings);

    let sentence_lengths = split
        .iter()
        .map(|sentence| sentence.raw_text.chars().count() as f64)
        .collect::<Vec<_>>();
    if sentence_lengths.len() >= 5
        && let Some((mean, stdev)) = mean_and_stdev(&sentence_lengths)
        && mean > 0.0
        && stdev / mean < 0.25
    {
        let cv = stdev / mean;
        findings.push(Finding::new(
            split.first().map_or(1, |sentence| sentence.line),
            "low_sentence_variance",
            format!(
                "文数={}, 平均文長={mean:.1}字, 変動係数={cv:.3}",
                sentence_lengths.len()
            ),
            "warn",
            "文長の変動係数が閾値(0.25)未満。リズムが均質でAI臭い可能性",
        ));
    }
    if !experimental {
        findings.retain(|finding| !EXPERIMENTAL_CATEGORIES.contains(&finding.category.as_str()));
    }
    if genre == Some("business") {
        findings.retain(|finding| {
            !matches!(
                finding.category.as_str(),
                "high_bullet_ratio"
                    | "high_bold_density"
                    | "boilerplate_heading"
                    | "numbered_phase_structure"
            )
        });
    }
    let total_chars = split
        .iter()
        .map(|sentence| sentence.text.chars().count())
        .sum::<usize>();
    let nominal_count = tokenized
        .iter()
        .filter(|sentence| {
            sentence
                .tokens
                .iter()
                .rev()
                .find(|token| !matches!(token.pos(0), "記号" | "補助記号" | "空白"))
                .is_some_and(|token| token.pos(0) == "名詞")
        })
        .count();
    let nominal_ratio = if split.is_empty() {
        0.0
    } else {
        nominal_count as f64 / split.len() as f64
    };
    let nominal_min_chars = match genre {
        Some("essay") => 1500,
        Some("tech" | "business") => 3000,
        _ => 2000,
    };
    if split.len() >= 5 && total_chars >= nominal_min_chars && nominal_ratio <= 0.0 {
        findings.push(Finding::new(
            split.last().map_or(1, |sentence| sentence.line),
            "nominal_ending",
            format!("体言止め0件（全{}文、約{total_chars}字）", split.len()),
            "info",
            "この文書には体言止めが1つもない。ある程度の長さの文書でこの修辞技法が皆無なのはAI文章に特徴的。人間的な修辞の欠如の疑い",
        ));
    }

    findings.sort_by_key(|finding| finding.line);
    let mut by_category = BTreeMap::new();
    for finding in &findings {
        *by_category.entry(finding.category.clone()).or_default() += 1;
    }

    let mut measurements = BTreeMap::new();
    measurements.insert("total_sentences".to_owned(), json!(split.len()));
    measurements.insert("nominal_ending_count".to_owned(), json!(nominal_count));
    measurements.insert("nominal_ending_ratio".to_owned(), json!(nominal_ratio));
    measurements.insert(
        "total_paragraphs".to_owned(),
        json!(paragraph_analysis.total),
    );
    measurements.insert(
        "paragraph_lead_conjunction_count".to_owned(),
        json!(paragraph_analysis.conjunction_count),
    );
    measurements.insert(
        "paragraph_lead_conjunction_ratio".to_owned(),
        json!(paragraph_analysis.conjunction_ratio),
    );
    measurements.insert(
        "paragraph_sentence_counts".to_owned(),
        json!(paragraph_analysis.sentence_counts),
    );
    measurements.insert(
        "paragraph_sentence_count_cv".to_owned(),
        json!(paragraph_analysis.sentence_count_cv),
    );
    measurements.insert("rhythm".to_owned(), rhythm_stats);
    measurements.insert("ngram".to_owned(), ngram_stats);
    measurements.insert("lexical_diversity".to_owned(), lexical_stats);
    measurements.insert("structural".to_owned(), structural_stats);
    measurements.insert("low_specificity".to_owned(), specificity_stats);

    Ok(LintReport {
        stats: LintStats {
            total_findings: findings.len(),
            by_category,
            genre: genre.map(str::to_owned),
            experimental,
            measurements,
        },
        findings,
    })
}
