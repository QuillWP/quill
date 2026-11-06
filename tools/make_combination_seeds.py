#!/usr/bin/env python3
"""Prepare combined-feature RTF inputs for Word to save as fuzz fixtures.

This does not control Word or run fuzzing. Open the sources in Word, make
native edits as needed, and save DOCX copies in the sibling word directory.
"""

import base64
import argparse
import hashlib
import shutil
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "fuzz/word-fixtures/2026-10-31"
PNG = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="
)
HEADER = (
    r"{\rtf1\ansi\ansicpg1252\deff0\uc1"
    r"{\fonttbl{\f0 Times New Roman;}{\f1 Arial;}{\f2 Courier New;}}"
    r"{\colortbl;\red25\green65\blue130;\red235\green240\blue250;}"
    r"{\stylesheet{\s0\f0\fs22 Normal;}"
    r"{\s1\sbasedon0\snext0\outlinelevel0\b\f1\fs32 Heading 1;}"
    r"{\s2\sbasedon0\snext0\outlinelevel1\b\f1\fs26 Heading 2;}"
    r"{\s3\sbasedon0\snext0\b\f1\fs40 Title;}}"
    r"{\*\revtbl{Unknown;}{Test Author One;}{Test Author Two;}}"
    r"\paperw12240\paperh15840\margl1440\margr1440\margt1440\margb1440"
    r"\ftnbj\fet2\sectd\sbknone "
)


def text(value):
    out = []
    for char in value:
        if char in "\\{}":
            out.append("\\" + char)
        elif ord(char) < 128:
            out.append(char)
        else:
            encoded = char.encode("utf-16-le")
            for i in range(0, len(encoded), 2):
                n = int.from_bytes(encoded[i:i + 2], "little", signed=True)
                out.append(r"\u%d?" % n)
    return "".join(out)


def para(value, style=0):
    return r"\pard\plain\s%d " % style + value + r"\par "


def heading(value, level=1):
    return para(text(value), level)


def field(instruction, result):
    return r"{\field{\*\fldinst " + instruction + r"}{\fldrslt " + result + "}}"


def note(value):
    return r"{\super\chftn}{\footnote\pard\plain\fs18 {\super\chftn} " + text(value) + "}"


def picture():
    return r"{\pict\pngblip\picw1\pich1\picwgoal720\pichgoal480 " + PNG.hex() + "}"


def table(rows, level=1, width=8640, repeat_header=False):
    result = []
    for index, cells in enumerate(rows):
        props = r"\trowd\trgaph80" + (r"\trhdr" if repeat_header and index == 0 else "")
        for i in range(len(cells)):
            props += (r"\clbrdrt\brdrs\brdrw10\clbrdrb\brdrs\brdrw10"
                      r"\clbrdrl\brdrs\brdrw10\clbrdrr\brdrs\brdrw10"
                      + (r"\clcbpat2" if index % 2 == 0 else "")
                      + r"\cellx%d" % (width * (i + 1) // len(cells)))
        body = "".join(r"\pard\plain\intbl\itap%d " % level + cell
                       + (r"\nestcell " if level > 1 else r"\cell ") for cell in cells)
        if level == 1:
            result.append(props + body + r"\row ")
        else:
            result.append(body + r"{\*\nesttableprops" + props + r"\nestrow}{\nonesttables\par}")
    return "".join(result)


def running(title):
    return (r"{\header\pard\plain\f1\fs18 " + text(title) + r"\par}"
            r"{\footer\pard\plain\qc\fs18 Page " + field("PAGE", "1")
            + " of " + field("NUMPAGES", "1") + r"\par}")


def nested_packet():
    inner = table([[text("Inner image and note") + picture()
                    + note("Footnote anchored in the nested image cell."), text("Inner value 17")],
                   [text("Unicode café Ω and mixed ") + r"{\b bold}{\i italic}",
                    field('HYPERLINK "https://example.com/fixture"', "Fixture link")]], 2, 3600)
    rows = [[text("Section"), text("Combined content")],
            [text("Nested evidence"), text("Before inner table") + r"\par " + inner
             + r"\pard\plain\intbl\itap1 After inner table"],
            [picture() + text(" Image in an outer cell"), text("Outer reference")
             + note("A second footnote belongs to an outer cell.")]]
    rows += [[text(f"Record {i:02d}"), text(("Long wrapping cell with an ordinary sentence. " * (i % 4 + 1)))
              + (picture() if i % 7 == 0 else "")] for i in range(1, 29)]
    return HEADER + running("Nested editorial packet") + heading("Nested editorial packet", 3) + para(
        text("This fixture combines a repeating table header, nested tables, pictures, notes, links and mixed runs.")) + table(
        rows, repeat_header=True) + para(text("END NESTED PACKET")) + "}"


def section_report():
    body = [HEADER, running("Section and reference report"), heading("Section and reference report", 3),
            para(text("This fixture combines section transitions, styles, fields, notes, tables and pictures.")),
            field(r'TOC \\o "1-2" \\h', "Contents pending update"), heading("Portrait introduction"),
            para(text("Introductory prose ") + note("A portrait-section footnote.") + picture()),
            r"\sect\sectd\sbkpage\lndscpsxn\pgwsxn15840\pghsxn12240 ",
            running("Landscape evidence section"), heading("Landscape evidence"),
            table([[text("Metric"), text("Picture and explanation"), text("Value")],
                   [text("Alpha"), picture() + text(" Landscape cell picture") + note("Landscape table note."), text("12.50")],
                   [text("Beta"), text("Mixed ") + r"{\b bold }{\i italic }{\super 2}", text("0.00")]], width=11000, repeat_header=True),
            r"\pard\plain\par\sect\sectd\sbkpage\cols2\colsx360\pgwsxn12240\pghsxn15840 ",
            running("Two column conclusion"), heading("Two column conclusion")]
    for i in range(12):
        body.append(heading(f"Finding {i + 1}", 2))
        body.append(para(text("Column text with wrapping and accents café naïve. " * 4)
                         + (note("A note after a column transition.") if i == 4 else "")))
    return "".join(body) + para(text("END SECTION REPORT")) + "}"


def review_matrix():
    annotation = (r"{\*\atrfstart 1}review anchor{\*\atrfend 1}"
                  r"{\*\atnid TA}{\*\atnauthor Test Author One}\chatn"
                  r"{\*\annotation{\*\atnref 1}\pard\plain\fs18 Check this combined fixture.}")
    deep = table([[picture() + text(" Level three image"), text("Deep cell")]], 3, 1500)
    middle = table([[text("Middle cell") + r"\par " + deep
                     + r"\pard\plain\intbl\itap2 Middle tail", text("Middle neighbor")]], 2, 3600)
    body = [HEADER, r"\revisions\revprop3 ", running("Review and nesting matrix"),
            heading("Review and nesting matrix", 3), para(text("This fixture combines review markup, three table depths, images, footnotes and field results.")),
            para(text("A paragraph with ") + r"{\revised\revauth1 inserted words} and {\deleted\revauthdel2 deleted words}. " + annotation),
            heading("Nested review evidence"),
            table([[text("Outer review cell") + r"\par " + middle
                    + r"\pard\plain\intbl\itap1 Outer tail", text("Neighbor with note") + note("Review matrix footnote.")],
                   [r"{\revised\revauth1 Added cell text}", picture() + text(" Another picture")]]),
            heading("Follow up tasks"), para(r"\bullet\tab " + text("Inspect notes after nested rows.")),
            para(r"1.\tab " + text("Keep this numbering beside the review material.")),
            para(text("A locked date ") + r"{\field\fldlock{\*\fldinst DATE}{\fldrslt 31 October 2026}}"),
            para(text("END REVIEW MATRIX"))]
    return "".join(body) + "}"


def seed_corpus():
    made = []
    for path in sorted((OUT / "word").glob("*.docx")):
        if path.name.startswith(("._", "~$")):
            continue
        with zipfile.ZipFile(path) as archive:
            if "word/document.xml" not in archive.namelist():
                raise SystemExit(f"Not a Word document: {path}")
        made.extend(copy_seed(path, target) for target in ["docx", "zip"])
    for path in sorted((OUT / "sources").glob("*.rtf")):
        if not path.name.startswith("._"):
            made.append(copy_seed(path, "rtf"))
    if not made:
        raise SystemExit("No fixtures found; create and save them in Word first")
    for path in made:
        print(path)


def copy_seed(source, target):
    digest = hashlib.sha256(source.read_bytes()).hexdigest()
    destination = ROOT / "fuzz/corpus" / target / (digest + source.suffix)
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.exists():
        if destination.read_bytes() != source.read_bytes():
            raise SystemExit(f"Seed name conflict: {destination}")
    else:
        shutil.copyfile(source, destination)
    return destination


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seed-corpus", action="store_true",
                        help="copy completed Word DOCX and source RTF fixtures into ignored corpora")
    args = parser.parse_args()
    if args.seed_corpus:
        seed_corpus()
        return
    for folder in ["sources", "word", "roundtrips", "checks"]:
        (OUT / folder).mkdir(parents=True, exist_ok=True)
    for name, build in [("nested-packet", nested_packet), ("section-report", section_report),
                        ("review-matrix", review_matrix)]:
        path = OUT / "sources" / (name + ".rtf")
        if path.exists():
            raise SystemExit(f"Refusing to overwrite {path}")
        path.write_text(build(), encoding="ascii")
        print(path)


if __name__ == "__main__":
    main()
