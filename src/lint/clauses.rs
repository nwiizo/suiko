//! 読解負荷の族が使う文の構造: 文に埋もれた並列と問いの並び、長い連体修飾。

use crate::morphology::Morpheme;

use super::morph::noun_ended;

/// 名詞で終わる区画が読点で並ぶ最長の列挙。戻り値は(開始, 終了, 項目数, 最後の項目より
/// 前に並んだ区画の字数)。最後の項目は述語に溶けるので、読み手が抱える並びの長さは
/// それより前の区画で測る。
pub(super) fn buried_list(tokens: &[Morpheme]) -> Option<(usize, usize, usize, usize)> {
    let mut bounds = Vec::new();
    let mut start = 0;
    for (index, token) in tokens.iter().enumerate() {
        if token.surface == "、" {
            bounds.push((start, index));
            start = index + 1;
        }
    }
    bounds.push((start, tokens.len()));
    // 「〜を試したところ」「〜したとき」は形式名詞で終わる副詞的な節で、名詞句の項目ではない。
    // 「〜したこと、〜したこと」のように「こと」で名詞化した節は項目として数える。
    let clause_end = |segment: &[Morpheme]| {
        let content = segment
            .iter()
            .rposition(|token| !matches!(token.pos(0), "記号" | "補助記号" | "空白"));
        content.is_some_and(|last| {
            last > 0
                && matches!(
                    segment[last].surface.as_str(),
                    "ところ"
                        | "とき"
                        | "ため"
                        | "場合"
                        | "まま"
                        | "うち"
                        | "際"
                        | "たび"
                        | "とおり"
                        | "通り"
                )
                && matches!(segment[last - 1].pos(0), "動詞" | "助動詞" | "形容詞")
        })
    };
    let mut run = Vec::new();
    let mut best = None;
    for (index, (start, end)) in bounds.iter().copied().enumerate() {
        if end > start && noun_ended(&tokens[start..end]) && !clause_end(&tokens[start..end]) {
            run.push((start, end));
        } else {
            run.clear();
        }
        if run.len() >= 2 && index + 1 < bounds.len() {
            let items = run.len() + 1;
            if best.is_none_or(|(_, _, best_items, _)| items > best_items) {
                // 1つ目の項目は、「エンジニアが向けてきた観察」の「観察」のように、並び全体に
                // 掛かる修飾を除いた最後の名詞句から数える。
                let (first_start, first_end) = run[0];
                let head_start = tokens[first_start..first_end - 1]
                    .iter()
                    .rposition(|token| {
                        // 「業務プロセス運用の見直し」の「の」は項目の内側の名詞句をつなぐ。
                        (token.pos(0) == "助詞" && token.surface != "の")
                            || matches!(
                                token.pos(0),
                                "助動詞" | "動詞" | "形容詞" | "補助記号" | "記号" | "空白"
                            )
                    })
                    .map_or(first_start, |offset| first_start + offset + 1);
                // 「〜が進んだこと」のように名詞化した節は、節全体が一つの項目になる。
                let head_start = if tokens[head_start..first_end]
                    .iter()
                    .all(|token| matches!(token.surface.as_str(), "こと" | "もの" | "の"))
                {
                    first_start
                } else {
                    head_start
                };
                let held = tokens[head_start..run[run.len() - 1].1]
                    .iter()
                    .map(|token| token.surface.chars().count())
                    .sum::<usize>();
                best = Some((run[0].0, bounds[index + 1].1, items, held));
            }
        }
    }
    best
}

/// 読点で並べた疑問節「〜か、〜か」が、末尾の「か」に続く格助詞・係助詞で
/// 一つの述語へ係る範囲。`start..end`は最初の節から末尾の助詞までのtoken範囲。
#[derive(Clone, Copy, Debug)]
pub(super) struct BuriedQuestionList {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) clauses: usize,
    pub(super) chars: usize,
}

// 選択肢を受けて選ぶ・決める・比べる・見分ける述語。サ変名詞は名詞の辞書形で照合する。
const ALTERNATIVE_PREDICATES: &[&str] = &[
    "分ける",
    "見分ける",
    "切り分ける",
    "見極める",
    "区別",
    "判別",
    "識別",
    "選ぶ",
    "選択",
    "決める",
    "決まる",
    "比べる",
    "比較",
    "判断",
    "判定",
    "確定",
];

// 疑問詞を含む節は、選択肢ではなく別々に答える問いになる。
const INTERROGATIVES: &[&str] = &[
    "何",
    "なに",
    "誰",
    "だれ",
    "どこ",
    "どう",
    "なぜ",
    "いつ",
    "どの",
    "どれ",
    "どちら",
    "どんな",
    "いくつ",
    "いくら",
    "どれほど",
    "どれだけ",
    "どうして",
];

/// 疑問節を受ける助詞の後、次の読点・句点までで最初の述語が選択肢を受ける動作か。
/// 「〜のかを、〜と選ぶ」のように助詞の直後に置いた読点は一つだけ越える。
fn governs_alternatives(tokens: &[Morpheme]) -> bool {
    let skip = usize::from(tokens.first().is_some_and(|token| token.surface == "、"));
    tokens[skip..]
        .iter()
        .take_while(|token| !matches!(token.pos(0), "補助記号" | "記号"))
        .find(|token| {
            token.pos(0) == "動詞" || (token.pos(0) == "名詞" && token.pos(2) == "サ変可能")
        })
        .is_some_and(|token| ALTERNATIVE_PREDICATES.contains(&token.dictionary_form()))
}

fn question_ka(token: &Morpheme) -> bool {
    token.surface == "か" && token.pos(0) == "助詞" && matches!(token.pos(1), "副助詞" | "終助詞")
}

/// 疑問節の同格列挙（カタログ F1の節版）。読み手は最初の「〜か、」で節が
/// 終わったと受け取り、末尾の「〜かを」で全体が一つの項だったと分かる。
/// 「はいか、いいえかを」のような短い選択肢は、節の字数と述語の有無で外す。
pub(super) fn buried_question_list(
    tokens: &[Morpheme],
    clause_min_chars: usize,
    list_min_chars: usize,
) -> Option<BuriedQuestionList> {
    // 「〜かどうか」「〜か否か」は一つの節の中にある「か」なので区切りに数えない。
    let whether_inner = |index: usize| {
        tokens
            .get(index + 1)
            .is_some_and(|token| matches!(token.surface.as_str(), "どう" | "否"))
            && tokens.get(index + 2).is_some_and(question_ka)
    };
    let whether_end = |index: usize| {
        index >= 2
            && matches!(tokens[index - 1].surface.as_str(), "どう" | "否")
            && question_ka(&tokens[index - 2])
    };
    let clause_ok = |start: usize, ka: usize| {
        tokens[start..=ka]
            .iter()
            .map(|token| token.surface.chars().count())
            .sum::<usize>()
            >= clause_min_chars
            && tokens[start..ka]
                .iter()
                .any(|token| matches!(token.pos(0), "動詞" | "形容詞" | "助動詞" | "形状詞"))
    };

    // 節の始点は、直前の文を閉じる太字記号や空白を読み飛ばした位置にする。
    let content_start = |start: usize| {
        tokens[start..]
            .iter()
            .position(|token| !matches!(token.pos(0), "記号" | "補助記号" | "空白"))
            .map_or(tokens.len(), |offset| start + offset)
    };
    let mut clause_start = content_start(0);
    let mut run = Vec::<usize>::new();
    let mut best: Option<BuriedQuestionList> = None;
    for (index, token) in tokens.iter().enumerate() {
        if token.surface == "、" {
            let closes_question = index > 0
                && question_ka(&tokens[index - 1])
                && !whether_end(index - 1)
                && clause_ok(clause_start, index - 1);
            if closes_question {
                run.push(clause_start);
            } else {
                run.clear();
            }
            clause_start = content_start(index + 1);
            continue;
        }
        if run.is_empty()
            || !question_ka(token)
            || whether_inner(index)
            || tokens
                .get(index + 1)
                .is_some_and(|next| next.surface == "、")
        {
            continue;
        }
        let Some(particle) = tokens.get(index + 1).filter(|next| {
            next.pos(0) == "助詞" && matches!(next.surface.as_str(), "を" | "が" | "は" | "も")
        }) else {
            // 「〜か、〜かで」のように別の形で続く並びは、項の列挙として数えない。
            run.clear();
            continue;
        };
        // 「〜かもしれない」は推量で、疑問節を項にしない。
        let conjecture = particle.surface == "も"
            && tokens
                .get(index + 2)
                .is_some_and(|next| matches!(next.dictionary_form(), "しれる" | "知れる"));
        // 「〜場なのか、〜場なのか、誰が〜かを決める場なのか」のように、後ろに別の疑問の
        // 「か」が続くなら、この「かを」は最後の項目の内側にある。
        let embedded = tokens[index + 2..]
            .iter()
            .take_while(|token| token.surface != "、")
            .any(question_ka);
        let clauses = run.len() + 1;
        // 「Aなのか、Bなのかを分ける」「〜するか、〜するかを選ぶ」「〜のか、それとも〜のかを」の
        // 2節は、一つの選択疑問の選択肢になる。節ごとに切ると問いが崩れるため数えない。
        // 疑問詞を含む節（「何を〜か、誰が〜かを決める」）は別々の問いなので残す。
        // Sudachiは「それとも」を「それ／と／も」に分けるため、表層をつないで照合する。
        let lead = tokens[clause_start..index]
            .iter()
            .take(3)
            .map(|token| token.surface.as_str())
            .collect::<String>();
        // 「何も」「誰か」のように「も」「か」が続く形は疑問詞ではない。
        let yes_no_clauses = !(run[0]..index).any(|position| {
            INTERROGATIVES.contains(&tokens[position].surface.as_str())
                && !tokens
                    .get(position + 1)
                    .is_some_and(|next| matches!(next.surface.as_str(), "も" | "か"))
        });
        let alternative = clauses == 2
            && (["それとも", "あるいは", "または", "もしくは"]
                .iter()
                .any(|marker| lead.starts_with(marker))
                || (yes_no_clauses && governs_alternatives(&tokens[index + 2..])));
        // 読み手が係り先まで抱えるのは並んだ節の全体なので、その字数で負担を測る。
        let list_chars = tokens[run[0]..=index]
            .iter()
            .map(|token| token.surface.chars().count())
            .sum::<usize>();
        if !conjecture
            && !embedded
            && !alternative
            && list_chars >= list_min_chars
            && clause_ok(clause_start, index)
            && best.is_none_or(|best| clauses > best.clauses)
        {
            best = Some(BuriedQuestionList {
                start: run[0],
                end: index + 2,
                clauses,
                chars: list_chars,
            });
        }
        run.clear();
    }
    best
}

/// 一つの被修飾名詞に前置された連体修飾節。`start..head`が修飾節、
/// `head..head_end`が被修飾名詞句のtoken範囲。
#[derive(Clone, Copy, Debug)]
pub(super) struct AttributiveSpan {
    pub(super) start: usize,
    pub(super) head: usize,
    pub(super) head_end: usize,
    pub(super) chars: usize,
    pub(super) predicates: usize,
}

// 連体修飾を受けても、読み手が保留する項にならない名詞。形式名詞・非自立の名詞と、
// 「〜する必要がある」「〜する可能性がある」のように述語の枕になる名詞を含む。
// 削除済みの nested_attributive（連体形の個数）が人間文書で全発火した主因は
// 「〜すること」「〜したとき」のような結びなので、被修飾名詞を実質名詞に限る。
pub(crate) const FORMAL_NOUNS: &[&str] = &[
    "こと",
    "もの",
    "ため",
    "とき",
    "時",
    "場合",
    "ところ",
    "はず",
    "わけ",
    "つもり",
    "まま",
    "うち",
    "際",
    "たび",
    "ほう",
    "方",
    "かぎり",
    "限り",
    "以上",
    "以外",
    "前",
    "後",
    "よう",
    "上",
    "中",
    "間",
    "度",
    "ごと",
    "くらい",
    "ぐらい",
    "ほど",
    "せい",
    "おかげ",
    "かわり",
    "代わり",
    "つど",
    "ゆえ",
    "点",
    "面",
    "形",
    "意味",
    "必要",
    "余地",
    "可能性",
    "恐れ",
    "おそれ",
    "危険",
    "傾向",
    "予定",
    "見込み",
    "機会",
    "状態",
    "程度",
    "気",
    "事",
    "感じ",
    "始末",
    "場面",
    "所",
    "ためし",
    "よし",
];

fn is_noun(token: &Morpheme) -> bool {
    token.pos(0) == "名詞" && matches!(token.pos(1), "普通名詞" | "固有名詞")
}

fn is_argument_head(token: &Morpheme) -> bool {
    is_noun(token) && token.pos(2) != "副詞可能" && !FORMAL_NOUNS.contains(&token.surface.as_str())
}

// 格助詞に続いて複合助詞として働く動詞（によって、について、に関する、における）。
// 形態素としては動詞だが、読み手が節として保留する述語ではないので数えない。
const COMPOUND_PARTICLE_VERBS: &[&str] = &[
    "よる",
    "つく",
    "対する",
    "関する",
    "基づく",
    "伴う",
    "おく",
    "わたる",
    "通じる",
    "沿う",
    "応じる",
    "際する",
    "めぐる",
    "つれる",
    "従う",
    "比べる",
    "加える",
    "向ける",
    "限る",
    "当たる",
    "至る",
    "関わる",
    "即する",
];

fn is_compound_particle_verb(tokens: &[Morpheme], index: usize) -> bool {
    let token = &tokens[index];
    token.pos(0) == "動詞"
        && COMPOUND_PARTICLE_VERBS.contains(&token.dictionary_form())
        && index > 0
        && tokens[index - 1].pos(1) == "格助詞"
}

/// 節の述語として数える語。複合助詞の動詞は数えない。Sudachiが非自立可能と品詞づける
/// 動詞（する、かける、始める、いる）は、「〜している」「〜し始める」のように「て」や
/// 別の動詞へ直接続くときだけ補助動詞として除き、「電話をかけた」「対応を始め」の
/// ように名詞句へ続くときは述語に数える。
fn is_predicate(tokens: &[Morpheme], index: usize) -> bool {
    let token = &tokens[index];
    match token.pos(0) {
        "形容詞" => token.pos(1) != "非自立可能",
        "動詞" => {
            if is_compound_particle_verb(tokens, index) {
                return false;
            }
            if token.pos(1) != "非自立可能" {
                return true;
            }
            let Some(previous) = index.checked_sub(1).map(|index| &tokens[index]) else {
                return true;
            };
            previous.pos(1) != "接続助詞" && !matches!(previous.pos(0), "動詞" | "助動詞")
        }
        _ => false,
    }
}

fn count_predicates(tokens: &[Morpheme], range: std::ops::Range<usize>) -> usize {
    range.filter(|&index| is_predicate(tokens, index)).count()
}

/// 読点の直前の語が、引用される終止形の並列（考えている、）か。
fn ends_terminal_clause(token: &Morpheme) -> bool {
    matches!(token.pos(0), "動詞" | "形容詞" | "助動詞") && token.pos(5).starts_with("終止形")
}

/// 読点の直前の語が、連用中止（選び、置き、触れず）か。「て」「が」などの接続助詞、
/// 「であり」「している」の補助動詞、「により」の複合助詞、手段や状態を表す
/// 「〜ことで、」「〜のままで、」の助動詞「だ」は含めない。
fn ends_conjunctive_clause(tokens: &[Morpheme], index: usize) -> bool {
    let token = &tokens[index];
    token.pos(5).starts_with("連用形")
        && match token.pos(0) {
            "助動詞" => token.dictionary_form() != "だ",
            "動詞" | "形容詞" => is_predicate(tokens, index),
            _ => false,
        }
}

/// 節の中に格助詞「が」で示した主語があるか。
fn has_subject_marker(tokens: &[Morpheme], range: std::ops::Range<usize>) -> bool {
    tokens[range]
        .iter()
        .any(|token| token.pos(1) == "格助詞" && token.surface == "が")
}

/// 係助詞「は」が主題を示すか。名詞に直接付くか、格助詞を挟んで付く「では」「には」
/// 「ことは」は主題として止める。「ときには」のように副詞可能の名詞に付くものは節の
/// 中の副詞句なので止めない。
fn is_topic_marker(tokens: &[Morpheme], index: usize) -> bool {
    let token = &tokens[index];
    if token.pos(0) != "助詞" || token.pos(1) != "係助詞" || token.surface != "は" {
        return false;
    }
    let mut cursor = index;
    while cursor > 0 && tokens[cursor - 1].pos(0) == "空白" {
        cursor -= 1;
    }
    if cursor > 0 && tokens[cursor - 1].pos(1) == "格助詞" {
        cursor -= 1;
    }
    let Some(noun) = cursor.checked_sub(1).map(|index| &tokens[index]) else {
        return false;
    };
    match noun.pos(0) {
        "代名詞" | "接尾辞" => true,
        "名詞" => noun.pos(2) != "副詞可能",
        _ => false,
    }
}

/// `end`の直前から前へたどり、読点・主題・括弧・文頭で区切られる節の始点を返す。
fn segment_start(tokens: &[Morpheme], end: usize) -> usize {
    let mut start = end;
    while start > 0 {
        let prev = &tokens[start - 1];
        if matches!(prev.pos(1), "読点" | "句点" | "括弧開" | "括弧閉")
            || is_topic_marker(tokens, start - 1)
        {
            break;
        }
        start -= 1;
    }
    start
}

/// 被修飾名詞から前へたどり、修飾節の始点を返す。文頭、主題の係助詞「は」、括弧、
/// 越えられない読点で止める。
///
/// 読点は、引用される終止形の並列なら越える。連用中止は一度だけ、読点の両側の節に
/// それぞれ述語が2つ以上あり、手前の節が「が」で示す主語を持たないときに越える。
/// 単独の述語しかない節や主語を持つ節は主節の連用中止であることが多く、含めると
/// 修飾節でない部分まで指してしまう。
fn attributive_clause_start(tokens: &[Morpheme], head: usize) -> usize {
    let mut start = head - 1;
    let mut crossed_conjunctive = false;
    while start > 0 {
        let prev = &tokens[start - 1];
        if is_topic_marker(tokens, start - 1) || matches!(prev.pos(1), "句点" | "括弧開" | "括弧閉")
        {
            break;
        }
        if prev.pos(1) == "読点" {
            let Some(before) = start.checked_sub(2) else {
                break;
            };
            if ends_terminal_clause(&tokens[before]) {
                start -= 2;
                continue;
            }
            // nwiizo-coding-style: 連用中止の読点越えは1回に限る; 三つ以上の連体節が
            // 並列する文を拾う必要が出たら、主節の連用中止と区別できる評価例を先に足す。
            let outer = segment_start(tokens, before)..start - 1;
            if !crossed_conjunctive
                && ends_conjunctive_clause(tokens, before)
                && count_predicates(tokens, start..head) >= 2
                && count_predicates(tokens, outer.clone()) >= 2
                && !has_subject_marker(tokens, outer)
            {
                crossed_conjunctive = true;
                start -= 2;
                continue;
            }
            break;
        }
        start -= 1;
    }
    start
}

/// 述語を2つ以上含み、`min_chars`字以上の連体修飾節が一つの実質名詞に前置され、
/// その名詞句が格助詞・係助詞で主節の項になっている箇所を返す。
/// 複数あれば修飾節が最も長いものを返す。
pub(super) fn long_attributive_span(
    text: &str,
    tokens: &[Morpheme],
    min_chars: usize,
) -> Option<AttributiveSpan> {
    let mut best: Option<AttributiveSpan> = None;
    for head in 1..tokens.len() {
        let marker = &tokens[head - 1];
        // 形状詞の「〜な」は句であって節ではないので、連体形でも対象にしない。
        if !marker.pos(5).starts_with("連体形")
            || (marker.pos(0) == "助動詞" && marker.dictionary_form() == "だ")
            || !is_argument_head(&tokens[head])
        {
            continue;
        }
        let mut head_end = head + 1;
        while head_end < tokens.len()
            && (is_noun(&tokens[head_end]) || tokens[head_end].pos(0) == "接尾辞")
        {
            head_end += 1;
        }
        let Some(particle) = tokens.get(head_end) else {
            continue;
        };
        // 「〜人でもある」の述語名詞や「〜関わりと〜関わり」の並列を除くため、
        // 「と」と単独の「で」は項の印に数えない。「〜研究では」の「では」は主題として数える。
        let marks_argument = particle.pos(0) == "助詞"
            && matches!(particle.pos(1), "格助詞" | "係助詞")
            && match particle.surface.as_str() {
                "が" | "は" | "を" | "も" | "に" | "へ" | "から" | "より" => true,
                "で" => tokens
                    .get(head_end + 1)
                    .is_some_and(|next| next.pos(1) == "係助詞" && next.surface == "は"),
                _ => false,
            };
        if !marks_argument {
            continue;
        }
        let start = attributive_clause_start(tokens, head);
        let predicates = count_predicates(tokens, start..head);
        if predicates < 2 {
            continue;
        }
        let chars = text[tokens[start].byte_start..tokens[head].byte_start]
            .chars()
            .filter(|c| !c.is_whitespace())
            .count();
        if chars < min_chars {
            continue;
        }
        if best.is_none_or(|best| chars > best.chars) {
            best = Some(AttributiveSpan {
                start,
                head,
                head_end,
                chars,
                predicates,
            });
        }
    }
    best
}
