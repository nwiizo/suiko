# Dependency security snapshot

## 2026-08-26 review

The complete registry dependency set resolved by `Cargo.lock` was queried
against the OSV batch API using the `crates.io` ecosystem and exact package
versions. No vulnerability-class advisory matched the locked graph.

One RustSec informational advisory matched:

- `RUSTSEC-2025-0057`: `fxhash 0.2.1` is unmaintained. It is transitive through
  `scraper 0.24.0 -> selectors 0.31.0 -> fxhash 0.2.1`; the advisory lists no
  patched `fxhash` version and recommends `rustc-hash` to direct consumers.

This is a dated snapshot, not continuous assurance. Re-run an advisory scan
before production promotion and on every dependency update. Treat new
vulnerability advisories as release blockers; review informational and
unmaintained advisories for a viable transitive upgrade or documented
acceptance. `Cargo.lock` remains mandatory for CI and release builds.
