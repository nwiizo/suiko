# Evaluation corpus

`corpus.toml` is the versioned input to `suiko-eval`. It has two record types:

Corpus roles are kept separate and a sample is never reused for a role it was
not assigned to:

- **threshold tuning (dev)**: documents/samples with `split = "dev"` (the
  default). `sweep` only ever sees these.
- **holdout**: `split = "holdout"`, stratified so genre, document type, and
  author do not overlap with dev. Evaluated once after thresholds are fixed;
  never used to re-tune. (Populated 2026-08-18: 3 Aozora essays by authors
  absent from dev; external web sources carry their own split in
  `sources.toml`.)
- **syntactic boundary fixtures**: `eval/labeled/` contrast pairs that pin
  detector behavior, not population rates.
- **real-usage stress**: external repositories (e.g., the 2026-08-18 technical
  book translation run) recorded in TODO.md/calibration.md, not copied here.

The human/AI `label` records **provenance, not quality**. Suiko's goal is
useful revision prompts, not authorship detection: AI-origin text can be
excellent and human text can be monotonous, and paraphrase can defeat
origin-detection entirely. Detection/fpr over these labels are calibration
proxies only, and authorship accuracy is never a success criterion. AI
documents serve as stress slices (by model, prompt, and date), not as targets
to maximize detection against.

Statistical contract: every `suiko-eval` output records the manifest SHA-256
(the corpus version), the resampling unit, and Wilson 95% intervals next to
each rate; denominators below 5 are marked `low_n` and are not quoted as
performance figures. Human annotation follows `annotation-guide.md`, and rule
adoption preconditions (minimum sample sizes, acceptable false-positive
bounds) are pre-registered there.

- `[[document]]`: whole documents with a human/AI label, Suiko genre,
  provenance, usage terms, and SHA-256. `report`, `sweep`, and
  `length-analysis` use these for document-level fire-rate proxies.
- `[[sample]]`: small labeled fixtures with a single target `category` and an
  `expect = "fire" | "silent"` ground truth. `labeled` uses these to compute
  per-category detection (fire samples that fired) and false-positive rate
  (silent samples that fired). Samples are in-repo specification fixtures, so
  they carry no SHA-256; git tracks their content.

The command rejects unknown fields, unknown categories, duplicate IDs, and
hash mismatches. Category fire rates are calibration proxies for comparing
thresholds; they are not probabilities that a document was written by AI.

Morphological analysis uses sudachi.rs with the SudachiDict version pinned in
`build.rs`. Changing the dictionary or tokenizer version can change these
measurements; rerun `report`, `labeled`, and the sweeps after any change.

## Corpus acquisition (`sources.toml`)

`sources.toml` is the acquisition manifest for human documents: 93 entries
(81 web + 12 Aozora) seeded from `corpus/sources.json` of coji/natural-japanese
(MIT) at `0f1cc1c`, with per-entry URL, author, genre, license, and a
`split` that never lets one unit (web domain / Aozora author) span dev and
holdout. The 12 `slide` entries upstream were excluded: Suiko has no slide
genre, and slide PDFs keep almost no prose after masking.

- **Aozora (public domain)**: committed under `corpus/aozora/` with
  provenance headers, and registered as `[[document]]` entries in
  `corpus.toml`.
- **Web (copyrighted)**: never committed. `uv run scripts/fetch-corpus.py`
  downloads each URL to `eval/corpus/external/` (gitignored) and records the
  SHA-256, char count, and extraction method of every fetched file in
  `external-lock.json` (committed), so measurements can state exactly which
  fetch they were computed on. The 2026-08-18 fetch succeeded for 81/81
  entries. One note.com entry (`note-essay-ciotan-8a0bfe52`) is a paid
  article; only its free portion is fetched.
- **AI documents**: `scripts/generate-ai-corpus.sh <id> <genre> <prompt-file>
  [model]` generates an uncurated first-pass document via the claude CLI,
  stores the prompt under `eval/prompts/`, and prints the `[[document]]`
  snippet with the hash. Manual de-AI editing stays forbidden.

How external (non-committed) web documents join `suiko-eval` runs is decided
with the `calibrate` subcommand design (TODO.md); until then they are
measured ad hoc against `external-lock.json` hashes.

## Aozora Bunko preparation

`corpus/darakuron.txt` is based on the ruby-enabled text file linked from
Aozora Bunko card 42620. The following deterministic preparation was applied:

1. decode the source file from CP932 to UTF-8 and normalize CRLF to LF;
2. remove the Aozora notation guide and bibliographic footer;
3. remove ruby delimiters/readings, ruby range markers, and input annotations;
4. represent each source paragraph with a blank-line separator while retaining
   the work's wording and paragraph boundaries.

The local file hash in `corpus.toml` covers this prepared text. Full source,
credits, terms, and the preparation record are in `THIRD_PARTY_NOTICES.md`.

The 2015-06 text ranking was also reviewed for candidates. New-orthography
essays are suitable for the main human calibration set. Fiction is useful only
as a literary-style stress set, and old-orthography business prose must not be
mixed into thresholds intended for current Japanese technical writing.

## AI-generated documents

`corpus/ai-tech-001.md`, `corpus/ai-essay-001.md`, and
`corpus/ai-business-001.md` were generated by Claude (model
`claude-fable-5`) on 2026-08-18 specifically for this corpus. They are
uncurated first-pass outputs: no manual de-AI editing was applied, and no
detector output was used to steer the writing beyond a length requirement.
The generation requests were, in effect:

- ai-tech-001: "OpenTelemetryによる可観測性基盤の構築について、4,000字以上の
  技術ブログ記事を書いてください"
- ai-essay-001: "「待つこと」をテーマに、4,000字以上の随筆を書いてください"
- ai-business-001: "リモートワーク制度見直しの社内報告書を書いてください"

Length extensions were requested in follow-ups until the masked body exceeded
4,000 characters (the lexical-diversity gate). These documents are MIT-licensed
parts of this repository.

## Data statements

Following the spirit of Data Statements for NLP, each source records its
purpose, population, selection, era, genre, license, and known biases:

| source | purpose | population / selection | era | genre | license | known biases |
|---|---|---|---|---|---|---|
| bundled fixtures (`ai-smelly.md`, `natural.md`) | fast regression | hand-written contrast pair, not sampled | 2026 | essay | MIT | constructed, not natural distribution |
| `darakuron.txt` (Aozora Bunko) | long human essay for calibration | picked from the 2015-06 text ranking; new-orthography essay | 1946 | essay | public domain, Aozora handling standard | single author, mid-20th-century prose; no modern web/tech style |
| Claude-generated docs (`ai-*-001.md`) | AI stress slices ≥4,000 chars | single model (claude-fable-5), single prompt each, uncurated first pass | 2026-08-18 | tech / essay / business | MIT | one model, one date, no paraphrase or human-edit variants |
| `labeled/` samples | per-category behavior specs | authored to isolate one detector each | 2026-08-18 | mixed | MIT | specification fixtures, not population samples |
| `corpus/aozora/` (12 essays) | human essay volume for calibration | natural-japanese's Aozora selection (modern-colloquial classics) | 1920s–1940s | essay | public domain, Aozora handling standard | 4 authors only (寺田寅彦 6/12); pre-war vocabulary and punctuation norms differ from current writing — slice by era before adjusting vocabulary/style thresholds |
| `sources.toml` web entries (81, fetched not committed) | 2020s human articles for calibration | natural-japanese's curated selection; 17 domains, quality-tagged | 2018–2025 | tech / business / essay | per-source (mostly all-rights-reserved; local evaluation only) | platform skew (zenn.dev 23/81), government PDFs dominate business, one paid article truncated to its free part |

Known gaps: dev/holdout for web sources is assigned but holdout evaluation has
not run yet; no multi-model AI documents. A historical 2026-07 measurement
over 103 human + 81 AI documents predates this manifest, cannot be reproduced
(no manifest, hashes, or settings survive), and is kept in the skill
references only as annotated history, not as evidence.

## Labeled samples

`labeled/<category>/fire-*.md` must trigger their category; `silent-*.md` are
minimal contrasts that must stay quiet. Each `[[sample]]` records the reason in
`note`. A sample may reference a corpus document (for example, `堕落論` is a
`low_lexical_diversity_ttr` silent sample because its repetition is a human
rhetorical choice). Known mismatches are kept in the manifest on purpose: the
`labeled` report is the record of what the current thresholds do and do not
support. `labeled/low_lexical_diversity_ttr/fire-001.md` is a synthetic
fixture that cycles a small vocabulary; its generation script is recorded in
the git history and it is committed as a stable file.

## Reproduction

```sh
cargo run --features evaluation --bin suiko-eval -- report eval/corpus.toml
cargo run --features evaluation --bin suiko-eval -- labeled eval/corpus.toml
cargo run --features evaluation --bin suiko-eval -- sweep eval/corpus.toml --rule low-lexical-diversity-ttr --values 0.35,0.40,0.42,0.45,0.47,0.50
cargo run --features evaluation --bin suiko-eval -- sweep eval/corpus.toml --rule repeated-sentence-lead --values 3,5,7,9,11,13,15
cargo run --features evaluation --bin suiko-eval -- sweep eval/corpus.toml --rule low-specificity --values=-0.30,-0.25,-0.20,-0.15,-0.10,-0.05
cargo run --features evaluation --bin suiko-eval -- sweep eval/corpus.toml --rule nominal-ending --values 0.0,0.02,0.05,0.10
cargo run --features evaluation --bin suiko-eval -- sweep eval/corpus.toml --rule sentence-too-long --values 70,90,110,130
cargo run --features evaluation --bin suiko-eval -- length-analysis eval/corpus.toml
```
