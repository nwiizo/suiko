use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;

// SKILL.mdが参照する相対パスのファイルがすべてバンドルに存在することを
// オフラインで検証する。実登録の検証は scripts/verify-skill-install.sh。
#[test]
fn skill_bundle_references_resolve() {
    let root = Path::new("skills/suiko");
    for required in [
        "SKILL.md",
        "agents/openai.yaml",
        "references/manual-checklist.md",
        "assets/style-profile-template.md",
    ] {
        assert!(root.join(required).is_file(), "missing {required}");
    }

    let skill = std::fs::read_to_string(root.join("SKILL.md")).expect("read SKILL.md");
    let link = regex_lite(&skill);
    for target in link {
        assert!(
            root.join(&target).is_file(),
            "SKILL.md references missing file: {target}"
        );
    }
}

// 依存を増やさないための最小のMarkdownリンク抽出。](path) 形式のうち
// 外部URLとアンカーを除いた相対パスを返す。
fn regex_lite(text: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("](") {
        rest = &rest[start + 2..];
        let Some(end) = rest.find(')') else { break };
        let target = &rest[..end];
        if !target.starts_with("http") && !target.starts_with('#') && !target.is_empty() {
            targets.push(target.to_owned());
        }
        rest = &rest[end + 1..];
    }
    targets
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TriggerSuite {
    version: u32,
    cases: Vec<TriggerCase>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TriggerCase {
    id: String,
    split: String,
    expected: String,
    capability: String,
    prompt: String,
}

#[test]
fn trigger_cases_have_a_stable_complete_shape() {
    let suite: TriggerSuite =
        serde_json::from_str(include_str!("../skills/suiko/evals/trigger-cases.json"))
            .expect("valid trigger evaluation suite");

    assert_eq!(suite.version, 1);
    assert_eq!(suite.cases.len(), 26);
    let mut ids = BTreeSet::new();
    let mut train = 0;
    let mut test = 0;
    for case in suite.cases {
        assert!(ids.insert(case.id), "duplicate case id");
        match case.split.as_str() {
            "train" => train += 1,
            "test" => test += 1,
            other => panic!("unknown split: {other}"),
        }
        assert!(matches!(case.expected.as_str(), "trigger" | "not_trigger"));
        assert!(!case.capability.trim().is_empty());
        assert!(!case.prompt.trim().is_empty());
    }
    assert_eq!((train, test), (18, 8));
}
