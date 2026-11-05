//! `suiko meaning` は利用者が指定したコマンドへ依頼を渡し、返ったJSONを本文の範囲へ照合する。
//! LLMの代わりに固定の返答を出すコマンドで、依頼の受け渡しと照合を決定的に確かめる。

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::Value;

const BODY: &str = "# 題\n\n雑談は、仕事だ。窓ガラスが夜を折りたたむ。\n";
const RESPONSE: &str = r#"{"items":[
 {"text":"窓ガラスが夜を折りたたむ","kind":"semantic_breakdown","pattern":null,"reason":"窓は夜を畳めない。"},
 {"text":"本文にない文","kind":"semantic_breakdown","pattern":null,"reason":"写し損ね"}
]}"#;

fn fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let body = dir.path().join("body.md");
    let response = dir.path().join("response.json");
    std::fs::write(&body, BODY).unwrap();
    std::fs::write(&response, RESPONSE).unwrap();
    (dir, body, response)
}

#[test]
fn meaning_maps_the_command_response_to_spans_and_reports_discards() {
    let (_dir, body, response) = fixture();
    let output = cargo_bin_cmd!("suiko")
        .arg("meaning")
        .arg(&body)
        .arg("--json")
        .arg("--")
        .arg("cat")
        .arg(&response)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["deterministic"], false);
    assert_eq!(json["command"][0], "cat");
    let findings = json["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["category"], "semantic_breakdown");
    assert_eq!(findings[0]["line"], 3);
    assert_eq!(findings[0]["excerpt"], "窓ガラスが夜を折りたたむ");
    assert_eq!(findings[0]["span"]["start_column"], 9);
    assert_eq!(json["discarded"][0]["reason"], "本文にない");

    cargo_bin_cmd!("suiko")
        .arg("meaning")
        .arg(&body)
        .arg("--")
        .arg("cat")
        .arg(&response)
        .assert()
        .success()
        .stdout(predicate::str::contains("LLMによる確認候補 1件"))
        .stdout(predicate::str::contains(
            "L3 [semantic_breakdown] 窓ガラスが夜を折りたたむ",
        ))
        .stdout(predicate::str::contains(
            "本文と照合できなかった抜き出し: 1件",
        ));
}

#[test]
fn meaning_sends_the_criteria_and_body_on_stdin() {
    let (dir, body, _) = fixture();
    let received = dir.path().join("received.txt");
    // 標準入力を保存して空の結果を返すコマンドで、依頼文と本文が届いたことを確かめる。
    cargo_bin_cmd!("suiko")
        .arg("meaning")
        .arg(&body)
        .arg("--")
        .arg("sh")
        .arg("-c")
        .arg(r#"cat > "$0"; echo '{"items":[]}'"#)
        .arg(&received)
        .assert()
        .success()
        .stdout(predicate::str::contains("LLMによる確認候補 0件"));
    let input = std::fs::read_to_string(&received).unwrap();
    assert!(input.starts_with(suiko::meaning::PROMPT));
    assert!(input.ends_with(BODY));
    assert!(input.contains("semantic_breakdown（意味破綻型）"));

    // JSONを返さないコマンドは失敗として扱う。
    cargo_bin_cmd!("suiko")
        .arg("meaning")
        .arg(&body)
        .arg("--")
        .arg("echo")
        .arg("該当なし")
        .assert()
        .failure()
        .stderr(predicate::str::contains("JSON"));
}

#[test]
fn meaning_requires_a_command_and_reports_its_failure() {
    let (_dir, body, _) = fixture();
    cargo_bin_cmd!("suiko")
        .arg("meaning")
        .arg(&body)
        .assert()
        .failure()
        .stderr(predicate::str::contains("COMMAND"));
    cargo_bin_cmd!("suiko")
        .arg("meaning")
        .arg(&body)
        .arg("--")
        .arg("false")
        .assert()
        .failure()
        .stderr(predicate::str::contains("LLMのコマンドが失敗しました"));
    cargo_bin_cmd!("suiko")
        .arg("meaning")
        .arg(&body)
        .arg("--")
        .arg("suiko-no-such-command")
        .assert()
        .failure()
        .stderr(predicate::str::contains("LLMのコマンドを起動できません"));
}
