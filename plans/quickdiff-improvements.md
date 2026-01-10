Quickdiff improvements plan (detailed)

Scope: address all findings from the review, keeping behavior consistent and avoiding scope creep.

- [x] 1) Diff worker non-blocking
  - Switch request channel send to non-blocking (try_send) and drop/coalesce stale requests.
  - Ensure UI state handles skipped requests (pending_request_id, loading flag, error state).
  - Add a small unit test for the worker queue behavior (latest request wins).

- [ ] 2) PR operations off the UI thread
  - Add a PR worker with request/response messages (list PRs, load PR diff).
  - Update App to set pr_loading immediately and apply worker responses asynchronously.
  - Ensure UI stays responsive during gh operations and error state is surfaced.

- [ ] 3) File-level highlight caching (Lumen-style)
  - Add a FileHighlighter-like cache in quickdiff highlight module.
  - Compute highlights once per loaded file (on diff load) and reuse in render.
  - Use cached per-line spans so multi-line constructs highlight correctly.
  - Invalidate cache when file selection changes or diff reloads.

- [ ] 4) PR picker scroll correctness
  - Update pr_picker_scroll in pr_picker_next/pr_picker_prev to keep selection visible.
  - Mirror sidebar scroll logic for consistent UX.

- [ ] 5) CLI path validation
  - Replace RelPath::new with RelPath::try_new in comments CLI.
  - Return user-friendly errors on absolute/invalid paths.

- [ ] 6) Event loop responsiveness
  - Add an event queue and process all pending events per tick.
  - Coalesce consecutive scroll events to avoid excessive redraws/requests.

- [ ] 7) UI safety and correctness polish
  - Use char-safe truncation for sidebar paths.
  - Use saturating math for PR action overlay sizing on tiny terminals.

- [ ] 8) Tests for patch extraction
  - Add unit tests covering multi-hunk patches, rename headers, and empty hunks.

Notes
- Lumen patterns used: FileHighlighter (multi-line aware) and event coalescing.
- Windows-specific atomic rename issue is intentionally out of scope.
