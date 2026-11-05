//! 中身を言わずに、語・評価・型だけを置いた箇所を示す。
//! どれも info で、置き換え先の語は出さない。書き手が中身を持っているかを確かめる合図にとどめる。

use std::sync::LazyLock;

use regex::Regex;

use super::Finding;
use super::morph::TokenizedSentence;
use crate::text;

fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("valid hollow-wording pattern")
}

/// 意味の範囲が広く、何を指すかを本文で決めないと読み手が中身を補えないカタカナ語。
const VAGUE_LOANWORDS: &[&str] = &[
    "マイルストーン",
    "シナジー",
    "ソリューション",
    "エンゲージメント",
    "プラットフォーム",
    "コミットメント",
    "アライメント",
    "ナレッジ",
    "イニシアチブ",
    "バリュー",
    "スキーム",
    "インサイト",
    "エコシステム",
    "ケイパビリティ",
    "レバレッジ",
    "オポチュニティ",
    "ステークホルダー",
    "コンセンサス",
    "ロードマップ",
    "ベストプラクティス",
    "イノベーション",
    "トランスフォーメーション",
    "エクスペリエンス",
    "カスタマージャーニー",
    "タッチポイント",
    "モメンタム",
    "コアコンピタンス",
    "プレゼンス",
    "ウェルビーイング",
    "パーパス",
];

/// 名詞がもともと持つ評価を、述語でもう一度言うだけの組。
const THIN_EVALUATIONS: &[(&str, &str)] = &[
    ("座り心地", "快適|よい|良い|いい|抜群"),
    ("使い心地", "よい|良い|いい|快適|抜群"),
    ("書き心地", "なめらか|滑らか|よい|良い|いい|抜群"),
    ("寝心地", "よい|良い|いい|快適|抜群"),
    ("履き心地", "よい|良い|いい|快適|抜群"),
    ("着心地", "よい|良い|いい|快適|抜群"),
    ("乗り心地", "快適|よい|良い|いい|抜群"),
    ("居心地", "よい|良い|いい|快適|抜群"),
    (
        "肌触り",
        "心地よい|心地良い|よい|良い|いい|なめらか|滑らか|抜群",
    ),
    (
        "手触り",
        "心地よい|心地良い|よい|良い|いい|なめらか|滑らか|抜群",
    ),
    ("質感", "上質|よい|良い|いい|高い|抜群"),
    ("使い勝手", "よい|良い|いい|抜群"),
    ("手軽さ", "抜群|魅力"),
    ("打鍵感", "心地よい|心地良い|よい|良い|いい|快適|抜群"),
    ("装着感", "快適|よい|良い|いい|抜群"),
    ("仕上がり", "美しい|きれい|綺麗|上質|抜群"),
    ("味わい", "深い|豊か|格別"),
    ("香り", "豊か|よい|良い|いい|格別"),
    ("操作性", "高い|優れてい(?:ます|る)|よい|良い|抜群"),
    ("視認性", "高い|優れてい(?:ます|る)|よい|良い|抜群"),
    (
        "利便性|快適性|安定性|耐久性|携帯性|静音性|保温性|信頼性|拡張性|保守性|可読性|機能性|デザイン性|品質|性能",
        "高い|優れてい(?:ます|る)|抜群",
    ),
];

/// 文全体が「（読点のない20字以下の前置き）名詞（が/は/も）（程度の語）述語（文末）」の形のときだけ当てる。
static THIN_EVALUATION: LazyLock<Regex> = LazyLock::new(|| {
    let pairs = THIN_EVALUATIONS
        .iter()
        .map(|(noun, predicate)| format!("(?:{noun})(?:が|は|も)(?:{ADVERB})?(?:{predicate})"))
        .collect::<Vec<_>>()
        .join("|");
    regex(&format!("^[^、，,]{{0,20}}?(?:{pairs})(?:{TAIL})?$"))
});
const ADVERB: &str =
    "とても|非常に|大変|たいへん|すごく|かなり|十分に?|抜群に|格段に|より|さらに|一段と";
const TAIL: &str = "です|でした|だ|だった|である|でございます|ですね|ですよ";

/// 英語の言い回しを一語ずつ写した句。（名前, 正規表現, この語が同じ文にあれば文字どおりの意味とみる）
static CALQUES: LazyLock<Vec<(&str, Regex, Option<Regex>)>> = LazyLock::new(|| {
    [
        (
            "足すものがない",
            r"足すもの(?:が|は|も)(?:ない|ありません|なかった|ありませんでした|何もない)",
            None,
        ),
        ("価値を足す", r"価値を足", None),
        ("意味をなす", r"意味を(?:なす|成す|なし|成し)", None),
        ("ステップを取る", r"ステップを取", None),
        ("違いを作る", r"違いを作", None),
        (
            "手を伸ばす",
            r"(?:部|課|室|チーム|担当者?|窓口|サポート|事務局|私たち|弊社|当社|我々)(?:まで|に|へ)(?:お?気軽に)?手を伸ば",
            None,
        ),
        ("同じページにいる", r"同じページ(?:の上)?に(?:い|立|そろ|揃)", None),
        // 「水を試しに沸かす」の「試しに」は試行の副詞で、動詞「試す」ではない。
        ("水を試す", r"水を試(?:す|し[てたま]|さ|そ|せ)", None),
        ("ループを閉じる", r"ループを閉じ", None),
        ("ループに入れる", r"ループに(?:入れ|入っ|加え)", None),
        (
            "駆動される",
            r"(?:データ|数字|数値|指標|顧客|ユーザー|利用者|テスト|仕様)に駆動され",
            None,
        ),
        (
            "配達する",
            r"(?:数字|数値|結果|成果|価値|売上|利益|成長|体験)を配達",
            None,
        ),
        (
            "大きな絵",
            r"大きな絵(?:を|が|で|の中)",
            Some(r"描|画家|美術|展示|額|飾"),
        ),
        ("インパクトを持つ", r"インパクトを持", None),
        ("決定を作る", r"決定を作", None),
        ("質問を持つ", r"質問を持[っちつ]", None),
        ("真剣に取る", r"真剣に取[るりっら]", None),
        ("目を保つ", r"目を保", None),
        (
            "テーブルに置く",
            r"(?:案|提案|選択肢|アイデア|条件|議題)を[^。]{0,6}テーブルに(?:置|載せ|乗せ|出)",
            None,
        ),
        (
            "テーブルにある",
            r"(?:案|選択肢|提案|議題|条件)(?:は|が|も|だけが|こそ)、?[^。]{0,20}?テーブル(?:の上)?に(?:あ[るり]|載って|乗って)",
            None,
        ),
        (
            "次のレベルに",
            r"次のレベルに(?:引き上げ|持っていく|押し上げ|連れて)",
            None,
        ),
    ]
    .into_iter()
    .map(|(name, pattern, literal)| (name, regex(pattern), literal.map(regex)))
    .collect()
});

static SHORT_CONCESSION: LazyLock<Regex> =
    LazyLock::new(|| regex(r"^(?:もちろん|勿論)、[^。！？!?、「」『』]{1,10}$"));
static CONCESSION_ANSWER: LazyLock<Regex> = LazyLock::new(|| {
    regex(
        r"^(?:もちろん|勿論)、(?:大丈夫|構いません|かまいません|いいですよ|いいよ|できます|可能です|です|だ)",
    )
});
static PERSONA_ENDING: LazyLock<Regex> = LazyLock::new(|| regex(r"なんよね?$"));
static TE_NOMINAL_STOP: LazyLock<Regex> = LazyLock::new(|| {
    regex(r"(?:[ぁ-ん]て|[んい]で|[一-龥々]て)[、，,][^。、，,]{1,20}?を[一-龥々]{2,}$")
});

fn is_question(sentence: &TokenizedSentence) -> bool {
    matches!(sentence.end_mark, Some('?' | '？'))
        || sentence.text.trim_end_matches(['」', '』']).ends_with('か')
}

fn in_quote(sentence: &TokenizedSentence, start: usize, end: usize) -> bool {
    crate::markup::quotation_ranges(&sentence.text)
        .iter()
        .any(|range| range.start < end && start < range.end)
}

fn raw_line<'a>(raw_lines: &[&'a str], sentence: &TokenizedSentence) -> &'a str {
    raw_lines
        .get(sentence.line - 1)
        .copied()
        .unwrap_or_default()
}

/// 性質を表す名詞（操作性、質感、見やすさ、座り心地）。
fn property_noun(tokens: &[&crate::morphology::Morpheme], index: usize) -> bool {
    let token = tokens[index];
    if token.pos(0) == "接尾辞" && matches!(token.surface.as_str(), "さ" | "み" | "感" | "性")
    {
        return true;
    }
    token.pos(0) == "名詞"
        && token.pos(1) == "普通名詞"
        && [
            "性",
            "感",
            "さ",
            "心地",
            "触り",
            "勝手",
            "ごたえ",
            "応え",
            "仕上がり",
            "味わい",
        ]
        .iter()
        .any(|suffix| token.surface.ends_with(suffix) && token.surface != *suffix)
}

/// 評価だけを述べる語（良い、高い、優れる、抜群、上質）。
fn evaluative(token: &crate::morphology::Morpheme) -> bool {
    matches!(
        token.normalized(),
        "良い"
            | "高い"
            | "優れる"
            | "抜群"
            | "上質"
            | "快適"
            | "心地良い"
            | "素晴らしい"
            | "魅力"
            | "魅力的"
            | "滑らか"
            | "豊か"
            | "格別"
            | "見事"
            | "秀逸"
            | "申し分無い"
            | "最高"
            | "優秀"
            | "優れもの"
    )
}

/// 仮説: 中身の薄い評価は語の一覧ではなく形で決まる。性質名詞を主語にし、評価語だけで文を閉じ、
/// 数・読点・目的語を持たない短い文（「本体の操作性も非常に優れています」）。
fn property_evaluation(sentence: &TokenizedSentence) -> bool {
    let tokens = sentence
        .tokens
        .iter()
        .filter(|token| !matches!(token.pos(0), "補助記号" | "記号" | "空白"))
        .collect::<Vec<_>>();
    if sentence.text.contains(['、', '，'])
        || tokens
            .iter()
            .any(|token| token.pos(1) == "数詞" || token.surface == "を")
    {
        return false;
    }
    let Some(subject) = (0..tokens.len()).find(|&index| {
        property_noun(&tokens, index)
            && tokens.get(index + 1).is_some_and(|particle| {
                particle.pos(0) == "助詞" && matches!(particle.surface.as_str(), "が" | "は" | "も")
            })
    }) else {
        return false;
    };
    let prefix = tokens[..subject]
        .iter()
        .map(|token| token.surface.chars().count())
        .sum::<usize>();
    if prefix > 20 {
        return false;
    }
    let mut index = subject + 2;
    // 程度の語（とても、非常に、格段に）。
    while tokens.get(index).is_some_and(|token| {
        token.pos(0) == "副詞"
            || (token.pos(0) == "形状詞" && !evaluative(token))
            || (token.pos(0) == "助動詞" && token.surface == "に")
    }) {
        index += 1;
    }
    if !tokens.get(index).is_some_and(|token| evaluative(token)) {
        return false;
    }
    // 評価語のあとは文末の助動詞・補助動詞・終助詞だけ。「高まりました」のような変化や、「高い評価」の連体は除く。
    tokens[index + 1..].iter().all(|token| {
        token.pos(0) == "助動詞"
            || (token.pos(0) == "動詞" && token.pos(1) == "非自立可能")
            || (token.pos(0) == "助詞" && matches!(token.pos(1), "接続助詞" | "終助詞"))
    }) && !tokens[index + 1..]
        .iter()
        .any(|token| matches!(token.normalized(), "ない" | "無い" | "ぬ" | "ず" | "た"))
}

/// 「〜のは、A、B、C。」で述語を書かずに終える文。のは の前の動詞の漢字が直前の文にあれば、前提は書いてある。
fn cleft_list_ending(sentence: &TokenizedSentence, previous: Option<&TokenizedSentence>) -> bool {
    let tokens = &sentence.tokens;
    let Some(no) = tokens.windows(3).position(|window| {
        window[0].surface == "の"
            && window[1].surface == "は"
            && window[2].pos(1) == "読点"
            && window[0].pos(0) == "助詞"
    }) else {
        return false;
    };
    let head_verb = tokens[..no]
        .iter()
        .rev()
        .find(|token| matches!(token.pos(0), "動詞" | "形容詞"));
    let Some(head_verb) = head_verb else {
        return false;
    };
    let rest = &tokens[no + 3..];
    let items = 1 + rest.iter().filter(|token| token.pos(1) == "読点").count();
    let nominal = rest.iter().all(|token| {
        matches!(
            token.pos(0),
            "名詞" | "接頭辞" | "接尾辞" | "補助記号" | "記号" | "空白" | "連体詞"
        ) || (token.pos(0) == "助詞" && matches!(token.surface.as_str(), "の" | "と" | "や"))
            || (token.pos(0) == "形状詞")
    });
    if items < 3 || !nominal || !matches!(sentence.end_mark, Some('。' | '！' | '!')) {
        return false;
    }
    previous.is_none_or(|previous| {
        !is_question(previous)
            && !head_verb
                .surface
                .chars()
                .filter(|ch| matches!(ch, '一'..='龥' | '々'))
                .any(|ch| previous.text.contains(ch))
    })
}

/// 同じ行で続く、読点のない4字以下の文（効く。重い。安い。）が3つ以上の箇所。
fn fragment_runs(tokenized: &[TokenizedSentence], raw_lines: &[&str]) -> Vec<Finding> {
    let fragment = |sentence: &TokenizedSentence| {
        !sentence
            .text
            .contains(['、', '「', '」', '『', '』', ' ', '　'])
            && (1..=4).contains(&sentence.text.chars().count())
            && matches!(sentence.end_mark, Some('。' | '！' | '!'))
    };
    let mut findings = Vec::new();
    let mut run: Vec<&TokenizedSentence> = Vec::new();
    let mut flush = |run: &mut Vec<&TokenizedSentence>| {
        if let [first, .., last] = run.as_slice()
            && run.len() >= 3
            // 同じ語の繰り返し（ヤッホー！ヤッホー！）は呼びかけで、評価の列挙ではない。
            && run.iter().any(|sentence| sentence.text != first.text)
        {
            let end = last.line_byte_start + last.text.len() - first.line_byte_start;
            let mut finding = first.info_finding(
                raw_lines,
                0..first.text.len(),
                "evaluation_fragment_run",
                "評価の語だけの短い文が3つ以上続いています（「効く。重い。安い。」など）。何がどうなのかが前後から分かる場合は、対象と評価を述語までつないでください。分からない場合は主語や理由を作らず、書き手に確かめてください。小説の語りや詩など、意図した表現は残せます。",
            );
            finding.span = first.span(raw_lines, 0, end);
            if let Some(excerpt) = raw_lines
                .get(first.line - 1)
                .and_then(|line| line.get(first.line_byte_start..first.line_byte_start + end))
            {
                finding.excerpt = excerpt.to_owned();
            }
            findings.push(finding);
        }
        run.clear();
    };
    for sentence in tokenized {
        if !fragment(sentence) || run.last().is_some_and(|last| last.line != sentence.line) {
            flush(&mut run);
        }
        if fragment(sentence) {
            run.push(sentence);
        }
    }
    flush(&mut run);
    findings
}

// nwiizo-coding-style: 語の一覧と文全体の形だけで当て、文脈の意味は判定しない;
// 引用の中、見出し、表は見ず、1文に同じ規則の候補は1件までにする。
pub(super) fn sentence_findings(
    tokenized: &[TokenizedSentence],
    raw_lines: &[&str],
    genre: Option<&str>,
) -> Vec<Finding> {
    let loanwords = regex(&format!(
        "(?:^|[^ァ-ヴー・])({})(?:$|[^ァ-ヴー・])",
        VAGUE_LOANWORDS.join("|")
    ));
    let mut findings = fragment_runs(tokenized, raw_lines);
    for (index, sentence) in tokenized.iter().enumerate() {
        let body = sentence.text.as_str();
        let line = raw_line(raw_lines, sentence);
        if line.trim_start().starts_with('|') {
            continue;
        }
        let previous = index
            .checked_sub(1)
            .map(|previous| &tokenized[previous])
            .filter(|previous| previous.line + 1 >= sentence.line);

        if let Some(found) = loanwords
            .captures_iter(body)
            .filter_map(|captures| captures.get(1))
            .find(|found| !in_quote(sentence, found.start(), found.end()))
        {
            findings.push(sentence.info_finding(
                raw_lines,
                found.range(),
                "vague_loanword",
                format!(
                    "「{}」は指す範囲が広く、読み手が中身を補えないことのあるカタカナ語です。この文書で何を指すか（誰の、どの作業や仕組みか）が前後で分かるか確認してください。定義済みの用語や固有名なら残せます。",
                    found.as_str()
                ),
            ));
        }

        if (THIN_EVALUATION.is_match(body) || property_evaluation(sentence))
            && !in_quote(sentence, 0, body.len())
        {
            findings.push(sentence.info_finding(
                raw_lines,
                0..body.len(),
                "thin_evaluation",
                "名詞がもともと持つ評価を、述語でもう一度言うだけの文です（「質感も上質です」）。素材・寸法・数値など評価の中身が文書にあればそちらに寄せ、なければ書き手に確かめてください。",
            ));
        }

        for (name, pattern, literal) in CALQUES.iter() {
            if literal
                .as_ref()
                .is_some_and(|literal| literal.is_match(body))
            {
                continue;
            }
            if let Some(found) = pattern
                .find_iter(body)
                .find(|found| !in_quote(sentence, found.start(), found.end()))
            {
                findings.push(sentence.info_finding(
                    raw_lines,
                    found.range(),
                    "calque_phrase",
                    format!(
                        "「{name}」は英語の言い回しを一語ずつ写した句です。日本語で、誰が何をどうするかを書けるか確認してください。"
                    ),
                ));
                break;
            }
        }

        if matches!(sentence.end_mark, Some('。' | '！' | '!'))
            && SHORT_CONCESSION.is_match(body)
            // 「もちろん、筆者も参加します」のように主題や主語があれば、先回りの承認ではなく事実の文。
            && !body
                .split_once('、')
                .is_some_and(|(_, rest)| rest.contains(['は', 'が', 'も']))
            && !CONCESSION_ANSWER.is_match(body)
            && !previous.is_some_and(is_question)
        {
            findings.push(sentence.info_finding(
                raw_lines,
                0..body.len(),
                "short_concession",
                "文頭の「もちろん、」のあとに数語だけが続く文です。読み手が思いそうなことを先回りして認めているだけなら「もちろん、」を外せます。問いや依頼への答え、前提の確認なら残してください。",
            ));
        }

        if genre != Some("essay")
            && PERSONA_ENDING.is_match(body.trim_end_matches(['！', '!', '？', '?']))
        {
            let start = body.rfind("なんよ").unwrap_or(0);
            findings.push(sentence.info_finding(
                raw_lines,
                start..body.len(),
                "persona_ending",
                "地の文が「なんよ」で終わっています。方言や話し言葉を意図していなければ、文書のほかの文末にそろえてください。",
            ));
        }

        if !text::is_list_item(line)
            && sentence.end_mark == Some('。')
            && TE_NOMINAL_STOP.is_match(body)
        {
            findings.push(sentence.info_finding(
                raw_lines,
                0..body.len(),
                "te_nominal_stop",
                "段落の文が「〜して、〜を改善。」のように漢語の名詞で止まっています。報告の箇条書きでなければ、「改善しました」のように述語まで書くと、したことか予定かが読み取れます。",
            ));
        }

        if cleft_list_ending(sentence, previous) {
            findings.push(sentence.info_finding(
                raw_lines,
                0..body.len(),
                "cleft_list_ending",
                "「〜のは、A、B、C。」と名詞を並べ、述語を書かずに終えています。何かが変わった・何かを目指したという前提を前の文や見出しで書いていなければ、「〜では、A、B、Cが変わった」のように述語まで書いてください。強調として選んだ形なら残せます。",
            ));
        }
    }
    findings
}

const LABEL_COMMON: &[&str] = &[
    "当番",
    "本番",
    "順番",
    "定番",
    "一番",
    "番号",
    "係",
    "関係",
    "連絡係",
    "番組",
    "交番",
    "週番",
];

// nwiizo-coding-style: 見出しの行だけを見て、形と本文での出現を照合する。
pub(super) fn heading_findings(raw: &str) -> Vec<Finding> {
    let visible = crate::text::mask_html_comments(raw);
    let lines = visible.split('\n').collect::<Vec<_>>();
    let protected = text::protected_lines(&lines);
    let body = lines
        .iter()
        .enumerate()
        .filter(|(index, line)| !protected[*index] && !text::is_heading(line))
        .map(|(_, line)| *line)
        .collect::<Vec<_>>()
        .join("\n");
    let colon = regex(r"とは[：:]");
    let bracket = regex(r"[（(](?:いわゆる[^）)]*|概要|詳細|補足|解説|説明|まとめ)[）)]");
    let mut findings = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if protected[index] {
            continue;
        }
        let Some((_, title)) = text::heading(line) else {
            continue;
        };
        let title = title.trim();
        let mut push = |category: &str, detail: &str| {
            let mut finding = Finding::new(index + 1, category, line.trim(), "info", detail);
            let start = line.len() - line.trim_start().len();
            finding.span =
                super::make_span(&lines, index + 1, start, index + 1, line.trim_end().len());
            findings.push(finding);
        };
        if colon.is_match(title) {
            push(
                "heading_colon",
                "見出しが「Xとは：Y」の形です。コロンの後の語句を落とさずに、「Xの仕組みと使い方」のような名詞句にできるか確認してください。",
            );
        }
        if bracket.is_match(title) {
            push(
                "redundant_heading_bracket",
                "見出しの補足カッコが「概要」「詳細」「いわゆる」のように、見出しの語の言い直しになっています。節の中身を表す語にまとめられるか確認してください。",
            );
        }
        let label = title.trim_matches(|ch: char| !ch.is_alphanumeric());
        let short = label.chars().count() <= 8;
        let figurative = ["の横", "のそば", "係", "番"]
            .iter()
            .any(|suffix| label.ends_with(suffix))
            && !LABEL_COMMON.iter().any(|common| {
                label.ends_with(common) && label.chars().count() <= common.chars().count() + 1
            });
        if short && figurative && !body.contains(label) {
            push(
                "metaphor_label",
                "見出しが「〜の横」「〜係」「〜番」のような短いたとえで、本文にその語がありません。節で何を説明するかが読み手に分かる名前にできるか確認してください。",
            );
        }
    }
    findings
}

fn first_person(text: &str) -> usize {
    static FIRST_PERSON: LazyLock<Regex> =
        LazyLock::new(|| regex(r"私(?:は|が|の|も|に|を|た|自身)|わたし|僕|ぼく|俺|自分|筆者"));
    FIRST_PERSON.find_iter(text).count()
}

// nwiizo-coding-style: 随筆に限った文書単位の傾向。台詞の句点と、一人称に対する数字・箇条書きの密度だけを数える。
pub(super) fn essay_findings(raw: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let prose = crate::text::revision_text(raw);
    let dialogue = regex(r"「([^「」\n]{1,200})」");
    let lines = dialogue
        .captures_iter(&prose)
        .map(|captures| captures.get(1).expect("group").as_str())
        .collect::<Vec<_>>();
    let closed = lines.iter().filter(|line| line.ends_with('。')).count();
    if lines.len() >= 10 && closed * 2 >= lines.len() {
        let first = prose
            .find("。」")
            .map_or(1, |byte| prose[..byte].matches('\n').count() + 1);
        findings.push(Finding::new(
            first,
            "dialogue_period",
            format!("台詞{}個のうち{closed}個が「。」」で終わる", lines.len()),
            "info",
            "台詞の終わりの「。」が閉じかっこの前に多く付いています。小説や随筆では「〜です」」のように句点を省く書き方が一般的です。媒体の表記方針を確認してください。",
        ));
    }
    let chars = prose.chars().filter(|ch| !ch.is_whitespace()).count();
    if chars >= 1000 {
        let per_mille = |count: usize| count as f64 * 1000.0 / chars as f64;
        let numbers = regex(r"[0-9０-９]+(?:[.,．，][0-9０-９]+)*")
            .find_iter(&prose)
            .count();
        let items = raw.lines().filter(|line| text::is_list_item(line)).count();
        if per_mille(first_person(&prose)) < 1.0 && per_mille(numbers + items) >= 8.0 {
            findings.push(Finding::new(
                1,
                "essay_report_density",
                format!(
                    "一人称 {:.1}/千字、数字と箇条書き {:.1}/千字",
                    per_mille(first_person(&prose)),
                    per_mille(numbers + items)
                ),
                "info",
                "随筆として指定した文書に一人称がほとんどなく、数字と箇条書きが多く出ています。報告書のような書き方になっていないか、書き手が見たこと・考えたことが文に残っているか確認してください。",
            ));
        }
    }
    findings
}
