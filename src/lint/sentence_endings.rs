//! 型の族: 文末の働き（断定・推量・問い・体言止めなど）の観測と、同じ文末が続く箇所
//! （`repeated_sentence_ending`・`repeated_sentence_mode`）。

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use serde_json::{Value, json};

use crate::morphology::Morpheme;

use super::Finding;
use super::metrics::mean_and_stdev;
use super::morph::{TokenizedSentence, mora_length};

const SENTENCE_MODE_RUN_MIN: usize = 3;
const LONG_SENTENCE_MORA_MIN: usize = 30;
const SENTENCE_MODE_RUN_MAX_CV: f64 = 0.15;
const SHORT_NOMINAL_MORA_MAX: usize = 25;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum SentenceMode {
    Assertive,
    Tentative,
    Question,
    Nominal,
    Other,
}

impl SentenceMode {
    const ALL: [Self; 5] = [
        Self::Assertive,
        Self::Tentative,
        Self::Question,
        Self::Nominal,
        Self::Other,
    ];

    fn names(self) -> (&'static str, &'static str) {
        match self {
            Self::Assertive => ("assertive", "断定"),
            Self::Tentative => ("tentative", "推量・保留"),
            Self::Question => ("question", "疑問"),
            Self::Nominal => ("nominal", "体言止め"),
            Self::Other => ("other", "その他"),
        }
    }

    fn as_str(self) -> &'static str {
        let (id, _) = self.names();
        id
    }

    fn label(self) -> &'static str {
        let (_, label) = self.names();
        label
    }
}

#[derive(Debug)]
struct SentenceEndingObservation<'a> {
    sentence: &'a TokenizedSentence,
    mode: SentenceMode,
    signature: &'static str,
    mora: usize,
}

fn sentence_ending(sentence: &TokenizedSentence) -> (SentenceMode, &'static str) {
    if matches!(sentence.end_mark, Some('？' | '?')) {
        return (SentenceMode::Question, "question_mark");
    }

    let tokens = sentence
        .tokens
        .iter()
        .filter(|token| !matches!(token.pos(0), "記号" | "補助記号" | "空白"))
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        return (SentenceMode::Other, "other");
    }
    if tokens
        .last()
        .is_some_and(|token| token.pos(0) == "助詞" && token.surface == "か")
    {
        return (SentenceMode::Question, "particle_ka");
    }

    let ending = tokens
        .iter()
        .rev()
        .take(8)
        .rev()
        .map(|token| token.surface.as_str())
        .collect::<String>();
    if let Some(suffix) = [
        "かもしれない",
        "かもしれません",
        "かもしれなかった",
        "に違いない",
        "に違いありません",
        "だろう",
        "でしょう",
        "ようだ",
        "ようです",
        "らしい",
        "らしいです",
        "と思う",
        "と思います",
    ]
    .iter()
    .find(|suffix| ending.ends_with(**suffix))
    {
        return (SentenceMode::Tentative, *suffix);
    }
    if super::morph::noun_ended(&sentence.tokens) {
        return (SentenceMode::Nominal, "nominal");
    }
    if let Some(suffix) = [
        "である",
        "であった",
        "だった",
        "なのだ",
        "のだ",
        "というわけだ",
        "です",
        "でした",
        "だ",
    ]
    .iter()
    .find(|suffix| ending.ends_with(**suffix))
    {
        return (SentenceMode::Assertive, *suffix);
    }
    (SentenceMode::Other, "other")
}

fn same_paragraph(previous: &TokenizedSentence, current: &TokenizedSentence) -> bool {
    current.line <= previous.line + 1
}

fn sentence_ending_observations(
    tokenized: &[TokenizedSentence],
) -> Vec<SentenceEndingObservation<'_>> {
    tokenized
        .iter()
        .map(|sentence| {
            let (mode, signature) = sentence_ending(sentence);
            SentenceEndingObservation {
                sentence,
                mode,
                signature,
                mora: mora_length(&sentence.tokens),
            }
        })
        .collect()
}

fn sentence_ending_stats(observations: &[SentenceEndingObservation<'_>]) -> Value {
    let mut counts = BTreeMap::new();
    let mut longest_runs = BTreeMap::new();
    for mode in SentenceMode::ALL {
        counts.insert(mode.as_str(), 0_usize);
        longest_runs.insert(mode.as_str(), 0_usize);
    }

    let mut previous: Option<&SentenceEndingObservation<'_>> = None;
    let mut current_run = 0;
    for observation in observations {
        *counts
            .get_mut(observation.mode.as_str())
            .expect("all sentence modes initialized") += 1;
        if previous.is_some_and(|previous| {
            previous.mode == observation.mode
                && same_paragraph(previous.sentence, observation.sentence)
        }) {
            current_run += 1;
        } else {
            current_run = 1;
        }
        let longest = longest_runs
            .get_mut(observation.mode.as_str())
            .expect("all sentence modes initialized");
        *longest = (*longest).max(current_run);
        previous = Some(observation);
    }

    json!({"counts": counts, "longest_runs": longest_runs})
}

fn matching_runs<'a>(
    observations: &'a [SentenceEndingObservation<'a>],
    eligible: impl Fn(&SentenceEndingObservation<'_>) -> bool,
) -> Vec<&'a [SentenceEndingObservation<'a>]> {
    let mut runs = Vec::new();
    let mut start = 0;
    while start < observations.len() {
        if !eligible(&observations[start]) {
            start += 1;
            continue;
        }
        let mode = observations[start].mode;
        let mut end = start + 1;
        while end < observations.len()
            && eligible(&observations[end])
            && observations[end].mode == mode
            && observations[end].signature == observations[start].signature
            && same_paragraph(observations[end - 1].sentence, observations[end].sentence)
        {
            end += 1;
        }
        if end - start >= SENTENCE_MODE_RUN_MIN {
            runs.push(&observations[start..end]);
        }
        start = end;
    }
    runs
}

fn run_lines(run: &[SentenceEndingObservation<'_>]) -> Vec<usize> {
    run.iter()
        .map(|observation| observation.sentence.line)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn run_mora_cv(run: &[SentenceEndingObservation<'_>]) -> f64 {
    let values = run
        .iter()
        .map(|observation| observation.mora as f64)
        .collect::<Vec<_>>();
    mean_and_stdev(&values).map_or(
        0.0,
        |(mean, stdev)| {
            if mean == 0.0 { 0.0 } else { stdev / mean }
        },
    )
}

/// 助動詞の活用型。読んだの「だ」は「助動詞-タ」で、断定の「だ」（助動詞-ダ）と分かれる。
fn auxiliary_kind(token: &Morpheme) -> &str {
    if token.pos(0) == "助動詞" {
        token.pos(4)
    } else {
        ""
    }
}

/// `repeated_sentence_ending`が数える文末の形と、文中のbyte範囲。
/// 過去の「た」（読んだ・した）、「ません」、「のだ」、台詞の中の文末は数えない。
fn counted_ending(sentence: &TokenizedSentence) -> Option<(&'static str, Range<usize>)> {
    let text = sentence.text.trim_end();
    if text.ends_with(['」', '』', ')', '）', '】'])
        || text.matches(['「', '『']).count() > text.matches(['」', '』']).count()
    {
        return None;
    }
    let tokens = sentence
        .tokens
        .iter()
        .filter(|token| !matches!(token.pos(0), "記号" | "補助記号" | "空白"))
        .collect::<Vec<_>>();
    let (&last, rest) = tokens.split_last()?;
    let previous = rest.last().copied();
    if let Some(first) = previous {
        let form = match (
            auxiliary_kind(first),
            first.surface.as_str(),
            last.surface.as_str(),
        ) {
            ("助動詞-デス", "です", "ね") if last.pos(1) == "終助詞" => Some("ですね"),
            ("助動詞-デス", "でし", "た") => Some("でした"),
            ("助動詞-マス", "まし", "た") => Some("ました"),
            ("助動詞-デス", "でしょ", "う") => Some("でしょう"),
            ("助動詞-ダ", "だろ", "う") => Some("だろう"),
            ("助動詞-ダ", "で", "ある") if last.pos(0) == "動詞" => Some("である"),
            _ => None,
        };
        if let Some(form) = form {
            return Some((form, first.byte_start..last.byte_end));
        }
    }
    let explanatory = previous.is_some_and(|token| matches!(token.surface.as_str(), "の" | "ん"));
    let form = match (auxiliary_kind(last), last.surface.as_str()) {
        ("助動詞-デス", "です") => "です",
        ("助動詞-デス", "でしょう") => "でしょう",
        ("助動詞-マス", "ます") => "ます",
        ("助動詞-ダ", "だろう") => "だろう",
        // 「のだ」「んだ」は説明の形で、断定の「だ」とは別に数える。
        ("助動詞-ダ", "だ") if !explanatory => "だ",
        _ => return None,
    };
    Some((form, last.byte_start..last.byte_end))
}

/// 文末の形を数えた文。
struct CountedEnding<'a> {
    sentence: &'a TokenizedSentence,
    form: &'static str,
    /// 文末の形の、文中のbyte範囲。
    range: Range<usize>,
}

/// 同じ段落で同じ文末の形が続く文の並び。3文に満たない並びも返す。
fn ending_runs<'a>(observations: &[SentenceEndingObservation<'a>]) -> Vec<Vec<CountedEnding<'a>>> {
    let mut runs: Vec<Vec<CountedEnding<'a>>> = Vec::new();
    // 直前の並びに、次の文をつなげられるか。段落の境と、数えない文末で切れる。
    let mut open = false;
    for (index, observation) in observations.iter().enumerate() {
        let sentence = observation.sentence;
        if index > 0 && !same_paragraph(observations[index - 1].sentence, sentence) {
            open = false;
        }
        // 4字未満の文は数えず、連続も切らない。
        if sentence.text.trim().chars().count() < 4 {
            continue;
        }
        let Some((form, range)) = counted_ending(sentence) else {
            open = false;
            continue;
        };
        let counted = CountedEnding {
            sentence,
            form,
            range,
        };
        match runs.last_mut() {
            Some(run) if open && run.last().is_some_and(|last| last.form == form) => {
                run.push(counted);
            }
            _ => runs.push(vec![counted]),
        }
        open = true;
    }
    runs
}

/// 3文以上続いた並びを、3文目の文末を範囲として1件にまとめる。
fn ending_run_finding(run: &[CountedEnding<'_>], raw_lines: &[&str]) -> Finding {
    let third = &run[SENTENCE_MODE_RUN_MIN - 1];
    let lines = run
        .iter()
        .map(|counted| counted.sentence.line)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut finding = Finding::new(
        third.sentence.line,
        "repeated_sentence_ending",
        third.sentence.raw_text.chars().take(40).collect::<String>(),
        "info",
        format!(
            "同じ段落で文末「{}」が{}文続いています。文をつなぐか、主語や説明の順を変えると、それぞれの文の働きが見えます。手順・定義・決定事項のように同じ働きの文を並べている箇所は残してください。語尾を散らすためだけに言い換えないでください。",
            third.form,
            run.len()
        ),
    );
    finding.span = third
        .sentence
        .span(raw_lines, third.range.start, third.range.end);
    finding.related_lines = Some(lines);
    finding
}

/// 同じ段落で同じ文末の形が3文以上続く箇所を示す。
/// 包んだ文の位置（行、行内の開始byte）も返し、`repeated_sentence_mode`の重複を避ける。
fn repeated_ending_findings(
    observations: &[SentenceEndingObservation<'_>],
    raw_lines: &[&str],
) -> (Vec<Finding>, BTreeSet<(usize, usize)>) {
    let mut findings = Vec::new();
    let mut covered = BTreeSet::new();
    for run in ending_runs(observations) {
        if run.len() < SENTENCE_MODE_RUN_MIN {
            continue;
        }
        findings.push(ending_run_finding(&run, raw_lines));
        covered.extend(
            run.iter()
                .map(|counted| (counted.sentence.line, counted.sentence.line_byte_start)),
        );
    }
    (findings, covered)
}

fn sentence_ending_findings(
    observations: &[SentenceEndingObservation<'_>],
    raw_lines: &[&str],
) -> Vec<Finding> {
    let (mut findings, covered) = repeated_ending_findings(observations, raw_lines);
    // 三文だけの例文集や境界fixtureを文書のリズムとして扱わない。
    if observations.len() < 6 {
        return findings;
    }
    for run in matching_runs(observations, |observation| {
        matches!(
            observation.mode,
            SentenceMode::Assertive | SentenceMode::Tentative | SentenceMode::Question
        ) && observation.mora >= LONG_SENTENCE_MORA_MIN
    }) {
        // 文末の形の連続（repeated_sentence_ending）が包む連続は、そちらだけを示す。
        if run.iter().all(|observation| {
            covered.contains(&(
                observation.sentence.line,
                observation.sentence.line_byte_start,
            ))
        }) {
            continue;
        }
        let mora_cv = run_mora_cv(run);
        if mora_cv > SENTENCE_MODE_RUN_MAX_CV {
            continue;
        }
        let mode = run[0].mode;
        let signature = &run[0].signature;
        let mut finding = Finding::new(
            run[0].sentence.line,
            "repeated_sentence_mode",
            run[0]
                .sentence
                .raw_text
                .chars()
                .take(40)
                .collect::<String>(),
            "info",
            format!(
                "{}モーラ以上の{}型「{}」が{}文連続し、文長の変動係数={:.3}（上限{:.2}）。同じ文末表現と長さが続き、局所的なリズムが平坦な疑い",
                LONG_SENTENCE_MORA_MIN,
                mode.label(),
                signature,
                run.len(),
                mora_cv,
                SENTENCE_MODE_RUN_MAX_CV
            ),
        );
        finding.related_lines = Some(run_lines(run));
        findings.push(finding);
    }
    for run in matching_runs(observations, |observation| {
        observation.mode == SentenceMode::Nominal && observation.mora <= SHORT_NOMINAL_MORA_MAX
    }) {
        let mut finding = Finding::new(
            run[0].sentence.line,
            "consecutive_nominal_endings",
            run[0]
                .sentence
                .raw_text
                .chars()
                .take(40)
                .collect::<String>(),
            "info",
            format!(
                "{}モーラ以下の体言止めが{}文連続（閾値{}文以上）。短い停止の反復が文章の流れを細切れにしている疑い",
                SHORT_NOMINAL_MORA_MAX,
                run.len(),
                SENTENCE_MODE_RUN_MIN
            ),
        );
        finding.related_lines = Some(run_lines(run));
        findings.push(finding);
    }
    findings
}

/// 文末の観測値（統計）と、同じ文末が続く箇所の指摘。
pub(super) fn analysis(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
) -> (Vec<Finding>, Value) {
    let observations = sentence_ending_observations(tokenized);
    let stats = sentence_ending_stats(&observations);
    (sentence_ending_findings(&observations, raw_lines), stats)
}
