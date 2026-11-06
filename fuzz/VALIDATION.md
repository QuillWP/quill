# Save-integrity and editing fuzz validation, 2026-10-24

This pass implements save-integrity properties and randomized core editing tests.
It does not start another overnight campaign.

## Changes

- DOCX and RTF fuzzing now fails when Quill cannot reopen its own exports.
- Native saves compare preserved document semantics against the imported model.
- The new editing target compares supported generated documents through native,
  DOCX and RTF saves. It checks text, formatting, stories, sections, comments,
  resolution state and revision records, including authorship.
- An independent String model checks plain edits. Snapshot histories check
  branching Undo/Redo; run normalization and UTF-8 boundaries are invariants.
- A discovered regression is fixed in both exchange readers: a tracked formatting
  record must survive even when its previous and current appearance match.

## Verification

| Check | Result |
|---|---|
| `cargo test --workspace --locked` | 506 passed, 3 ignored, no failures |
| Final focused property suite | 5 passed, including 128 seeds × 64 editing operations in each of the plain and tracked/comment models |
| Workspace Clippy, all targets, warnings denied | Passed |
| Separate fuzz-project Clippy, all targets, warnings denied | Passed |
| Nightly optimized fuzz build, debug assertions and AddressSanitizer | All 11 targets built |
| DOCX sanitizer trial, 121 seconds | 9,675 inputs, exit 0, no saved failures |
| RTF sanitizer trial, 121 seconds | 2,852 inputs, exit 0, no saved failures |
| Editing sanitizer trial, 121 seconds | 564 programs, exit 0, no saved failures |
| Final editing binary with additional history/run assertions | 256 programs, 36 seconds, exit 0, no saved failures |
| Python runner syntax and whitespace checks | Passed |

The parser trials used 128 representative seed files per format, each at most
256 KiB, and mutated them. Earlier attempts against the entire existing corpora
were stopped during initialization after exceeding the intended short-test budget;
those incomplete attempts are not counted as passing trials. These are bounded
pre-commit checks, not a new full-corpus or overnight certification.

## Explicit coverage boundaries

- Arbitrary imported exchange features are not promised lossless. Full DOCX/RTF
  semantic assertions use generated documents restricted to supported features.
- Internal comment highlights can be discontinuous after editing. Current exchange
  export uses one first-to-last range; the exchange oracle models that limitation.
  Native saves still compare the internal text ranges.
- Deleted comments retain metadata for Undo. Only live comment metadata represents
  saved content; renumberable note/comment IDs are compared by meaning.
- Comment resolution is outside text Undo in the current implementation. It ends
  the reference model's history segment; its saved state is still checked.
- The editing harness tests core document operations. Mouse hit testing, caret
  navigation, rendering and operating-system integration require separate UI tests.

The ordinary property tests also deliberately remove text, a revision record and
comment content to verify that the semantic oracle rejects silent loss.
