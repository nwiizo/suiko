use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;

#[test]
fn finding_keeps_its_existing_public_fields_and_json_shape() {
    let finding = suiko::lint::Finding {
        line: 1,
        category: "translationese".to_owned(),
        excerpt: "対象の文".to_owned(),
        severity: "info".to_owned(),
        detail: "表現を確認する".to_owned(),
        related_lines: None,
        status: None,
        span: None,
        suggestion: None,
        review_suggestion: None,
    };
    assert_eq!(
        serde_json::to_value(finding).unwrap(),
        serde_json::json!({
            "line": 1,
            "category": "translationese",
            "excerpt": "対象の文",
            "severity": "info",
            "detail": "表現を確認する",
            "related_lines": null,
        })
    );
}

fn lint_fixture(name: &str) -> Value {
    let path = format!("tests/fixtures/{name}");
    let output = cargo_bin_cmd!("suiko")
        .args(["lint", &path, "--json"])
        .output()
        .expect("run suiko lint");
    assert!(output.status.success());
    serde_json::from_slice(&output.stdout).expect("valid JSON output")
}

#[test]
fn calibrated_fixtures_keep_their_finding_counts() {
    let smelly = lint_fixture("ai-smelly.md");
    let natural = lint_fixture("natural.md");

    // v0.4.3から全規則を既定で表示する（`--experimental`を付けても出力が同じことは
    // tests/cli.rsのlint_shows_former_experimental_rules_by_defaultが確かめる）。
    // 従来の--experimentalの28件に、箇所ごとの「AではなくB」（negated_contrast）3件が加わる。
    // L12「この事実は、」の後ろに「変えること」の連体修飾があるため、読点の削除候補は控える。
    assert_eq!(smelly["stats"]["total_findings"], 31);
    assert_eq!(smelly["stats"]["by_category"]["negated_contrast"], 3);
    assert!(smelly["stats"]["by_category"]["short_topic_comma"].is_null());
    assert_eq!(natural["stats"]["total_findings"], 0);
}

// 一般校正の担当範囲外であることを固定する。詳細は tests/fixtures/boundary/README.md。
#[test]
fn general_proofreading_problems_stay_out_of_scope() {
    // v0.4.3の文末の連続と「AではなくB」は、誤字・誤用とは別の箇所（3文続く「ます」、
    // 「わけではなく、単に朝が弱いだけだ」）を指す。境界の問題そのものは指さない。
    for (fixture, expected) in [
        ("typo", Some(("repeated_sentence_ending", "ます"))),
        (
            "misuse",
            Some(("negated_contrast", "ではなく、単に朝が弱いだけだ")),
        ),
        ("variant", None),
        ("keigo", None),
        ("risk", None),
    ] {
        let path = format!("tests/fixtures/boundary/{fixture}.md");
        let mut command = cargo_bin_cmd!("suiko");
        let output = command
            .args(["lint", &path, "--reading-load", "--json"])
            .output()
            .expect("run suiko lint");
        assert!(output.status.success());
        let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
        let body = std::fs::read_to_string(&path).expect("read fixture");
        let found = json["findings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|finding| {
                let span = &finding["span"];
                let line = body
                    .lines()
                    .nth(span["start_line"].as_u64().unwrap() as usize - 1)
                    .unwrap();
                let text = &line[span["start_byte"].as_u64().unwrap() as usize
                    ..span["end_byte"].as_u64().unwrap() as usize];
                (
                    finding["category"].as_str().unwrap().to_owned(),
                    text.to_owned(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            found,
            expected
                .map(|(category, text)| (category.to_owned(), text.to_owned()))
                .into_iter()
                .collect::<Vec<_>>(),
            "boundary fixture {fixture} must not point at the boundary problem"
        );
        assert_eq!(json["reading_load"]["stats"]["total"], 0);
    }

    // 公用文では翻訳調に加え、「下さい」「出来」のかな表記を確認候補として示す。
    // 文体の誤りとは判定せず、すべてinfoにとどめる。
    let output = cargo_bin_cmd!("suiko")
        .args(["lint", "tests/fixtures/boundary/koyobun.md", "--json"])
        .output()
        .expect("run suiko lint");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    assert_eq!(json["stats"]["total_findings"], 3);
    assert_eq!(json["stats"]["by_category"]["translationese_morph"], 1);
    assert_eq!(json["stats"]["by_category"]["auxiliary_kana"], 2);
    assert!(
        json["findings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|finding| finding["severity"] == "info")
    );
}

#[test]
fn calibrated_categories_match_the_expected_profile() {
    let report = lint_fixture("ai-smelly.md");
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
