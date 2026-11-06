#!/usr/bin/env python3
"""Run the overnight fuzzing campaign; retain logs and a live report."""
import concurrent.futures
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import struct
import subprocess
import threading
import time
import zipfile

ROOT = Path(__file__).resolve().parents[1]
TARGETS = ['docx', 'rtf', 'zip', 'odt', 'html', 'xml', 'images', 'settings', 'glossary', 'merge_data']
LIMIT = 262144

def now():
    return dt.datetime.now().astimezone().isoformat(timespec='seconds')

def seed():
    base = ROOT / 'fuzz/corpus'
    for target in TARGETS:
        (base / target).mkdir(parents=True, exist_ok=True)
    def add(target, data):
        if 0 < len(data) <= LIMIT:
            (base / target / hashlib.sha256(data).hexdigest()).write_bytes(data)
    samples = Path(os.environ.get('QUILL_TEST_CASES', ROOT / 'tests/cases'))
    if not samples.is_dir():
        raise RuntimeError(f'Missing sample directory: {samples}. Set QUILL_TEST_CASES to a folder of sample documents.')
    xml_archives = 0
    for folder, _, names in os.walk(samples):
        for name in names:
            if name.startswith('._'):
                continue
            path = Path(folder) / name
            ext = path.suffix.lower()
            try:
                if path.stat().st_size > LIMIT:
                    continue
                target = {'.odt':'odt', '.fodt':'odt', '.html':'html', '.htm':'html', '.png':'images', '.jpg':'images', '.jpeg':'images', '.bmp':'images', '.tif':'images', '.tiff':'images'}.get(ext)
                if target:
                    add(target, path.read_bytes())
                if ext == '.odt':
                    add('zip', path.read_bytes())
                if ext == '.docx':
                    with zipfile.ZipFile(path) as archive:
                        for item in archive.infolist():
                            wanted = (xml_archives < 400 and item.filename in ('word/document.xml', 'word/styles.xml')) or item.filename.startswith('word/media/')
                            if wanted and item.file_size <= LIMIT:
                                add('images' if item.filename.startswith('word/media/') else 'xml', archive.read(item))
                    xml_archives += 1
            except (OSError, ValueError, zipfile.BadZipFile, RuntimeError):
                continue
    # Valid tiny BMP and uncompressed grayscale TIFF, absent from the source collection.
    add('images', b'BM' + struct.pack('<IHHI', 58, 0, 0, 54) + struct.pack('<IiiHHIIiiII', 40, 1, 1, 1, 24, 0, 4, 0, 0, 0, 0) + b'\x00\x00\xff\x00')
    entries = [(256,4,1,1),(257,4,1,1),(258,3,1,8),(259,3,1,1),(262,3,1,1),(273,4,1,122),(278,4,1,1),(279,4,1,1),(277,3,1,1)]
    add('images', b'II*\x00' + struct.pack('<I',8) + struct.pack('<H',len(entries)) + b''.join(struct.pack('<HHII',*entry) for entry in entries) + struct.pack('<I',0) + b'\x80')
    return {target: sum(p.is_file() for p in (base / target).iterdir()) for target in TARGETS}

def main():
    run = ROOT / 'fuzz/runs' / dt.datetime.now().strftime('%Y%m%d-%H%M%S')
    run.mkdir(parents=True)
    (ROOT / 'fuzz/runs/latest.txt').write_text(str(run) + '\n')
    lock = threading.Lock()
    state = {'started':now(), 'phase':'Seeding', 'targets':{t:{'status':'queued'} for t in TARGETS}}
    def report():
        with lock:
            (run / 'status.json').write_text(json.dumps(state, indent=2) + '\n')
            lines = ['# Quill overnight fuzz report', '', f"Started: {state['started']}", f"Phase: {state['phase']}", '', 'Three hours per target; three concurrent targets, three workers each. AddressSanitizer, debug assertions and overflow checks enabled. Limits: 256 KiB input, 10 seconds per input, 1536 MiB per worker. Failures are retained while testing continues.', '', 'Source snapshot: source.diff and untracked-sources/ (captured before build). Original test documents are read only.', '', '| Target | State | Seconds | New failure files | Latest progress |', '|---|---|---:|---:|---|']
            for target, info in state['targets'].items():
                folder = run / target
                artifacts = sorted((folder / 'artifacts').glob('*')) if folder.exists() else []
                progress = ''
                log = folder / 'fuzzer.log'
                if log.exists():
                    with log.open('rb') as stream:
                        stream.seek(max(0, log.stat().st_size - 65536))
                        tail = stream.read().decode(errors='replace')
                    matches = re.findall(r'^#\d+.*$', tail, re.M)
                    progress = matches[-1] if matches else tail.strip().split('\n')[-1][:200]
                elapsed = round(info.get('elapsed', time.time()-info['epoch'] if 'epoch' in info else 0))
                lines.append(f"| {target} | {info['status']} | {elapsed} | {len(artifacts)} | {progress.replace('|', '/')} |")
                if artifacts:
                    lines.extend([''] + [f'- {target}: `{p.relative_to(run)}`' for p in artifacts] + [''])
            lines += ['', 'Logs and saved failures are in each target directory. A completed run with no saved failures is not proof of correctness. Failed or interrupted targets are not counted as completed. Reproduce and minimize saved failures before treating them as confirmed bugs.']
            if 'error' in state:
                lines += ['', 'Campaign error: ' + state['error']]
            temp = run / 'report.tmp'
            temp.write_text('\n'.join(lines) + '\n')
            temp.replace(run / 'report.md')
    report()
    try:
        state['seeds'] = seed()
        for target, count in state['seeds'].items():
            if count == 0:
                raise RuntimeError(f'Empty corpus: {target}')
        with (run / 'source.diff').open('w') as output:
            subprocess.run(['git','diff','HEAD','--binary'], cwd=ROOT, stdout=output, check=True)
        paths = subprocess.check_output(['git','ls-files','--others','--exclude-standard','-z'], cwd=ROOT).split(b'\0')
        import shutil
        for raw in paths:
            if raw:
                relative = Path(os.fsdecode(raw))
                dest = run / 'untracked-sources' / relative
                dest.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(ROOT / relative, dest)
        (run / 'commit.txt').write_bytes(subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT))
        state['phase'] = 'Building all targets'
        report()
        build_dir = run / 'build'
        with (run / 'build.log').open('w') as output:
            subprocess.run(['cargo','+nightly','fuzz','build','-a','-O','--target-dir',str(build_dir)], cwd=ROOT, stdout=output, stderr=subprocess.STDOUT, check=True)
        state['phase'] = 'Running'
        report()
        def exercise(target):
            folder = run / target
            artifacts = folder / 'artifacts'
            artifacts.mkdir(parents=True)
            binary = build_dir / 'aarch64-apple-darwin/release' / target
            args = [str(binary), str(ROOT / 'fuzz/corpus' / target), '-max_total_time=10800', '-fork=3', '-rss_limit_mb=1536', '-timeout=10', '-max_len=262144', '-ignore_crashes=1', '-ignore_timeouts=1', '-ignore_ooms=1', f'-artifact_prefix={artifacts}/']
            (folder / 'command.json').write_text(json.dumps(args, indent=2))
            info = state['targets'][target]
            info.update(status='running', started=now(), epoch=time.time())
            report()
            try:
                with (folder / 'fuzzer.log').open('w') as output:
                    process = subprocess.Popen(args, cwd=folder, stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
                    info['pid'] = process.pid
                    code = process.wait(timeout=11100)
                info.update(status='completed' if code == 0 else f'failed (exit {code})', exit_code=code)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
                info['status'] = 'failed (campaign time limit)'
            except Exception as exc:
                info['status'] = f'failed ({exc})'
            info.update(elapsed=time.time()-info['epoch'], finished=now())
            report()
        with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
            futures = [pool.submit(exercise, target) for target in TARGETS]
            while not all(f.done() for f in futures):
                time.sleep(30)
                report()
            for future in futures:
                future.result()
        state['phase'] = 'Finished' if all(i['status']=='completed' for i in state['targets'].values()) else 'Finished with failures or incomplete targets'
        state['finished'] = now()
    except Exception as exc:
        state.update(phase='Failed', error=str(exc), finished=now())
        raise
    finally:
        report()

if __name__ == '__main__':
    main()
