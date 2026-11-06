# Quill fuzz targets

These developer-only libFuzzer targets exercise Quill's untrusted-input
boundaries without becoming part of the application or its packages.

Install the minimal nightly toolchain and `cargo-fuzz`, then run a target from
the repository root, for example:

```sh
cargo +nightly fuzz run docx
```

The targets cover ZIP containers, XML, DOCX, ODT, RTF, image identification and
decoding, settings, glossary files, and print-merge data.

## Overnight campaign

Run `python3 tools/overnight_fuzz.py` from the repository root under
`caffeinate -i` to seed the missing corpora, build all ten targets with
AddressSanitizer and checked arithmetic, and run each for three hours.
Three targets run concurrently with three workers apiece (about twelve hours
plus preparation). Original sample documents are only read.

Each campaign has an ignored `fuzz/runs/<timestamp>/` directory containing
`report.md` (updated every thirty seconds and finalized on completion),
`status.json`, build and per-target logs, exact commands, source snapshot,
and saved failure inputs. `fuzz/runs/latest.txt` identifies the latest run.
The report distinguishes incomplete runs from completed targets; saved failures
still need reproduction and minimization before they are treated as confirmed bugs.
