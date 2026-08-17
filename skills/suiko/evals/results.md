# Trigger evaluation results

Date: 2026-08-18

The target was only the `description` field in `SKILL.md`. Three independent
evaluators classified each prompt without using the Skill body. The fixed
critical checklist required advertised-capability reasoning, exactly one
decision per case, no body-inferred capability, and explicit ambiguity notes.

## Baseline

All three evaluators produced the same train result:

| TP | TN | FP | FN | Accuracy |
|---:|---:|---:|---:|---:|
| 9 | 8 | 0 | 1 | 17/18 (94.44%) |

The single false negative was `style-profile`. The description advertised
writing and revision but did not advertise learning a reusable style profile
from past documents. Evaluators considered inferring that capability from the
Skill body invalid.

Mechanical retries: two evaluators retried JSON extraction after initially
treating the top-level object as an array. No evaluator retried or changed a
classification.

## Minimal change

The description was extended only with the existing workflow: learn a reusable
style profile from 3–5 past Japanese documents for a concrete writing task.
The exclusion for generic author-voice review was narrowed to review without a
writing or revision task. No other capability was added.

## After change and sealed holdout

Three fresh evaluators agreed on every case:

| Split | TP | TN | FP | FN | Accuracy |
|---|---:|---:|---:|---:|---:|
| train | 10 | 8 | 0 | 0 | 18/18 (100%) |
| test | 4 | 4 | 0 | 0 | 8/8 (100%) |

The holdout covered a natural Japanese sharing note, diagnosis without
rewriting, style learning, message-oriented heading revision, translation,
format-only conversion, book summarization, and code-comment deletion.

All evaluators noted the same material ambiguity: the holdout style-learning
prompt does not state the number of source documents. They treated this as an
input constraint to check after triggering because the prompt requests a
profile for a concrete new writing task. There were no decision retries and no
test mismatches.

These results measure interpretation of this fixed prompt set. They do not
prove the host platform's production router will have the same trigger rate.
