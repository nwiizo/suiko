//! 精読すると意味が定まらない表現を、利用者が指定した外部のLLMコマンドに確認させる。
//!
//! 基準はtodesking「スロップ表現100連発」の意味破綻型・過剰圧縮型。形態素の並びだけでは、
//! 比喩の写像が成り立つか、標語の内容が前後で補えるかを判定できない（suiko-evalの生成文評価、
//! 2026-11-06）。suiko自身はAPIの鍵もHTTPの通信も持たず、`claude -p`・`codex exec`等の
//! 既存のコマンドへ標準入力で依頼する。結果は実行のたびに変わり得るため、決定的な
//! `lint`・`check`・`verify`には含めない。
//!
//! 依頼の送信（`request`）と、返答を本文の範囲へ照合する処理（`findings`）を分ける。
//! 評価では返答を保存して照合だけを再実行できる。

use std::io::Write;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::Error;
use crate::lint::{Finding, Span};

/// 依頼文の版。返答を保存して比べるときに、どの依頼文の返答かを区別する。
pub const PROMPT_VERSION: u32 = 2;

/// LLMへの依頼文。本文はこの後ろに付ける。例は記事（todesking「スロップ表現100連発」）と
/// yomiyasu の slop-catalog に載っているものだけを使い、評価用の生成文から取らない。
///
/// 版1は基準と「選ばないもの」を文で伝えるだけで、Sonnetが生成文の対照（文体の指定なし）の
/// 自然な文にも1000字あたり1.5件付けた（suiko-eval、2026-11-06）。版2は、残す比喩の目印を
/// yomiyasuから足し、候補ごとに精読した読み（`reading`）を書かせる。読みを書けた候補は
/// suikoの側で捨てる（`findings`）。
pub const PROMPT: &str = r#"次の日本語の文書から、精読しても意味が一つに定まらない表現を抜き出してください。多くの文書では該当は0〜2件です。

## 基準
1. semantic_breakdown（意味破綻型）: 流し読みでは自然に見えるが、精読すると意味が破綻している表現。主語と述語だけを取り出すと成り立たず、何を言いたいかを連想もできない比喩、動作と主語が釣り合わない比喩、感覚の動詞の取り違えなど。例:「窓ガラスが夜をやさしく折りたたんでいる」「玄関の靴べらが、片方だけ消えた靴の形を、いつまでも空中に留めていた」「その音だけは全員の膝の上に等しく乗っている」「午後が膝を抱える」「嵐がそっと耳打ちした」「猫が日だまりの温かさを聞く」
2. over_compression（過剰圧縮型）: 短く言い切ろうとして情報を省きすぎ、内容が一つに定まらない表現。patternは次のどれか。例:「迷ったら、人を守る」「権限には持ち主を置く」「成果で働く、場所は手段」
   - 省きすぎ: 誰が・何を・何のために、が文書の前後からも復元できない
   - 動詞の独自用法: 動詞を通常取らない目的語・格で使い、意味が一つに決まらない
   - 主題と述語の不一致: 主題（〜は）と述語が噛み合わない

## 選ばないもの
- 字義どおりの文、ふつうの説明や感想の文。
- 何を何に例えたかを読み取れる比喩（「つぶやきが冷えた窓ガラスに白い輪を残した」は息で窓が曇る様子と読める）。
- 前後の文で例えた理由を述べている比喩。
- 働きが一読で分かる擬人化（「センサーが異常に気づく」「バッファは書き込みをいったん預かっておく」）。
- 定着した慣用句、前後の文や見出しから内容を補える標語。

## 手順
候補ごとに、精読と前後の文から読み取れる意味を一つ書いてみてください。書ければ、その候補は意味が定まっているので readingに書き、書けなければ readingをnullにします。

## 出力
JSONだけを出力してください。説明やコードブロックは不要です。
{"items":[{"text":"本文からそのまま写した部分文字列","kind":"semantic_breakdown","pattern":null,"reading":null,"reason":"なぜ意味が定まらないかを1文で"}]}
- textは本文に一度だけ現れる、改行を含まない部分文字列にする（主語から述語まで程度）。
- kindはsemantic_breakdownかover_compression。patternはover_compressionのときだけ上の3つのどれか、それ以外はnull。
- readingは、意味を一つに読めたときだけその読みを書く。読めなければnull。
- 該当がなければ {"items":[]} を出力する。

## 文書
"#;

const SEMANTIC_BREAKDOWN: &str = "semantic_breakdown";
const OVER_COMPRESSION: &str = "over_compression";

/// 項目は一つずつ読む。LLMの返答には同じキーの重複（`"pattern"`が2回など）や
/// 型の誤りが混ざるため、一件の誤りで返答全体を捨てない。
#[derive(Deserialize)]
struct Response {
    items: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct Item {
    text: String,
    kind: String,
    #[serde(default)]
    pattern: Option<String>,
    /// 版2の依頼文で、LLMが精読して読み取れた意味。書けた候補は意味が定まっているので捨てる。
    #[serde(default)]
    reading: Option<String>,
    #[serde(default)]
    reason: String,
}

/// 本文の範囲に照合できなかった抜き出し。LLMが本文を写し損ねた場合などに残す。
#[derive(Clone, Debug, Serialize)]
pub struct Discarded {
    pub text: String,
    pub reason: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct MeaningReport {
    pub findings: Vec<Finding>,
    pub discarded: Vec<Discarded>,
}

/// 依頼文と本文を`command`の標準入力へ渡し、標準出力をそのまま返す。
/// `command`は利用者が指定したプログラムと引数で、本文やLLMの返答から組み立てない。
pub fn request(body: &str, command: &[String]) -> Result<String, Error> {
    let (program, args) = command.split_first().ok_or_else(|| {
        Error::InvalidArguments(
            "LLMのコマンドを `--` の後に指定してください（例: -- claude -p --model haiku）"
                .to_owned(),
        )
    })?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            Error::InvalidArguments(format!(
                "LLMのコマンドを起動できません: {program} ({error})"
            ))
        })?;
    let input = format!("{PROMPT}{body}");
    let mut stdin = child.stdin.take().expect("stdin is piped");
    // 子が標準出力を書きながら入力を待つ場合に詰まらないよう、書き込みを別スレッドで行う。
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let output = child.wait_with_output().map_err(|error| {
        Error::InvalidArguments(format!("LLMのコマンドの終了を待てません: {error}"))
    })?;
    // 子が入力を読み切らずに終了した場合の書き込み失敗は、終了状態のほうで扱う。
    let _ = writer.join();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail = stderr.trim().chars().rev().take(400).collect::<Vec<_>>();
        return Err(Error::InvalidArguments(format!(
            "LLMのコマンドが失敗しました（{}）: {}",
            output.status,
            tail.into_iter().rev().collect::<String>()
        )));
    }
    String::from_utf8(output.stdout)
        .map_err(|_| Error::InvalidArguments("LLMの出力がUTF-8ではありません".to_owned()))
}

/// 返答の中で`items`を持つ最後のJSONオブジェクトを読む。前後の説明やコードブロックの囲みは無視する。
/// LLMは一度出した答えを撤回して二つ目のJSONを出すことがあるため、最後の答えを採る。
fn parse(raw: &str) -> Result<Response, Error> {
    raw.match_indices('{')
        .rev()
        .find_map(|(start, _)| {
            // serde_json::Valueは重複したキーを後の値で上書きして読む。構造体へ直接読むと重複で失敗する。
            let value = serde_json::Deserializer::from_str(&raw[start..])
                .into_iter::<serde_json::Value>()
                .next()?
                .ok()?;
            value
                .get("items")
                .is_some()
                .then(|| serde_json::from_value(value).ok())
                .flatten()
        })
        .ok_or_else(|| {
            Error::InvalidArguments("LLMの出力に items を持つJSONがありません".to_owned())
        })
}

fn span_of(body: &str, start: usize, end: usize) -> (usize, Span) {
    let line_start = body[..start].rfind('\n').map_or(0, |index| index + 1);
    let line = body[..start].matches('\n').count() + 1;
    let start_byte = start - line_start;
    let end_byte = end - line_start;
    let line_text = &body[line_start..];
    let column = |byte: usize| line_text[..byte].chars().count() + 1;
    (
        line,
        Span {
            start_line: line,
            start_column: column(start_byte),
            end_line: line,
            end_column: column(end_byte),
            start_byte,
            end_byte,
        },
    )
}

/// LLMの返答を本文の範囲へ照合する。本文に一度だけ、改行を含まずに現れる抜き出しだけを指摘にする。
pub fn findings(body: &str, raw: &str) -> Result<MeaningReport, Error> {
    let mut report = MeaningReport {
        findings: Vec::new(),
        discarded: Vec::new(),
    };
    for value in parse(raw)?.items {
        let Ok(item) = serde_json::from_value::<Item>(value.clone()) else {
            report.discarded.push(Discarded {
                text: value.to_string(),
                reason: "項目の形式が不正",
            });
            continue;
        };
        let discard = |reason| Discarded {
            text: item.text.clone(),
            reason,
        };
        let category = match item.kind.as_str() {
            SEMANTIC_BREAKDOWN => SEMANTIC_BREAKDOWN,
            OVER_COMPRESSION => OVER_COMPRESSION,
            _ => {
                report.discarded.push(discard("kindが基準にない"));
                continue;
            }
        };
        if item
            .reading
            .as_deref()
            .is_some_and(|reading| !reading.trim().is_empty())
        {
            report.discarded.push(discard("LLMが意味を一つに読めた"));
            continue;
        }
        if item.text.trim().is_empty() || item.text.contains('\n') {
            report.discarded.push(discard("空か、改行を含む"));
            continue;
        }
        let mut found = body
            .match_indices(item.text.as_str())
            .map(|(index, _)| index);
        let start = match (found.next(), found.next()) {
            (Some(start), None) => start,
            (None, _) => {
                report.discarded.push(discard("本文にない"));
                continue;
            }
            _ => {
                report.discarded.push(discard("本文に複数回現れる"));
                continue;
            }
        };
        let (line, span) = span_of(body, start, start + item.text.len());
        let pattern = item
            .pattern
            .as_deref()
            .filter(|_| category == OVER_COMPRESSION)
            .map(|pattern| format!("（{pattern}）"))
            .unwrap_or_default();
        let mut finding = Finding::new(
            line,
            category,
            &item.text,
            "info",
            format!(
                "LLMによる確認候補{pattern}: {} 精読して意味が一つに定まるなら残してください。実行ごとに結果が変わり得ます。",
                item.reason.trim()
            ),
        );
        finding.span = Some(span);
        report.findings.push(finding);
    }
    report.findings.sort_by_key(|finding| finding.line);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn findings_locate_unique_single_line_text_and_discard_the_rest() {
        let body = "# 題\n\n雑談は、仕事だ。窓ガラスが夜を折りたたむ。\n仕事だ。\n";
        let raw = r#"説明
```json
{"items":[
 {"text":"窓ガラスが夜を折りたたむ","kind":"semantic_breakdown","pattern":null,"reason":"窓は夜を畳めない。"},
 {"text":"雑談は、仕事だ","kind":"over_compression","pattern":"主題と述語の不一致","reason":"何が仕事か定まらない。"},
 {"text":"仕事だ","kind":"over_compression","pattern":"省きすぎ","reason":"重複"},
 {"text":"本文にない文","kind":"semantic_breakdown","reason":"写し損ね"},
 {"text":"雑談","kind":"other","reason":"分類外"}
]}
```"#;
        let report = findings(body, raw).unwrap();
        let found = report
            .findings
            .iter()
            .map(|finding| (finding.category.as_str(), finding.excerpt.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            found,
            [
                ("semantic_breakdown", "窓ガラスが夜を折りたたむ"),
                ("over_compression", "雑談は、仕事だ"),
            ]
        );
        let span = report.findings[0].span.unwrap();
        let line = body.lines().nth(span.start_line - 1).unwrap();
        assert_eq!(
            &line[span.start_byte..span.end_byte],
            "窓ガラスが夜を折りたたむ"
        );
        assert_eq!(span.start_column, 9);
        assert!(report.findings[1].detail.contains("（主題と述語の不一致）"));
        let reasons = report
            .discarded
            .iter()
            .map(|item| item.reason)
            .collect::<Vec<_>>();
        assert_eq!(
            reasons,
            ["本文に複数回現れる", "本文にない", "kindが基準にない"]
        );
        assert!(findings(body, "該当なし").is_err());
        // 重複したキーは後の値で読み、形式の誤った項目は捨てて残りを使う。
        let tolerant = findings(
            body,
            r#"{"items":[{"text":"窓ガラスが夜を折りたたむ","kind":"semantic_breakdown","pattern":null,"pattern":null,"reason":"重複"},{"text":1}]}"#,
        )
        .unwrap();
        assert_eq!(tolerant.findings.len(), 1);
        assert_eq!(tolerant.discarded[0].reason, "項目の形式が不正");
        // 答えを撤回して二つ目のJSONを出した返答は、最後の答えを採る。
        let revised = findings(
            body,
            "{\"items\":[{\"text\":\"窓ガラスが夜を折りたたむ\",\"kind\":\"semantic_breakdown\",\"reason\":\"a\"}]}\n\n訂正します。\n\n{\"items\":[]}",
        )
        .unwrap();
        assert!(revised.findings.is_empty());
        // 精読した読みを書けた候補は、意味が定まっているので捨てる。
        let read = findings(
            body,
            r#"{"items":[{"text":"窓ガラスが夜を折りたたむ","kind":"semantic_breakdown","reading":"窓に夜景が畳まれて映る","reason":"a"},{"text":"雑談は、仕事だ","kind":"over_compression","pattern":"省きすぎ","reading":null,"reason":"b"}]}"#,
        )
        .unwrap();
        assert_eq!(read.findings.len(), 1);
        assert_eq!(read.findings[0].excerpt, "雑談は、仕事だ");
        assert_eq!(read.discarded[0].reason, "LLMが意味を一つに読めた");
        assert!(
            findings(body, r#"{"items":[]}"#)
                .unwrap()
                .findings
                .is_empty()
        );
    }
}
