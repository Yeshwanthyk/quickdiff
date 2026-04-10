# Slop Scan Remediation Plan

## Metadata
- Created: 2026-04-10
- Owner: Codex + yesh
- Status: in_progress
- Scanner: `/tmp/slop-scan` (Rust-capable source CLI)
- Baseline findings: 115

## Constraints
- Commit after each completed remediation batch.
- Run full verification for each batch:
  - `cargo fmt --check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo test`

## Batch Plan

### Batch 1: correctness/safety findings
- [x] `rust.silenced-result`
- [x] `rust.unwrap-density`
- [x] `rust.unsafe-undocumented`
- [x] `structure.pass-through-wrappers`

### Batch 2: comment-quality findings
- [x] `comments.banner-dividers`
- [x] `comments.hedging-language`

### Batch 3: ownership/complexity structural findings
- [ ] `rust.clone-density`
- [ ] `rust.god-functions`

### Batch 4: API/style/structure findings
- [ ] `rust.visibility-discipline`
- [ ] `rust.string-over-borrow`
- [ ] `rust.over-derive`
- [ ] `rust.restating-comments`
- [ ] `rust.allow-proliferation`
- [ ] `structure.directory-fanout-hotspot`

## Execution Log
- [x] Batch 1 complete + committed + verified
- [ ] Batch 2 complete + committed + verified
- [ ] Batch 3 complete + committed + verified
- [ ] Batch 4 complete + committed + verified
- [ ] Final slop scan rerun and recorded
