#!/usr/bin/env python3
"""Prepare fuzz seeds for the features added since 2026-10-31.

The steps run in order, each with its own option, so a slow one can be redone
alone:

  --sources      RTF sources: border and shading colors, cropped and adjacent
                 pictures, bullets, line numbers by section, links and views,
                 and symbol fonts (Wingdings, Webdings, Symbol, Zapf Dingbats).
  --quill        Quill's own documents (cargo example feature_seeds): character
                 styles in every story, linked headings, contents lists, built
                 on these sources and the fixtures of 2026-10-31 and 2026-11-01.
  --word         Microsoft Word opens Quill's .docx, .rtf and .odt and the
                 sources, and saves .docx, .rtf, .odt and .htm into word/.
  --libreoffice  LibreOffice converts the same files into lo/.
  --return       Quill reads everything Word and LibreOffice wrote and saves
                 it again in every format, into return/.
  --seed-corpus  Copy every file into the ignored corpora of the targets
                 that read it.

With no option, all of them run. Everything goes in the ignored
fuzz/word-fixtures/2026-11-02 folder. Word must be installed, and the terminal
needs Accessibility permission for the helper that answers Word's dialogs.
"""

import argparse
import hashlib
import os
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

from make_combination_seeds import PNG, text

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "fuzz/word-fixtures/2026-11-02"
EARLIER = [ROOT / "fuzz/word-fixtures/2026-10-31/sources", ROOT / "fuzz/word-fixtures/2026-10-31/word",
           ROOT / "fuzz/word-fixtures/2026-11-01/sources", ROOT / "fuzz/word-fixtures/2026-11-01/word"]
# Set SOFFICE to the LibreOffice command line program if it is not on the PATH.
SOFFICE = Path(os.environ.get("SOFFICE") or shutil.which("soffice")
               or "/Applications/LibreOffice.app/Contents/MacOS/soffice")
LIMIT = 262144

HEADER = (
    r"{\rtf1\ansi\ansicpg1252\deff0\uc1"
    r"{\fonttbl{\f0 Times New Roman;}{\f1 Arial;}{\f2 Courier New;}{\f3 Symbol;}}"
    r"{\colortbl;\red200\green30\blue30;\red5\green99\blue193;\red235\green240\blue250;"
    r"\red255\green230\blue0;\red0\green128\blue64;\red18\green52\blue86;}"
    r"{\stylesheet{\s0\f0\fs22\sa160 Normal;}"
    r"{\s1\sbasedon0\snext0\outlinelevel0\b\f1\fs32 Heading 1;}"
    r"{\s2\sbasedon0\snext0\outlinelevel1\i\f1\fs26 Heading 2;}"
    r"{\s3\sbasedon0\snext0\outlinelevel2\f1\fs24 Heading 3;}"
    r"{\*\cs10\additive\ssemihidden Default Paragraph Font;}"
    r"{\*\cs11\additive\sbasedon10\i\cf1 Emphasis;}"
    r"{\*\cs12\additive\sbasedon10\ul\cf2 Hyperlink;}}"
    r"\paperw12240\paperh15840\margl1440\margr1440\margt1440\margb1440"
)


def para(value, style=0, extra=""):
    return r"\pard\plain\s%d%s " % (style, extra) + value + r"\par "


def edge(side, width, color, kind="brdrs"):
    return r"\brdr%s\%s\brdrw%d\brsp40\brdrcf%d" % (side, kind, width, color)


def link(address, value):
    return r'{\field{\*\fldinst HYPERLINK "%s"}{\fldrslt {\cs12\ul\cf2 %s}}}' % (address, text(value))


def picture(crop="", goal=720, png=PNG):
    return (r"{\pict\pngblip\picw1\pich1\picwgoal%d\pichgoal%d%s " % (goal, goal, crop)
            + png.hex() + "}")


def cell_row(cells, colors, shading):
    out = r"\trowd\trgaph108\trleft-108"
    right = -108
    for color, shade in zip(colors, shading):
        right += 3000
        for side in "tblr":
            out += r"\clbrdr%s\brdrs\brdrw%d\brdrcf%d" % (side, 10 + 10 * color, color)
        out += shade + r"\cellx%d" % right
    return out + "".join(r"\pard\plain\intbl\s0 " + c + r"\cell " for c in cells) + r"\row "


def borders_shading():
    out = [para(text("Border and shading colors"), 1)]
    out.append(para(text("A box in red, blue, yellow and green."), 0,
                    edge("t", 15, 1) + edge("b", 30, 2) + edge("l", 15, 4, "brdrdb") + edge("r", 45, 5)))
    out.append(para(text("Grouped one"), 0, edge("t", 15, 6) + edge("b", 15, 6) + edge("bar", 15, 1)))
    out.append(para(text("Grouped two, same box"), 0, edge("t", 15, 6) + edge("b", 15, 6)))
    out.append(para(text("Solid pale blue shading."), 0, r"\shading10000\cbpat3"))
    out.append(para(text("Solid yellow shading under red text."), 0, r"\shading10000\cbpat4\cf1"))
    out.append(para(text("Twenty five percent gray."), 0, r"\shading2500"))
    out.append(para(text("Pattern with a color."), 0, r"\shading5000\cfpat2\cbpat3"))
    out.append(para(text("Odd shading values."), 0, r"\shading-5\cbpat99"))
    out.append(cell_row([text("Red edges, solid blue"), text("Blue edges, 40 percent"), text("Green edges, none")],
                        [1, 2, 5], [r"\clshdng10000\clcbpat3", r"\clshdng4000\clcbpat4", ""]))
    out.append(cell_row([text("Second row"), link("mailto:cell@example.com", "cell@example.com"), picture()],
                        [6, 6, 6], ["", r"\clshdng10000\clcbpat4", r"\clshdng10000\clcbpat3"]))
    out.append(para(text("After the table.")))
    return "".join(out)


def pictures():
    out = [para(text("Pictures"), 1)]
    out.append(para(text("Two of the same picture side by side: ") + picture() + picture() + text(" end.")))
    out.append(para(text("Cropped on every side: ")
                    + picture(r"\piccropl120\piccropr240\piccropt60\piccropb30", 1440)))
    out.append(para(text("Cropped past its size: ") + picture(r"\piccropl9000\piccropr9000", 1440)))
    out.append(para(text("Negative crop: ") + picture(r"\piccropl-200\piccropt-200", 1080)))
    out.append(para(text("Scaled: ") + r"{\pict\pngblip\picw1\pich1\picwgoal720\pichgoal720\picscalex250\picscaley40 "
                    + PNG.hex() + "}"))
    out.append(para(r"{\cs11\i\cf1 " + text("Styled text, then ") + picture() + text(" inside the style.") + "}"))
    out.append(para(picture() + picture() + picture() + picture()))
    return "".join(out)


def lists_lines_links():
    out = [r"\viewkind1\viewscale150\linemod1\linex360\linestarts1\linerestart\sectd\sbknone "]
    out.append(para(text("Lists, line numbers and links"), 1))
    bullet = r"{\pntext\f3\'B7\tab}{\*\pn\pnlvlblt\pnf3\pnindent360{\pntxtb\'B7}}\fi-360\li720"
    for i in range(4):
        out.append(para(text("Bullet %d with " % i) + link("https://example.com/%d" % i, "a link")
                        + text(" and ") + link("mailto:p%d@example.com" % i, "p%d@example.com" % i), 0, bullet))
    out.append(para(text("Typed address someone@example.org and https://quillwp.com in plain text.")))
    out.append(r"\sect\sectd\sbkpage\linemod5\linex720\lineppage ")
    for level, style in enumerate([1, 2, 3, 1, 2, 3]):
        out.append(para(text("Heading %d in section two" % level), style))
        out.append(para(text("Line-numbered body text " * 6)))
    out.append(r"\sect\sectd\sbkcol\cols2 ")
    out.append(para(text("A third section in two columns without line numbers. " * 8)))
    return "".join(out)


# Symbol fonts as Word writes them: \fcharset2, so each byte is the font's
# own code (\'95 in Wingdings is not a bullet).
SYMBOL_HEADER = HEADER.replace(
    r"{\f3 Symbol;}}",
    r"{\f3 Symbol;}{\f4\fcharset2 Wingdings;}{\f5\fcharset2 Webdings;}"
    r"{\f6\fcharset2 Zapf Dingbats;}{\f7\fcharset2 Symbol;}{\f8\fcharset2 Wingdings 2;}}",
)


def symbols():
    """Symbols each way Word writes them: font bytes, Word's private-use F0xx
    codes, SYMBOL fields, a Wingdings bullet, and symbols in a header and a
    footnote, mixed with ordinary text and formatting."""
    out = [r"{\header\pard\plain\s0 " + text("Header ") + r"{\f4 J}\par}"]
    out.append(para(text("Symbol fonts"), 1))
    out.append(para(text("Wingdings bytes: ") + r"{\f4 JKL\'fe\'e0\'95\'a8}" + text(" end.")))
    out.append(para(text("Private-use codes: ") + r"{\f4 \u-4022?\u-3842?}{\f8 \u-3998?}"))
    out.append(para(text("Fields: ")
                    + r'{\field{\*\fldinst SYMBOL 74 \\f "Wingdings" \\s 14}{\fldrslt\f4\fs28 J}}'
                    + text(" and ")
                    + r'{\field{\*\fldinst SYMBOL 168 \\f "Symbol"}{\fldrslt\f7 \'a8}}'))
    out.append(para(text("Symbol: ") + r"{\f7 abgp\'b7\'ae}" + text("  Zapf Dingbats: ")
                    + r"{\f6 abcn4}" + text("  Webdings: ") + r"{\f5 abc\'a4}"))
    out.append(para(text("Formatted: ") + r"{\f4\b\i\fs36\cf1 JKL}" + text(" and ")
                    + r"{\f4\ul\up6 \'fe}"))
    bullet = r"{\pntext\f4\'a7\tab}{\*\pn\pnlvlblt\pnf4\pnindent360{\pntxtb\'a7}}\fi-360\li720"
    for i in range(2):
        out.append(para(text("Wingdings bullet %d" % i), 0, bullet))
    out.append(para(text("A note") + r"{\super\chftn}{\footnote\pard\plain\s0 {\super\chftn}"
                    + text(" Note with ") + r"{\f4 J}\par}" + text(" after it.")))
    return "".join(out)


SOURCES = {
    "fs-borders-shading": borders_shading,
    "fs-pictures": pictures,
    "fs-lists-lines-links": lists_lines_links,
    "fs-symbols": symbols,
}
HEADERS = {"fs-symbols": SYMBOL_HEADER}


def make_sources():
    (OUT / "sources").mkdir(parents=True, exist_ok=True)
    for name, build in SOURCES.items():
        path = OUT / "sources" / (name + ".rtf")
        path.write_text(HEADERS.get(name, HEADER) + build() + "}", encoding="ascii")
        print(path)


def make_quill():
    folders = [str(OUT / "sources")] + [str(f) for f in EARLIER if f.is_dir()]
    subprocess.run(["cargo", "run", "--release", "-p", "quill-formats", "--example", "feature_seeds",
                    str(OUT), *folders], cwd=ROOT, check=True)


def exchange_inputs():
    """The files Word and LibreOffice are given: the sources and Quill's exchange
    saves of the linked and built documents. The deleted and Normal variants
    differ only in their styles, which Quill's own saves already cover."""
    files = sorted((OUT / "sources").glob("*.rtf"))
    for ext in ("docx", "rtf", "odt"):
        files += [p for p in sorted((OUT / "quill").glob("*." + ext))
                  if p.stem.endswith("-linked") or p.stem.startswith("built-")]
    return files


def make_word():
    sys.path.insert(0, str(ROOT / "tools"))
    import word_seeds
    word_seeds.OUT = OUT / "word"
    word_seeds.OUT.mkdir(parents=True, exist_ok=True)
    wc = word_seeds.load_word_check()
    formats = [("docx", ".docx"), ("rtf", ".rtf")]
    with wc.DialogAnswerer():
        wc.start_word()
        for path in exchange_inputs():
            stem = path.stem + "-" + path.suffix[1:]
            if (word_seeds.OUT / (stem + ".docx")).exists():
                continue
            made, failed = word_seeds.convert_robustly(wc, stem, path.suffix, path.read_bytes(), formats)
            print("%-60s %d saved %s" % (stem, len(made), "; ".join(failed)), flush=True)
        wc.quit_word()


def make_libreoffice():
    """One LibreOffice run per output format, over every input at once."""
    lo = OUT / "lo"
    lo.mkdir(parents=True, exist_ok=True)
    wanted = {"odt": "odt", "docx": "docx:MS Word 2007 XML", "rtf": "rtf", "html": "html:HTML (StarWriter)"}
    with tempfile.TemporaryDirectory() as profile, tempfile.TemporaryDirectory() as work:
        # Inputs named by stem and extension, so x.docx and x.rtf don't collide.
        staged = {}
        for path in exchange_inputs():
            copy = Path(work) / "in" / (path.stem + "-" + path.suffix[1:] + path.suffix)
            copy.parent.mkdir(exist_ok=True)
            shutil.copyfile(path, copy)
            staged[copy] = path.suffix[1:]
        for ext, filter_name in wanted.items():
            inputs = [str(c) for c, kind in staged.items() if kind != ext]
            outdir = Path(work) / ext
            try:
                subprocess.run([str(SOFFICE), "-env:UserInstallation=file://" + profile, "--headless",
                                "--convert-to", filter_name, "--outdir", str(outdir), *inputs],
                               capture_output=True, timeout=3600)
            except subprocess.TimeoutExpired:
                print("LibreOffice timed out making", ext)
            made = sorted(outdir.glob("*." + ext)) if outdir.is_dir() else []
            for path in made:
                shutil.copyfile(path, lo / path.name)
            print("%-5s %d of %d converted" % (ext, len(made), len(inputs)), flush=True)


def make_return():
    """Quill reads what the other programs wrote and saves it again."""
    for folder in ["word", "lo"]:
        if (OUT / folder).is_dir():
            subprocess.run(["cargo", "run", "--release", "-p", "quill-formats", "--example", "seed_exports",
                            str(OUT), folder], cwd=ROOT, check=True)


def copy_seed(data, target, suffix=""):
    if not 0 < len(data) <= LIMIT:
        return 0
    destination = ROOT / "fuzz/corpus" / target / (hashlib.sha256(data).hexdigest() + suffix)
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.exists():
        return 0
    destination.write_bytes(data)
    return 1


def seed_corpus():
    """Copy every file into the targets that read it."""
    wanted = {".docx": ["docx", "zip"], ".odt": ["odt", "zip"], ".rtf": ["rtf"],
              ".htm": ["html"], ".html": ["html"], ".qwp": ["native"], ".md": ["markdown"]}
    clipboard = {".rtf": 0, ".htm": 1, ".html": 1}
    counts = {}
    for path in sorted(OUT.rglob("*")):
        suffix = path.suffix.lower()
        if path.name.startswith(("._", "~$")) or not path.is_file():
            continue
        data = path.read_bytes()
        for target in wanted.get(suffix, []):
            counts[target] = counts.get(target, 0) + copy_seed(data, target, suffix)
        if suffix in clipboard:
            # The target's first byte says RTF or HTML; 2 replaces missing fonts.
            for choice in (0, 2):
                made = copy_seed(bytes([clipboard[suffix] + choice]) + data, "clipboard")
                counts["clipboard"] = counts.get("clipboard", 0) + made
        if suffix == ".docx":
            try:
                with zipfile.ZipFile(path) as archive:
                    for name in ("word/document.xml", "word/styles.xml"):
                        if name in archive.namelist():
                            counts["xml"] = counts.get("xml", 0) + copy_seed(archive.read(name), "xml")
                    for item in archive.namelist():
                        if item.startswith("word/media/"):
                            counts["images"] = counts.get("images", 0) + copy_seed(archive.read(item), "images")
            except zipfile.BadZipFile:
                pass
        if suffix == ".odt":
            try:
                with zipfile.ZipFile(path) as archive:
                    for name in ("content.xml", "styles.xml"):
                        if name in archive.namelist():
                            counts["xml"] = counts.get("xml", 0) + copy_seed(archive.read(name), "xml")
            except zipfile.BadZipFile:
                pass
    for target, count in sorted(counts.items()):
        print("%-10s %d new seeds" % (target, count))


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    steps = ["sources", "quill", "word", "libreoffice", "return", "seed_corpus"]
    for step in steps:
        parser.add_argument("--" + step.replace("_", "-"), action="store_true")
    args = parser.parse_args()
    chosen = [s for s in steps if getattr(args, s)] or steps
    for step in chosen:
        print("==", step, flush=True)
        {"sources": make_sources, "quill": make_quill, "word": make_word, "libreoffice": make_libreoffice,
         "return": make_return, "seed_corpus": seed_corpus}[step]()


if __name__ == "__main__":
    main()
