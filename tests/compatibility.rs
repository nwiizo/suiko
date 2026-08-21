use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;

fn lint_fixture(name: &str, experimental: bool) -> Value {
    let path = format!("tests/fixtures/{name}");
    let mut command = cargo_bin_cmd!("suiko");
    command.args(["lint", &path, "--json"]);
    if experimental {
        command.arg("--experimental");
    }
    let output = command.output().expect("run suiko lint");
    assert!(output.status.success());
    serde_json::from_slice(&output.stdout).expect("valid JSON output")
}

#[test]
fn calibrated_fixtures_keep_their_finding_counts() {
    let smelly = lint_fixture("ai-smelly.md", false);
    let smelly_experimental = lint_fixture("ai-smelly.md", true);
    let natural = lint_fixture("natural.md", false);
    let natural_experimental = lint_fixture("natural.md", true);

    assert_eq!(smelly["stats"]["total_findings"], 21);
    assert_eq!(smelly_experimental["stats"]["total_findings"], 30);
    assert_eq!(natural["stats"]["total_findings"], 0);
    assert_eq!(natural_experimental["stats"]["total_findings"], 0);
}

// 一般校正の担当範囲外であることを固定する。詳細は eval/boundary/README.md。
#[test]
fn general_proofreading_problems_stay_out_of_scope() {
    for fixture in ["typo", "misuse", "variant", "keigo", "risk"] {
        let path = format!("eval/boundary/{fixture}.md");
        let mut command = cargo_bin_cmd!("suiko");
        let output = command
            .args(["lint", &path, "--experimental", "--reading-load", "--json"])
            .output()
            .expect("run suiko lint");
        assert!(output.status.success());
        let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
        assert_eq!(
            json["stats"]["total_findings"], 0,
            "boundary fixture {fixture} must stay quiet"
        );
        assert_eq!(json["reading_load"]["stats"]["total"], 0);
    }

    // 公用文の「〜によることができる」だけは、翻訳調の確認候補(info)として発火する。
    let output = cargo_bin_cmd!("suiko")
        .args([
            "lint",
            "eval/boundary/koyobun.md",
            "--experimental",
            "--json",
        ])
        .output()
        .expect("run suiko lint");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    assert_eq!(json["stats"]["total_findings"], 1);
    assert_eq!(json["stats"]["by_category"]["translationese_morph"], 1);
    assert_eq!(json["findings"][0]["severity"], "info");
}

#[test]
fn calibrated_categories_match_the_expected_profile() {
    let report = lint_fixture("ai-smelly.md", false);
    assert_eq!(report["stats"]["by_category"]["forbidden_phrase"], 8);
    // antithesis_repetitionは一致数を母数にした文書単位の集約finding(1件)
    assert_eq!(report["stats"]["by_category"]["antithesis_repetition"], 1);
    assert_eq!(report["stats"]["by_category"]["translationese"], 5);
    // Sudachi(Mode C)の長単位化で1段落の内容語数と抽象名詞率が変わり、
    // Lindera/IPADIC時代の2件から1件になった。残る1件はL22の一般論段落。
    assert_eq!(report["stats"]["by_category"]["low_specificity"], 1);
    assert_eq!(report["stats"]["by_category"]["low_burstiness"], 1);
    assert_eq!(report["stats"]["by_category"]["inanimate_subject_morph"], 1);
}
