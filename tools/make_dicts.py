#!/usr/bin/env python3
"""Build Quill's language data from the LibreOffice English dictionaries.

Usage: python3 tools/make_dicts.py <source dir> <output dir>

The source dir holds en_US.dic, en_US.aff, hyph_en_US.dic and
th_en_US_v2.dat from https://github.com/LibreOffice/dictionaries (en/).
Writes two zlib-compressed text files that Quill loads on first use:

- hyph_en_US.pat: Liang hyphenation patterns, one per line, with the
  LEFTHYPHENMIN and RIGHTHYPHENMIN settings on the first line.
- th_en_US.dat: the thesaurus, one headword per line:
  word<TAB>pos|meaning|synonym|synonym<TAB>pos|meaning|...
  Pruned to words in the spelling list and a few meanings and synonyms
  each, to keep it small.
"""

import re
import sys
import zlib
from pathlib import Path


def parse_aff(path):
    rules = {}
    lines = path.read_text(encoding="utf-8").splitlines()
    i = 0
    while i < len(lines):
        parts = lines[i].split()
        i += 1
        if len(parts) >= 4 and parts[0] in ("PFX", "SFX") and parts[2] in ("Y", "N"):
            kind, flag, cross, count = parts[0], parts[1], parts[2] == "Y", int(parts[3])
            entries = []
            for _ in range(count):
                p = lines[i].split()
                i += 1
                strip = "" if p[2] == "0" else p[2]
                add = "" if p[3] == "0" else p[3].split("/")[0]
                cond = p[4] if len(p) > 4 else "."
                pat = re.compile(("^" + cond) if kind == "PFX" else (cond + "$"))
                entries.append((strip, add, pat))
            rules[flag] = (kind, cross, entries)
    return rules


def apply(word, rule):
    kind, _, entries = rule
    out = []
    for strip, add, pat in entries:
        if not pat.search(word):
            continue
        if kind == "SFX":
            if strip and not word.endswith(strip):
                continue
            base = word[: len(word) - len(strip)] if strip else word
            out.append(base + add)
        else:
            if strip and not word.startswith(strip):
                continue
            out.append(add + word[len(strip):])
    return out


def expand(dic, aff):
    rules = parse_aff(aff)
    words = set()
    for line in dic.read_text(encoding="utf-8").splitlines()[1:]:
        word, _, flags = line.partition("/")
        word = word.strip()
        if not word or any(c.isdigit() for c in word):
            continue
        flags = flags.strip()
        words.add(word)
        sfx = [rules[f] for f in flags if f in rules and rules[f][0] == "SFX"]
        pfx = [rules[f] for f in flags if f in rules and rules[f][0] == "PFX"]
        suffixed = []
        for r in sfx:
            for w in apply(word, r):
                words.add(w)
                if r[1]:
                    suffixed.append(w)
        for r in pfx:
            for w in apply(word, r):
                words.add(w)
            if r[1]:
                for s in suffixed:
                    for w in apply(s, r):
                        words.add(w)
    return words


def hyphenation(path):
    lines = path.read_text(encoding="utf-8").splitlines()
    left, right, pats = 2, 3, []
    for line in lines[1:]:
        line = line.strip()
        if not line or line.startswith("%"):
            continue
        if line.startswith("LEFTHYPHENMIN"):
            left = int(line.split()[1])
        elif line.startswith("RIGHTHYPHENMIN"):
            right = int(line.split()[1])
        elif line[0].isupper():
            continue
        else:
            pats.append(line)
    return f"{left} {right}\n" + "\n".join(pats) + "\n"


def thesaurus(path, known, meanings=8, synonyms=15):
    lines = path.read_text(encoding="utf-8").splitlines()
    out = []
    i = 1
    note = re.compile(r"\s*\((generic|related|similar|antonym)[^)]*\)\s*$")
    # Plain synonyms first, then WordNet's similar, related and broader
    # terms; antonyms go in a group of their own, as Word's Antonyms.
    order = {None: 0, "similar": 1, "related": 2, "generic": 3}
    while i < len(lines):
        head, _, count = lines[i].rpartition("|")
        i += 1
        n = int(count)
        groups = lines[i : i + n]
        i += n
        if " " in head or head.lower() not in known:
            continue
        kept, antonyms = [], []
        for g in groups:
            parts = g.split("|")
            pos = parts[0].strip("()")
            syns = []
            for s in parts[1:]:
                m = note.search(s)
                kind = m.group(1) if m else None
                plain = note.sub("", s)
                if plain.lower() == head.lower():
                    continue
                if kind == "antonym":
                    if plain not in antonyms:
                        antonyms.append(plain)
                elif all(plain != x for _, x in syns):
                    syns.append((order[kind], plain))
            syns.sort(key=lambda x: x[0])
            if syns and len(kept) < meanings:
                kept.append("|".join([pos] + [x for _, x in syns[:synonyms]]))
        if antonyms:
            kept.append("|".join(["antonym"] + antonyms[:synonyms]))
        if kept:
            out.append(head + "\t" + "\t".join(kept))
    out.sort(key=lambda l: l.split("\t")[0].lower())
    return "\n".join(out) + "\n"


def write(path, text):
    data = zlib.compress(text.encode("utf-8"), 9)
    path.write_bytes(data)
    print(f"{path.name}: {len(text.encode('utf-8'))} bytes, {len(data)} compressed")


def main():
    src, dst = Path(sys.argv[1]), Path(sys.argv[2])
    dst.mkdir(parents=True, exist_ok=True)
    words = expand(src / "en_US.dic", src / "en_US.aff")
    write(dst / "hyph_en_US.pat", hyphenation(src / "hyph_en_US.dic"))
    known = {w.lower() for w in words}
    write(dst / "th_en_US.dat", thesaurus(src / "th_en_US_v2.dat", known))


if __name__ == "__main__":
    main()
