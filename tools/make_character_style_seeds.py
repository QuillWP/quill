#!/usr/bin/env python3
"""Prepare RTF sources that combine character styles with everything they touch.

Each source mixes named character styles and direct overrides with links, notes,
comments, tracked changes, headers, nested tables or Word's clipboard habits.
Open them in Word and save the results in the sibling word folder, then run
`cargo run -p quill-formats --example seed_exports` to add Quill's own files, and
this script with --seed-corpus to copy everything into the ignored corpora.
"""

import argparse
import hashlib
import shutil
import zipfile
from pathlib import Path

from make_combination_seeds import PNG, text

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "fuzz/word-fixtures/2026-11-01"
LIMIT = 262144

HEADER = (
    r"{\rtf1\ansi\ansicpg1252\deff0\uc1"
    r"{\fonttbl{\f0 Times New Roman;}{\f1 Arial;}{\f2 Courier New;}{\f3 Georgia;}{\f4 Garamond;}}"
    r"{\colortbl;\red200\green30\blue30;\red5\green99\blue193;\red235\green240\blue250;}"
    r"{\stylesheet{\s0\f0\fs22\sa160 Normal;}"
    r"{\s1\sbasedon0\snext0\outlinelevel0\b\f1\fs32 Heading 1;}"
    r"{\s2\sbasedon0\snext0\outlinelevel1\i\f3\fs26 Heading 2;}"
    r"{\*\cs10\additive\ssemihidden Default Paragraph Font;}"
    r"{\*\cs11\additive\sbasedon10\i\cf1 Emphasis;}"
    r"{\*\cs12\additive\sbasedon11\b Strong Emphasis;}"
    r"{\*\cs13\additive\sbasedon10\ul\cf2 Hyperlink;}"
    r"{\*\cs14\additive\sbasedon10\f2\fs18 Code;}"
    r"{\*\cs15\additive\sbasedon10\scaps\expnd4 Label;}"
    r"{\*\cs16\additive\sbasedon10\f4\fs30 Missing Font;}"
    r"{\*\cs17\additive\sbasedon10\up6\fs16 Raised;}}"
    r"{\*\revtbl{Unknown;}{Test Author One;}{Test Author Two;}}"
    r"\paperw12240\paperh15840\margl1440\margr1440\margt1440\margb1440"
    r"\ftnbj\fet2\sectd\sbknone "
)
LOOKS = {
    11: r"\cs11\i\cf1 ",
    12: r"\cs12\i\b\cf1 ",
    13: r"\cs13\ul\cf2 ",
    14: r"\cs14\f2\fs18 ",
    15: r"\cs15\scaps\expnd4 ",
    16: r"\cs16\f4\fs30 ",
    17: r"\cs17\up6\fs16 ",
}


def styled(style, value, direct=""):
    return "{" + LOOKS[style] + direct + text(value) + "}"


def para(value, style=0):
    return r"\pard\plain\s%d " % style + value + r"\par "


def link(address, value):
    return r'{\field{\*\fldinst HYPERLINK "%s"}{\fldrslt %s}}' % (address, styled(13, value))


def picture():
    return (r"{\pict\pngblip\picw1\pich1\picwgoal720\pichgoal720 " + PNG.hex() + "}")


def footnote(value):
    return r"{\super\chftn}{\footnote\pard\plain\s0 {\super\chftn} " + value + "}"


def comment(n, target, value):
    return (r"{\*\atrfstart %d}" % n + target + r"{\*\atrfend %d}" % n
            + r"{\*\atnid TA}{\*\atnauthor Test Author One}\chatn"
            + r"{\*\annotation{\*\atnref %d}\pard\plain\s0 " % n + value + "}")


def every_style():
    out = [para(text("Every named style"), 1)]
    for style in LOOKS:
        out.append(para(text("Plain before ") + styled(style, "styled %d" % style) + text(" plain after.")))
    out.append(para(text("Direct overrides"), 2))
    out.append(para(styled(11, "Italic turned off", r"\i0 ") + text(" and ")
                    + styled(12, "color changed", r"\cf2 ") + text(" and ")
                    + styled(14, "code in Georgia", r"\f3\fs28 ")))
    out.append(para(text("Styles over a heading font"), 2))
    out.append(para(text("Heading with ") + styled(11, "emphasis") + text(" and ")
                    + styled(14, "code"), 1))
    out.append(para(styled(11, "A whole paragraph in one style")))
    out.append(para(styled(15, "Adjoining") + styled(11, "styles") + styled(14, "with") + styled(16, "no")
                    + styled(17, "gaps")))
    return "".join(out)


def links_notes_review():
    out = [r"{\header\pard\plain\s0 " + styled(11, "Header emphasis") + text(" and ")
           + link("https://example.com/header", "a header link") + r"\par}",
           r"{\footer\pard\plain\s0 " + styled(15, "Footer label") + r"\par}"]
    out.append(para(text("Links, notes and review"), 1))
    out.append(para(text("See ") + link("https://example.com/a", "the first link") + text(" and ")
                    + link("mailto:someone@example.com", "an address") + text(" with ")
                    + r'{\field{\*\fldinst HYPERLINK "https://example.com/b"}{\fldrslt {\cs13\ul\cf1 '
                    + text("a link in its own color") + "}}}."))
    out.append(para(text("A note on ") + styled(11, "styled text")
                    + footnote(styled(14, "note code") + text(" and ") + styled(11, "note emphasis")) + "."))
    out.append(para(text("A comment on ") + comment(1, styled(12, "strong words"),
                                                    styled(11, "comment emphasis")) + "."))
    out.append(para(r"{\revised\revauth1\revdttm1782857760 " + styled(11, "inserted emphasis") + "}"
                    + r"{\deleted\revauthdel2\revdttmdel1782857760 " + styled(14, "deleted code") + "}"
                    + r" kept."))
    out.append(para(r"{\crauth1\crdate1782857760 " + styled(15, "reformatted label") + "}"))
    return "".join(out)


def row(cells, widths, padded=False):
    head = r"\trowd\trgaph108\trleft-108"
    if padded:
        head += r"\trpaddl80\trpaddr80\trpaddfl3\trpaddfr3"
    edges = ""
    edge = -108
    for width in widths:
        edge += width
        edges += r"\clbrdrt\brdrs\brdrw8\clbrdrb\brdrs\brdrw8\clbrdrl\brdrs\brdrw8\clbrdrr\brdrs\brdrw8"
        edges += r"\clcbpat3\clshdrawnil\cellx%d" % edge
    return head + edges + "".join(r"\pard\plain\intbl\s0 " + c + r"\cell " for c in cells) + r"\row "


def tables():
    out = [para(text("Tables"), 1)]
    out.append(row([styled(11, "Emphasis in a cell"), styled(14, "code") + text(" plain"),
                    styled(15, "Label")], [3000, 3000, 3000], True))
    out.append(row([link("https://example.com/cell", "cell link"), picture(),
                    styled(12, "picture beside")], [3000, 3000, 3000], True))
    out.append(r"\trowd\trgaph108\cellx4500\cellx9000")
    out.append(r"\pard\plain\intbl\itap1\s0 " + styled(11, "Outer cell") + r"\cell ")
    out.append(r"\pard\plain\intbl\itap2\s0 " + styled(14, "inner code") + r"\nestcell ")
    out.append(r"\pard\plain\intbl\itap2\s0 " + styled(12, "inner strong") + r"\nestcell ")
    out.append(r"{\*\nesttableprops\trowd\trgaph60\cellx2000\cellx4000\nestrow}{\nonesttables\par}")
    out.append(r"\pard\plain\intbl\itap1\s0 \cell \row ")
    out.append(para(text("Text after the tables.")))
    return "".join(out)


def clipboard_habits():
    # The way Word fills the clipboard: inherited spacing left out, a missing
    # font, and a table with row and cell padding.
    out = [para(styled(16, "Pasted in a font this computer may lack") + text(" beside ")
                + styled(11, "installed fonts"))]
    out.append(row([text("Space below"), styled(14, "cell text")], [4500, 4500], True))
    out.append(r"\trowd\trgaph108\trpaddl108\trpaddr108\trpaddfl3\trpaddfr3"
               r"\clpadl40\clpadr200\clpadfl3\clpadfr3\cellx4500\cellx9000")
    out.append(r"\pard\plain\intbl\s0 " + text("Own padding") + r"\cell ")
    out.append(r"\pard\plain\intbl\s0\sa0 " + text("No space after") + r"\cell \row ")
    out.append(r"\pard\plain\s2 " + text("Style chosen after a table") + r"\par ")
    return "".join(out)


def scripts():
    lines = ["Greek αβγ", "Cyrillic Жук", "Japanese 日本語",
             "Emoji \U0001F980 and \U0001F680", "Accents café naïve", "Combining é"]
    out = [para(text("Other scripts"), 1)]
    for i, line in enumerate(lines):
        style = list(LOOKS)[i % len(LOOKS)]
        out.append(para(styled(style, line) + text(" then plain ") + styled(11, line, r"\i0 ")))
    return "".join(out)


def long_mix():
    out = []
    for i in range(240):
        style = list(LOOKS)[i % len(LOOKS)]
        words = "Paragraph %d with " % i
        out.append(para(text(words) + styled(style, "a styled phrase") + text(" and more words after it ")
                        + styled(list(LOOKS)[(i * 3) % len(LOOKS)], "plus another"), 1 if i % 40 == 0 else 0))
    return "".join(out)


SOURCES = {
    "cs-every-style": every_style,
    "cs-links-notes-review": links_notes_review,
    "cs-tables": tables,
    "cs-clipboard-habits": clipboard_habits,
    "cs-scripts": scripts,
    "cs-long-mix": long_mix,
}


def copy_seed(source, target):
    data = source.read_bytes()
    if not 0 < len(data) <= LIMIT:
        return None
    destination = ROOT / "fuzz/corpus" / target / (hashlib.sha256(data).hexdigest() + source.suffix)
    destination.parent.mkdir(parents=True, exist_ok=True)
    if not destination.exists():
        shutil.copyfile(source, destination)
    return destination


def seed_corpus():
    """Copy everything made so far, each file into the targets that read it."""
    wanted = {".docx": ["docx", "zip"], ".odt": ["odt", "zip"], ".rtf": ["rtf"],
              ".htm": ["html"], ".html": ["html"], ".qwp": ["native"]}
    clipboard = {".rtf": 0, ".htm": 1, ".html": 1}
    made = 0
    for path in sorted(OUT.rglob("*")):
        if path.name.startswith(("._", "~$")) or not path.is_file():
            continue
        for target in wanted.get(path.suffix.lower(), []):
            made += copy_seed(path, target) is not None
        if path.suffix.lower() in clipboard:
            # The target's first byte says RTF or HTML; 2 replaces missing fonts.
            for choice in (0, 2):
                data = bytes([clipboard[path.suffix.lower()] + choice]) + path.read_bytes()
                if len(data) <= LIMIT:
                    target = ROOT / "fuzz/corpus/clipboard" / hashlib.sha256(data).hexdigest()
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes(data)
                    made += 1
        if path.suffix.lower() == ".docx":
            with zipfile.ZipFile(path) as archive:
                for name in ("word/document.xml", "word/styles.xml"):
                    data = archive.read(name)
                    if len(data) <= LIMIT:
                        target = ROOT / "fuzz/corpus/xml" / hashlib.sha256(data).hexdigest()
                        target.write_bytes(data)
                        made += 1
    print(made, "seeds in place")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seed-corpus", action="store_true",
                        help="copy the finished files into the ignored corpora")
    args = parser.parse_args()
    if args.seed_corpus:
        seed_corpus()
        return
    for folder in ["sources", "word", "quill"]:
        (OUT / folder).mkdir(parents=True, exist_ok=True)
    for name, build in SOURCES.items():
        path = OUT / "sources" / (name + ".rtf")
        path.write_text(HEADER + build() + "}", encoding="ascii")
        print(path)


if __name__ == "__main__":
    main()
