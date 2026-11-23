#!/usr/bin/env python3
"""Quill's overnight tests: the gate, then every test document opened.

    caffeinate -i python3 tools/overnight.py --until 06:00

The gate runs the formatting check, clippy and the whole test suite, builds
the debug app and smoke tests it. Then every document in the test-documents
folder beside the repository (or --corpus) is opened the way the app opens
it, by the release build, several at a time, each with a time and a memory
limit, and a .qwp save of it is opened again. Every 20th document that opened,
and every one that failed last time, is opened again by the debug build,
which stops on arithmetic overflow.

Each run keeps tests/overnight/runs/<timestamp>/: report.md (rewritten every
thirty seconds, so it shows how far the night got), results.tsv and
debug-results.tsv (one line per document), the gate's logs, and copies of
the documents that failed, never the originals. The report starts with what
got worse since the previous run; tests/overnight/runs/latest.txt names the
newest run. Each failure not already listed is added, as an open item, to
overnight-test-fails.md in the folder above the repository (beside todo.md),
so failures are kept apart from the to-do list.

While it runs, `--pause` lets the work in hand finish and then waits,
`--resume` carries on, `--stop` ends the run cleanly (report and failures
written) and `--status` says where it is. The document checkers run at low
priority, so the Mac stays usable during a run.

With QUILL_OVERNIGHT_MAIL set to an address (put it in the launchd agent,
never in the repository), a message goes out when a run starts and when it
ends, each saying how to take the run off the schedule. tools/overnight_mail.py
sends them over SMTP and does nothing else; it says how to keep its password
in the keychain. --mail-test sends one now.

Documents are only read. The app built and smoke tested is the debug build
beside the source, which runs outside the App Sandbox, offscreen and with a
scratch HOME (see tools/smoketest.py), so the run is safe while Quill is open.
Quit Quill Debug first, or the gate skips rebuilding it.

Python 3.11+, standard library only.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
from pathlib import Path
import queue
import shutil
import signal
import subprocess
import sys
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
RUNS = ROOT / "tests/overnight/runs"
CORPUS = Path(os.environ.get("QUILL_TEST_DOCUMENTS", ROOT.parent / "test-documents"))
# Opened every night, all of them.
EXTENSIONS = {"docx", "docm", "dotx", "dotm", "rtf", "odt", "ott", "odm", "fodt",
              "html", "htm", "md", "markdown", "qwp"}
# Thousands of .txt files: every 20th only, unless --all. (.doc is not a
# format Quill opens, so those are left out.)
SAMPLED = {"txt", "text"}
SAMPLE_EVERY = 20
COLUMNS = ["outcome", "ms", "paras", "reopens", "notes", "detail", "path"]
FAILED = {"panic", "timeout", "memory", "crash"}


def now() -> str:
    return dt.datetime.now().astimezone().isoformat(timespec="seconds")


FAILS = ROOT.parent / "overnight-test-fails.md"
AGENT = Path.home() / "Library/LaunchAgents/com.quillwp.overnight.plist"

UNSCHEDULE = f"""To take the overnight run off the schedule, run these in Terminal:

    launchctl unload {AGENT}
    rm {AGENT}

To stop only tonight's run: python3 {ROOT}/tools/overnight.py --stop"""

def mail(title: str, body: str) -> str | None:
    """Send `body` to QUILL_OVERNIGHT_MAIL, with the reminder of how to take
    the run off the schedule. Sending is all tools/overnight_mail.py does.
    None when sent or when no address is set; otherwise why not."""
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import overnight_mail
    if not overnight_mail.address():
        return None
    return overnight_mail.send(title, body.rstrip() + "\n\n" + UNSCHEDULE + "\n")


def commit() -> str:
    p = subprocess.run(["git", "-C", str(ROOT), "log", "-1", "--format=%h %s"],
                       capture_output=True, text=True)
    dirty = subprocess.run(["git", "-C", str(ROOT), "status", "--porcelain", "--untracked-files=no"],
                           capture_output=True, text=True).stdout.strip()
    return p.stdout.strip() + (" (with uncommitted changes)" if dirty else "")

# A launchd agent for --print-schedule: every day at --start, until --until.
SCHEDULE = """<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>com.quillwp.overnight</string>
	<key>ProgramArguments</key>
	<array>
		<string>/usr/bin/caffeinate</string>
		<string>-i</string>
		<string>{python}</string>
		<string>{root}/tools/overnight.py</string>
		<string>--until</string>
		<string>{end}</string>
	</array>
	<key>WorkingDirectory</key>
	<string>{root}</string>
	<key>EnvironmentVariables</key>
	<dict>
		<key>PATH</key>
		<string>{home}/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
	</dict>
	<key>StartCalendarInterval</key>
	<dict>
		<key>Hour</key>
		<integer>0</integer>
		<key>Minute</key>
		<integer>0</integer>
	</dict>
	<key>StandardOutPath</key>
	<string>{log}</string>
	<key>StandardErrorPath</key>
	<string>{log}</string>
</dict>
</plist>"""


def descendants(pid: int) -> list[int]:
    """Every process started under `pid`, however deep."""
    out = subprocess.run(["ps", "-A", "-o", "pid=,ppid="], capture_output=True, text=True).stdout
    children: dict[int, list[int]] = {}
    for line in out.splitlines():
        try:
            child, parent = (int(x) for x in line.split())
        except ValueError:
            continue
        children.setdefault(parent, []).append(child)
    found, todo = [], [pid]
    while todo:
        for child in children.get(todo.pop(), []):
            found.append(child)
            todo.append(child)
    return found


def signal_tree(pid: int, sig: int, include_root: bool = True):
    for p in ([pid] if include_root else []) + descendants(pid):
        try:
            os.kill(p, sig)
        except OSError:
            pass


def control_file(run: Path) -> Path:
    return run / "control"


def latest_run() -> Path | None:
    latest = RUNS / "latest.txt"
    return Path(latest.read_text().strip()) if latest.is_file() else None


def wanted(run: Path) -> str:
    """What the person asked the run to do: "run", "pause" or "stop"."""
    try:
        return control_file(run).read_text().strip() or "run"
    except OSError:
        return "run"


def until(clock: str) -> float:
    """Seconds from now to the next time the clock shows `clock` (HH:MM)."""
    hour, minute = (int(x) for x in clock.split(":"))
    current = dt.datetime.now()
    end = current.replace(hour=hour, minute=minute, second=0, microsecond=0)
    if end <= current:
        end += dt.timedelta(days=1)
    return (end - current).total_seconds()


def record_failures(run: Path, rows: list[tuple[str, dict, str]]):
    """Add each failure not already listed to overnight-test-fails.md, as an
    open item with what it was, where its copy is and how to reproduce it."""
    known = FAILS.read_text(encoding="utf-8") if FAILS.is_file() else ""
    new = [(rel, r, build) for rel, r, build in rows if f"`{rel}`" not in known]
    if not new:
        return 0
    if not known:
        known = ("# Overnight test failures\n\n"
                 "Written by quill-review/tools/overnight.py: each failure the first night it is\n"
                 "seen. Mark one [done] with what fixed it, as in todo.md.\n")
    lines = [f"\n## {dt.date.today().isoformat()} (run {run.name})\n"]
    for rel, r, build in new:
        lines.append(f"- [ ] `{rel}`: {r['outcome']} in the {build} build. {r.get('detail', '')[:300]}\n"
                     f"  Reproduce: `cargo run {'--release ' if build == 'release' else ''}-p quill-formats "
                     f"--example open_check -- <copy in {run.name}/failures/>`\n")
    FAILS.write_text(known.rstrip("\n") + "\n" + "".join(lines), encoding="utf-8")
    return len(new)


def documents(corpus: Path, everything: bool) -> list[str]:
    """Every document under `corpus` worth opening, as paths relative to it,
    sorted so a night and the next see the same list in the same order."""
    full, sampled = [], []
    for folder, dirs, names in os.walk(corpus):
        dirs[:] = sorted(d for d in dirs if not d.startswith("."))
        for name in names:
            if name.startswith((".", "~$")):
                continue
            ext = name.rsplit(".", 1)[-1].lower() if "." in name else ""
            rel = str((Path(folder) / name).relative_to(corpus))
            if ext in EXTENSIONS:
                full.append(rel)
            elif ext in SAMPLED:
                sampled.append(rel)
    full.sort()
    sampled.sort()
    if not everything:
        sampled = sampled[::SAMPLE_EVERY]
    return sorted(full + sampled)


def rss_kb(pid: int) -> int:
    """The process's resident memory in KB, or 0 if it cannot be read."""
    out = subprocess.run(["ps", "-o", "rss=", "-p", str(pid)], capture_output=True, text=True)
    try:
        return int(out.stdout.strip() or 0)
    except ValueError:
        return 0


class Checker:
    """One open_check process, asked about one document at a time. A hang,
    a crash or too much memory costs that document only: the process is
    ended and the next document gets a new one."""

    def __init__(self, program: Path, timeout: float, memory_mb: int):
        self.program = program
        self.timeout = timeout
        self.memory_kb = memory_mb * 1024
        self.proc = None
        self.lines: queue.Queue = queue.Queue()

    def _start(self):
        # Low priority, so someone using the Mac during a run barely notices.
        self.proc = subprocess.Popen(["nice", "-n", "15", str(self.program)], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                     text=True, bufsize=1)
        self.lines = queue.Queue()
        out, lines = self.proc.stdout, self.lines

        def read():
            for line in out:
                lines.put(line.rstrip("\n"))
            lines.put(None)
        threading.Thread(target=read, daemon=True).start()

    def stop(self):
        if self.proc and self.proc.poll() is None:
            self.proc.kill()
            self.proc.wait()
        self.proc = None

    def check(self, path: Path) -> dict:
        if self.proc is None or self.proc.poll() is not None:
            self._start()
        started = time.monotonic()
        try:
            self.proc.stdin.write(str(path) + "\n")
            self.proc.stdin.flush()
        except OSError:
            self.stop()
            return {"outcome": "crash", "ms": "0", "detail": "the checker had ended"}
        peak = 0
        while True:
            try:
                line = self.lines.get(timeout=1)
            except queue.Empty:
                line = ""
            if line is None:
                code = self.proc.wait()
                self.proc = None
                return {"outcome": "crash", "ms": str(int((time.monotonic() - started) * 1000)),
                        "detail": f"the checker ended with {code}"}
            if line.startswith("done\t"):
                parts = line.split("\t")
                return dict(zip(COLUMNS, parts[1:8]))
            elapsed = time.monotonic() - started
            if elapsed > self.timeout:
                self.stop()
                return {"outcome": "timeout", "ms": str(int(elapsed * 1000)),
                        "detail": f"no answer in {int(self.timeout)} s"}
            peak = max(peak, rss_kb(self.proc.pid))
            if peak > self.memory_kb:
                self.stop()
                return {"outcome": "memory", "ms": str(int(elapsed * 1000)),
                        "detail": f"over {self.memory_kb // 1024} MB"}


def read_results(path: Path) -> dict[str, dict]:
    if not path.is_file():
        return {}
    out = {}
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    for line in lines[1:]:
        parts = line.split("\t")
        if len(parts) == len(COLUMNS):
            row = dict(zip(COLUMNS, parts))
            out[row["path"]] = row
    return out


def write_results(path: Path, rows: dict[str, dict]):
    temp = path.with_suffix(".tmp")
    with temp.open("w", encoding="utf-8") as f:
        f.write("\t".join(COLUMNS) + "\n")
        for rel in sorted(rows):
            row = rows[rel]
            f.write("\t".join(str(row.get(c, "")).replace("\t", " ") for c in COLUMNS) + "\n")
    temp.replace(path)


def compare(before: dict[str, dict], after: dict[str, dict]) -> tuple[list, list, list]:
    """What got worse, what got better and which notes changed, for the
    documents both runs opened."""
    worse, better, notes = [], [], []
    for rel, now_row in after.items():
        old = before.get(rel)
        if not old:
            continue
        o, n = old["outcome"], now_row["outcome"]
        if o == "ok" and n != "ok":
            worse.append((rel, f"opened before, now {n}: {now_row.get('detail', '')}"))
        elif o == "refused" and n in FAILED:
            worse.append((rel, f"was refused cleanly, now {n}: {now_row.get('detail', '')}"))
        elif o != "ok" and n == "ok":
            better.append((rel, f"was {o}, now opens"))
        elif o in FAILED and n == "refused":
            better.append((rel, f"was {o}, now refused cleanly"))
        if o == "ok" and n == "ok":
            if old.get("reopens") == "yes" and now_row.get("reopens") == "no":
                worse.append((rel, "its .qwp save no longer opens"))
            try:
                was, takes = int(old["ms"]), int(now_row["ms"])
            except (KeyError, ValueError):
                was = takes = 0
            if takes > 2 * was and takes - was > 200:
                worse.append((rel, f"twice as slow: {was} ms, now {takes} ms"))
            if old.get("notes") != now_row.get("notes"):
                notes.append((rel, f"{old.get('notes') or '(none)'} -> {now_row.get('notes') or '(none)'}"))
    return worse, better, notes


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--hours", type=float, default=8, help="stop starting new work after this long (default 8)")
    parser.add_argument("--until", metavar="HH:MM", help="stop starting new work at this time instead, such as 06:00")
    parser.add_argument("--print-schedule", action="store_true",
                        help="print a launchd agent that starts a run at --start and runs until --until")
    parser.add_argument("--start", metavar="HH:MM", default="00:00", help="for --print-schedule (default 00:00)")
    parser.add_argument("--mail", metavar="ADDRESS", help="for --print-schedule: send start and end messages here")
    parser.add_argument("--mail-test", action="store_true",
                        help="send a test message to QUILL_OVERNIGHT_MAIL, and stop")
    for verb, words in [("pause", "let the latest run finish the work in hand, then wait"),
                        ("resume", "let a paused run carry on"),
                        ("stop", "end the latest run cleanly, with its report"),
                        ("status", "say where the latest run is")]:
        parser.add_argument(f"--{verb}", action="store_true", help=words)
    parser.add_argument("--corpus", type=Path, default=CORPUS, help="the folder of test documents")
    parser.add_argument("--all", action="store_true", help="open every .txt, not every 20th")
    parser.add_argument("--limit", type=int, help="open only the first N documents (to try the runner)")
    parser.add_argument("--skip-gate", action="store_true", help="leave out the gate")
    parser.add_argument("--workers", type=int, default=max(2, (os.cpu_count() or 4) // 2))
    parser.add_argument("--timeout", type=float, default=60, help="seconds per document, release build")
    parser.add_argument("--debug-timeout", type=float, default=900, help="seconds per document, debug build")
    parser.add_argument("--memory-mb", type=int, default=2048, help="memory per document")
    parser.add_argument("--no-fuzz", action="store_true",
                        help="leave out the fuzzing that fills the rest of the night")
    args = parser.parse_args()

    if args.pause or args.resume or args.stop or args.status:
        run = latest_run()
        if not run or not run.is_dir():
            print("overnight.py: there is no run.")
            return 1
        if not args.status:
            control_file(run).write_text("pause\n" if args.pause else "stop\n" if args.stop else "run\n")
        try:
            st = json.loads((run / "status.json").read_text())
        except (OSError, ValueError):
            st = {}
        print(f"{run.name}: {st.get('phase', '?')}; asked to {wanted(run)}; "
              f"{st.get('release_done', 0)} of {st.get('documents', 0)} documents opened, "
              f"debug sample {st.get('debug_done', 0)} of {st.get('debug_planned', 0)}. "
              f"Report: {run / 'report.md'}")
        return 0
    if args.mail_test:
        if not os.environ.get("QUILL_OVERNIGHT_MAIL"):
            parser.error("set QUILL_OVERNIGHT_MAIL to the address first")
        why = mail("Quill overnight: test message",
                   "This is a test of the overnight run's messages. Each night you will get one when "
                   "the run starts and one when it ends.")
        print("sent" if why is None else f"not sent: {why}")
        return 0 if why is None else 1
    if args.print_schedule:
        hour, minute = (int(x) for x in args.start.split(":"))
        end = args.until or "06:00"
        text = SCHEDULE.format(root=ROOT, python=sys.executable, log=RUNS / "launchd.log",
                               home=Path.home(), end=end)
        text = text.replace("<key>Hour</key>\n\t\t<integer>0</integer>",
                            f"<key>Hour</key>\n\t\t<integer>{hour}</integer>")
        text = text.replace("<key>Minute</key>\n\t\t<integer>0</integer>",
                            f"<key>Minute</key>\n\t\t<integer>{minute}</integer>")
        if args.mail:
            text = text.replace("\t\t<key>PATH</key>",
                                f"\t\t<key>QUILL_OVERNIGHT_MAIL</key>\n\t\t<string>{args.mail}</string>\n"
                                "\t\t<key>PATH</key>")
        print(text)
        return 0
    try:
        seconds = until(args.until) if args.until else args.hours * 3600
    except ValueError:
        parser.error("--until takes a time such as 06:00")
    if args.until and seconds > 13 * 3600:
        # launchd starts a job it missed while the Mac slept as soon as it
        # wakes, which may be the next morning. That is not a night.
        print(f"overnight.py: {args.until} is more than 13 hours away; not starting a run now.")
        return 0
    deadline = time.monotonic() + seconds
    corpus = args.corpus.resolve()
    if not corpus.is_dir():
        parser.error(f"no test documents at {corpus}; pass --corpus")
    previous = None
    latest = RUNS / "latest.txt"
    if latest.is_file():
        previous = Path(latest.read_text().strip())
    run = RUNS / dt.datetime.now().strftime("%Y%m%d-%H%M%S")
    (run / "logs").mkdir(parents=True)
    (run / "failures").mkdir()
    latest.write_text(str(run) + "\n")
    ends = (dt.datetime.now() + dt.timedelta(seconds=seconds)).strftime("%H:%M")
    mail_trouble = [mail(f"Quill overnight: starting ({run.name})",
                         f"The overnight run {run.name} is starting.\n\n"
                         f"Testing: {commit()}\n"
                         f"It starts no new work after {ends}.\n"
                         f"Report, as it goes: {run / 'report.md'}\n\n"
                         f"To pause tonight's run: python3 {ROOT}/tools/overnight.py --pause "
                         f"(--resume carries on).")]

    state = {"started": now(), "phase": "starting", "gate": [], "corpus": str(corpus),
             "previous": str(previous) if previous else None, "documents": 0,
             "release_done": 0, "debug_planned": 0, "debug_done": 0, "notes": []}
    release: dict[str, dict] = {}
    debug: dict[str, dict] = {}
    lock = threading.Lock()
    before = read_results(previous / "results.tsv") if previous else {}
    before_debug = read_results(previous / "debug-results.tsv") if previous else {}

    def report():
        with lock:
            rows = dict(release)
            drows = dict(debug)
            st = json.loads(json.dumps(state))
        worse, better, notes = compare(before, rows)
        dworse, _, _ = compare(before_debug, drows)
        failures = sorted((rel, r) for rel, r in {**rows, **drows}.items() if r["outcome"] in FAILED)
        lines = ["# Quill overnight report", "",
                 f"Started: {st['started']}  ", f"Phase: {st['phase']}{' (paused)' if st.get('paused') else ''}  ",
                 f"Test documents: {st['corpus']}  ",
                 f"Compared with: {st['previous'] or 'nothing (first run)'}", ""]
        lines += ["## Worse than last time", ""]
        lines += [f"- `{rel}`: {why}" for rel, why in worse + [(r, "(debug) " + w) for r, w in dworse]] or ["Nothing."]
        lines += ["", "## Failures", ""]
        if failures:
            lines += ["| Document | Outcome | Detail |", "|---|---|---|"]
            lines += [f"| `{rel}` | {r['outcome']} | {r.get('detail', '')[:160]} |" for rel, r in failures[:200]]
            lines += ["", "Copies are in failures/. To reproduce one: "
                      "`cargo run --release -p quill-formats --example open_check -- <file>` "
                      "(leave out --release for the debug build)."]
        else:
            lines += ["None."]
        lines += ["", "## Gate", ""]
        lines += [f"- {g['name']}: {g['result']} ({g['seconds']} s){' - ' + g['note'] if g.get('note') else ''}"
                  for g in st["gate"]] or ["Not run."]
        counts: dict[str, int] = {}
        for r in rows.values():
            counts[r["outcome"]] = counts.get(r["outcome"], 0) + 1
        dcounts: dict[str, int] = {}
        for r in drows.values():
            dcounts[r["outcome"]] = dcounts.get(r["outcome"], 0) + 1
        lines += ["", "## Opening every document", "",
                  f"Release build: {st['release_done']} of {st['documents']} documents. "
                  + ", ".join(f"{k} {v}" for k, v in sorted(counts.items())),
                  "",
                  f"Debug build (every {SAMPLE_EVERY}th that opened, and last time's failures): "
                  f"{st['debug_done']} of {st['debug_planned']}. "
                  + ", ".join(f"{k} {v}" for k, v in sorted(dcounts.items())),
                  ""]
        reopen_fail = [rel for rel, r in rows.items() if r.get("reopens") == "no"]
        if reopen_fail:
            lines += [f"Opened, but the .qwp save did not open again: {len(reopen_fail)}", ""]
            lines += [f"- `{rel}`" for rel in sorted(reopen_fail)[:50]] + [""]
        slow = sorted(((int(r["ms"]), rel) for rel, r in rows.items()
                       if r["outcome"] == "ok" and r.get("ms", "").isdigit()), reverse=True)[:15]
        if slow:
            lines += ["Slowest to open (release):", ""] + [f"- {ms} ms `{rel}`" for ms, rel in slow] + [""]
        lines += ["## Better than last time", ""]
        lines += [f"- `{rel}`: {why}" for rel, why in better[:100]] or ["Nothing."]
        if notes:
            lines += ["", f"## Import notes that changed ({len(notes)})", ""]
            lines += [f"- `{rel}`: {why}" for rel, why in notes[:100]]
        if st["notes"]:
            lines += ["", "## Notes", ""] + [f"- {n}" for n in st["notes"]]
        lines += ["", "A run with no failures does not prove Quill correct. The gate's logs are in logs/; "
                  "results.tsv and debug-results.tsv hold one line per document."]
        (run / "report.tmp").write_text("\n".join(lines) + "\n", encoding="utf-8")
        (run / "report.tmp").replace(run / "report.md")
        (run / "status.json").write_text(json.dumps(st, indent=1) + "\n")
        write_results(run / "results.tsv", rows)
        write_results(run / "debug-results.tsv", drows)

    stop_reporting = threading.Event()

    def keep_reporting():
        while not stop_reporting.wait(30):
            report()
    threading.Thread(target=keep_reporting, daemon=True).start()

    def phase(name):
        with lock:
            state["phase"] = name
        report()

    def step(name, command, timeout=3600, expect=None, cwd=ROOT):
        """Run one gate step, keeping its output in logs/."""
        if wanted(run) == "stop":
            return False
        while wanted(run) == "pause":
            time.sleep(5)
        started = time.monotonic()
        log = run / "logs" / f"{name.replace(' ', '-')}.log"
        try:
            p = subprocess.run(command, cwd=cwd, capture_output=True, text=True, timeout=timeout)
            output = p.stdout + p.stderr
            ok = p.returncode == 0 and (expect is None or expect in output)
            result = "passed" if ok else f"FAILED (exit {p.returncode})"
        except subprocess.TimeoutExpired as e:
            output = (e.stdout or "") + (e.stderr or "") if isinstance(e.stdout, str) else ""
            ok, result = False, f"FAILED (no end in {timeout} s)"
        log.write_text(" ".join(map(str, command)) + "\n\n" + output, encoding="utf-8")
        with lock:
            state["gate"].append({"name": name, "result": result,
                                  "seconds": int(time.monotonic() - started)})
        report()
        return ok

    def note(text):
        with lock:
            state["notes"].append(text)

    examples = ROOT / "target"
    release_checker = examples / "release/examples/open_check"
    debug_checker = examples / "debug/examples/open_check"

    if not args.skip_gate:
        phase("gate")
        step("format", ["cargo", "fmt", "--all", "--check"], 600)
        step("clippy", ["cargo", "clippy", "-p", "quill-core", "-p", "quill-layout", "-p", "quill-app",
                        "--", "-D", "warnings"], 1800)
        step("tests", ["cargo", "test", "--workspace"], 3600)
        step("winit patch", ["cargo", "run", "-q", "-p", "quill-app", "--example", "modal_events"], 600,
             expect="back in the handler without a crash")
        running = subprocess.run(["pgrep", "-f", "Quill Debug.app/Contents/MacOS/quill"],
                                 capture_output=True).returncode == 0
        if running:
            note("Quill Debug was running, so the debug app was not rebuilt or smoke tested.")
        elif step("debug app", [sys.executable, "tools/package.py", "--debug"], 1800):
            note("target/debug/Quill Debug.app was rebuilt from this source.")
            step("smoke test", [sys.executable, "tools/smoketest.py", "--app",
                                str(ROOT / "target/debug/Quill Debug.app")], 600)
    phase("building the document checker")
    built = (step("checker, release", ["cargo", "build", "--release", "-p", "quill-formats",
                                        "--example", "open_check"], 1800)
             and step("checker, debug", ["cargo", "build", "-p", "quill-formats",
                                         "--example", "open_check"], 1800))
    if not built:
        phase("stopped: the document checker did not build")
        stop_reporting.set()
        report()
        return 1

    docs = documents(corpus, args.all)
    if args.limit:
        docs = docs[:args.limit]
    with lock:
        state["documents"] = len(docs)

    def open_all(names, program, timeout, results, counter):
        pending: queue.Queue = queue.Queue()
        for rel in names:
            pending.put(rel)
        saved = [0]

        def work():
            checker = Checker(program, timeout, args.memory_mb)
            try:
                while time.monotonic() < deadline:
                    asked = wanted(run)
                    if asked == "stop":
                        return
                    if asked == "pause":
                        with lock:
                            state["paused"] = True
                        time.sleep(5)
                        continue
                    with lock:
                        state["paused"] = False
                    try:
                        rel = pending.get_nowait()
                    except queue.Empty:
                        return
                    row = checker.check(corpus / rel)
                    row["path"] = rel
                    for c in COLUMNS:
                        row.setdefault(c, "-" if c in ("paras", "reopens") else "")
                    with lock:
                        results[rel] = row
                        state[counter] += 1
                        keep = row["outcome"] in FAILED and saved[0] < 200
                        if keep:
                            saved[0] += 1
                            n = saved[0]
                    if keep:
                        try:
                            shutil.copy2(corpus / rel, run / "failures" / f"{n:03d}-{Path(rel).name}")
                        except OSError:
                            pass
            finally:
                checker.stop()
        threads = [threading.Thread(target=work) for _ in range(args.workers)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        left = pending.qsize()
        return left

    phase("opening every document (release)")
    left = open_all(docs, release_checker, args.timeout, release, "release_done")
    if left:
        note(f"The night ended before {left} documents were opened.")

    opened = sorted(rel for rel, r in release.items() if r["outcome"] == "ok")
    again = sorted(set(opened[::SAMPLE_EVERY])
                   | {rel for rel, r in {**before, **before_debug}.items() if r["outcome"] in FAILED}
                   | {rel for rel, r in release.items() if r["outcome"] in FAILED})
    again = [rel for rel in again if (corpus / rel).is_file()]
    with lock:
        state["debug_planned"] = len(again)
    phase("opening a sample (debug)")
    left = open_all(again, debug_checker, args.debug_timeout, debug, "debug_done")
    if left:
        note(f"The night ended before {left} documents of the debug sample were opened.")

    # Fuzzing fills what is left of the night: tools/overnight_fuzz.py with
    # the remaining hours, less time for building its targets. Pause and
    # stop reach every fuzzer process, which run in process groups of their
    # own. libFuzzer counts time by the clock, so a pause shortens fuzzing.
    left = deadline - time.monotonic()
    if not args.no_fuzz and left > 45 * 60 and wanted(run) != "stop":
        hours = max(0.25, (left - 25 * 60) / 3600)
        phase(f"fuzzing ({hours:.1f} hours)")
        env = dict(os.environ, QUILL_TEST_CASES=str(corpus))
        with (run / "logs" / "fuzz.log").open("w") as log:
            fuzz = subprocess.Popen([sys.executable, str(ROOT / "tools/overnight_fuzz.py"),
                                     "--hours", f"{hours:.2f}"], cwd=ROOT, env=env,
                                    stdout=log, stderr=subprocess.STDOUT)
            paused = False
            hard_end = deadline + 30 * 60
            while fuzz.poll() is None:
                time.sleep(5)
                asked = wanted(run)
                if asked == "pause" and not paused:
                    signal_tree(fuzz.pid, signal.SIGSTOP)
                    paused = True
                    with lock:
                        state["paused"] = True
                    report()
                elif asked == "run" and paused:
                    signal_tree(fuzz.pid, signal.SIGCONT)
                    paused = False
                    with lock:
                        state["paused"] = False
                    report()
                elif asked == "stop" or time.monotonic() > hard_end:
                    if paused:
                        signal_tree(fuzz.pid, signal.SIGCONT)
                    # libFuzzer ends cleanly on SIGINT; anything left after a
                    # minute is ended outright.
                    for _ in range(12):
                        signal_tree(fuzz.pid, signal.SIGINT, include_root=False)
                        try:
                            fuzz.wait(timeout=5)
                            break
                        except subprocess.TimeoutExpired:
                            pass
                    if fuzz.poll() is None:
                        signal_tree(fuzz.pid, signal.SIGKILL)
                        fuzz.wait()
                    break
        fuzz_run = (ROOT / "fuzz/runs/latest.txt")
        if fuzz_run.is_file():
            note(f"Fuzzing report: {fuzz_run.read_text().strip()}/report.md "
                 f"(the fuzzer exited with {fuzz.returncode}).")
    elif not args.no_fuzz:
        note("No fuzzing: less than 45 minutes of the night were left.")

    stop_reporting.set()
    found = [(rel, r, "release") for rel, r in sorted(release.items()) if r["outcome"] in FAILED]
    found += [(rel, r, "debug") for rel, r in sorted(debug.items()) if r["outcome"] in FAILED]
    added = record_failures(run, found)
    if added:
        note(f"{added} new failures were added to {FAILS.name}.")
    if mail_trouble[0]:
        note(f"The starting message was not sent: {mail_trouble[0]}")
    phase("stopped on request" if wanted(run) == "stop" else "finished")
    worse, _, _ = compare(before, release)
    failed = any(r["outcome"] in FAILED for r in {**release, **debug}.values())
    gate_failed = any(g["result"] != "passed" for g in state["gate"])
    failures = [rel for rel, r, _ in found]
    gate = [f"{g['name']}: {g['result']}" for g in state["gate"] if g["result"] != "passed"]
    verdict = ("needs a look" if worse or failed or gate_failed else "all clear")
    summary = [f"The overnight run {run.name} {state['phase']}: {verdict}.", "",
               f"Tested: {commit()}",
               f"Gate: {'passed' if not gate_failed else 'FAILED - ' + '; '.join(gate)}",
               f"Documents: {len(release)} opened in the release build "
               f"({sum(r['outcome'] == 'ok' for r in release.values())} opened, "
               f"{sum(r['outcome'] == 'refused' for r in release.values())} refused); "
               f"{len(debug)} in the debug build.",
               f"Worse than last time: {len(worse)}.",
               f"Failures: {len(failures)}" + (f", {added} new in {FAILS.name}" if added else "") + "."]
    summary += [f"  - {rel}: {why}" for rel, why in worse[:10]]
    summary += [f"  - {rel}" for rel in failures[:10]]
    summary += ["", f"Report: {run / 'report.md'}"]
    why = mail(f"Quill overnight: {verdict} ({run.name})", "\n".join(summary))
    if why:
        note(f"The ending message was not sent: {why}")
        report()
    print(f"Report: {run / 'report.md'}")
    return 1 if worse or failed or gate_failed else 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        raise SystemExit(130)
