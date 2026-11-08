# What the regex crate costs

Edit > Advanced > Grep, Regex Search and Sed all use the `regex` crate, so
patterns mean the same on every system.

Measured on 2026-11-06 with a small test program built with Quill's release
profile (fat LTO, stripped, opt-level 3), against the same program without
the crate:

| regex features | Added to the binary |
|---|---|
| default (all of them) | about 1.0 MB |
| `std`, `unicode-perl`, `unicode-case` (what Quill uses) | about 430 KB |

In Quill itself the release binary went from 4,916,000 to 5,362,736 bytes
(+446,736, about 9%). The features are trimmed in `crates/quill-app/Cargo.toml`,
and that is the reason for each one: `unicode-perl` for `\w`, `\d`, `\s` and
`\b` on non-English text, and `unicode-case` for case-insensitive matching.
Without `unicode-perl`, `\w` would miss accented letters, so it stays (its
saving was not measured). `\p{L}`-style classes (`unicode-gencat`,
`unicode-script`) and the `perf` features are left out on purpose.

The crate version is frozen by `Cargo.lock` until someone runs
`cargo update -p regex`; check the size again when that happens. After adding
the crate, `tools/footprint.py --check` may report the binary over the 10%
growth threshold until the baseline is re-recorded.
