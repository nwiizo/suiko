# Calibration report

Date: 2026-08-18

## Corpus and limits

The bundled set contains two human documents and one deliberately AI-smelly
fixture, all evaluated with the `essay` profile. One human document is the
7,423-character prepared text of `堕落論`; the other fixtures are shorter than
4,000 characters. This set exercises provenance, hashing, reporting, and
threshold sweeps, but it is too small and too narrow to estimate population
error rates or authorship probabilities.

## Current document-level proxies

| category | human fire rate | AI fire rate | human findings | AI findings |
|---|---:|---:|---:|---:|
| `repeated_sentence_lead` | 1/2 | 0/1 | 20 | 0 |
| `low_specificity` | 0/2 | 1/1 | 0 | 2 |
| `nominal_ending` | 0/2 | 0/1 | 0 | 0 |
| `low_lexical_diversity_ttr` | 1/2 | 0/1 | 1 | 0 |
| `low_lexical_diversity_mtld` | 0/2 | 0/1 | 0 | 0 |
| `translationese_morph` | 1/2 | 1/1 | 4 | 1 |

The `堕落論` repetition findings mostly point to deliberate `私は` and `人間は`
anaphora. Its morphology-based translationese findings point to expressions
such as `ことができる` that occur in human prose. These are useful review
prompts, not evidence of authorship.

Reading-load categories use prevalence, not false-positive/detection labels,
because readable difficulty can occur in both human and AI text. The long human
essay fires `sentence_too_long` ten times and also exercises buried lists,
double negatives, a kanji run, and a chain of `の` particles.

## Threshold sweeps

- Repeated sentence lead values 3, 5, and 7 all produce 20 human findings and
  no AI findings. Values 9–13 produce 13 human findings; 15 produces none.
- TTR values 0.35 and 0.40 produce no finding. Values 0.45 and 0.50 fire on the
  long human essay only.
- MTLD values 30–80 produce no finding. Value 100 fires on the long human essay
  only.

No default threshold changed. Lowering TTR to 0.40 would remove one human fire,
but there is no long AI document to measure the lost detection rate. Raising
the repeated-lead threshold to 15 would silence intentional repetition in this
essay, but the AI fixture does not exercise that rule at any tested threshold.
Changing either value would overfit one work.

`low_specificity`, nominal endings, morphology-based translationese, and each
reading-load category are reported, but do not yet have exposed sweep
parameters. They require labeled examples before another tuning surface is
added.

## Lindera heading fixture

`tests/fixtures/outline-lindera.md` fixes the intended IPADIC interpretation of
four headings. Overall values are length mean 5.5, length CV 0.396, nominal
ending ratio 0.75, and dominant POS-signature ratio 0.5. A change in tokenizer
behavior must update this fixture only when the new interpretation is intended.

## Reproduction

```sh
cargo run --features evaluation --bin suiko-eval -- report eval/corpus.toml
cargo run --features evaluation --bin suiko-eval -- sweep eval/corpus.toml --rule repeated-sentence-lead --values 3,5,7,9,11,13,15
cargo run --features evaluation --bin suiko-eval -- sweep eval/corpus.toml --rule low-lexical-diversity-ttr --values 0.35,0.40,0.45,0.50
cargo run --features evaluation --bin suiko-eval -- length-analysis eval/corpus.toml
```
