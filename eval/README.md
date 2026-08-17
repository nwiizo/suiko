# Evaluation corpus

`corpus.toml` is the versioned input to `suiko-eval`. Each document records a
stable ID, local path, human/AI label, Suiko genre, provenance, usage terms,
and SHA-256. The command rejects unknown fields and hash mismatches.

The bundled corpus is deliberately small. It keeps fast regression fixtures
separate from a real, longer human essay. Category fire rates are calibration
proxies for comparing thresholds; they are not probabilities that a document
was written by AI.

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
