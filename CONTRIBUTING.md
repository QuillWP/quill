# Contributing

Quill is alpha software and bug reports are the most useful help.

## Reporting a bug

[Open an issue](https://github.com/QuillWP/quill/issues) and include:

- the Quill version (Quill > About) and your macOS version;
- what you did, what you expected and what happened;
- if a document is involved, its format, and a small copy that shows the
  problem if you can share one (never your only copy);
- after a crash, the crash log: `~/Library/Preferences/Quill Crash Log`
  (`Quill Debug Crash Log` for the debug build);
- after a freeze, which leaves no crash log: start Quill from a terminal with
  `quill --with-events`, make it freeze and send the end of `quill-events.log`
  (written in the folder you started it from; `--events-file=PATH` picks another). `--with-events=all`
  also records mouse moves, redraws and every wake of the event loop.

Check [KNOWN-ISSUES.md](KNOWN-ISSUES.md) first.

## Security problems

A document that crashes Quill or uses huge amounts of memory is a security
problem: see [SECURITY.md](SECURITY.md).

## Code

Pull requests are welcome for small, focused fixes. Please open an issue first
for anything larger or any new feature. Read `AGENTS.md` for the build and
check commands and the rules the code follows, and run:

    cargo fmt --all
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
