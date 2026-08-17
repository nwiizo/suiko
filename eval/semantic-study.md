# Semantic flatness study

Date: 2026-08-18

## Question

Would adjacent-sentence embeddings add a useful signal for prose that keeps
restating one idea while varying its words and sentence lengths? The comparison
uses two constructed Japanese documents:

- `semantic/flat.md`: eight paragraphs restate that peer review prevents
  defects, with varied wording, sentence counts, and sentence lengths;
- `semantic/focused.md`: five paragraphs stay on one API timeout but advance
  through observation, decomposition, reproduction, remediation, and
  monitoring.

## Existing deterministic diagnostics

| document | findings | categories |
|---|---:|---|
| flat | 1 | `low_burstiness` |
| focused | 2 | `low_burstiness`, `uniform_paragraph_structure` |

The current rules notice rhythm in both documents but do not distinguish
semantic restatement from deliberate deepening. `outline` also exposes their
structure, but cannot measure whether successive sentences add a new claim.

The stripped release `suiko` binary was 51,181,520 bytes on arm64 macOS. With
warm filesystem caches, both documents completed in single-digit milliseconds;
the measurement is near the benchmark tool's process-startup resolution.

## Opt-in embedding experiment

A temporary executable used a 384-dimensional, quantized multilingual MiniLM
ONNX model. It split on Japanese sentence terminators, embedded each sentence,
and measured cosine similarity between adjacent sentences.

| document | sentences | inference | adjacent mean | adjacent standard deviation | range |
|---|---:|---:|---:|---:|---:|
| flat | 16 | 138 ms | 0.3984 | 0.1505 | 0.0878–0.6171 |
| focused | 13 | 81 ms | 0.4165 | 0.1884 | 0.0379–0.6489 |

The first run, including acquisition and initialization, took 50.6 seconds.
A cached run initialized in 653 ms and used 101–109 ms per document. Its model
cache occupied 257 MiB, the separate experimental executable was 33,840,128
bytes, and peak resident memory reached about 740 MB.

The flatter document had a lower similarity standard deviation, but the mean
scores were close and the per-pair ranges overlapped heavily. A threshold could
separate these two authored examples only because there are two examples; it
does not establish a useful false-positive rate on real prose. The embedding
signal therefore adds a hypothesis, not a calibrated finding.

## Decision

Do not integrate semantic flatness detection in version 0.1. The experiment did
not establish reliable incremental value, while the smallest practical local
candidate added a 257 MiB cache, roughly 100 ms per short document, substantial
memory use, and an initial network acquisition. Bundling the model would make
the distribution several times larger; downloading it on first use would break
the no-runtime-download boundary.

Keep semantic progression in visual review: ask whether each paragraph adds an
observation, reason, consequence, example, decision, or limitation. Reconsider
an opt-in external process only after a larger blind set demonstrates added
value and a stable threshold.
