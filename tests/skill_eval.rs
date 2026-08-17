use std::collections::BTreeSet;

use serde::Deserialize;

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
