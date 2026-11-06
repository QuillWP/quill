# Language data

Quill's hyphenation and thesaurus data, built by
`tools/make_dicts.py` from the English dictionaries in LibreOffice's
dictionaries repository (https://github.com/LibreOffice/dictionaries,
`en/`). Each file is zlib-compressed UTF-8 text, loaded on first use.

| File | Built from | License |
|---|---|---|
| `hyph_en_US.pat` | `hyph_en_US.dic` (TeX hyphenation patterns and TUGboat exceptions) | BSD-style, see `licenses/README_hyph_en_US.txt` |
| `th_en_US.dat` | `th_en_US_v2.dat` (MyThes thesaurus generated from WordNet) | WordNet license (Princeton University), see below |

The thesaurus is derived from WordNet. WordNet is copyright Princeton
University and is distributed under a license that permits use, copying,
modification and distribution for any purpose without fee, provided its
copyright notice and license accompany the data. See
https://wordnet.princeton.edu/license-and-commercial-use for the full text.

To rebuild after updating the sources:

    python3 tools/make_dicts.py <dir with the LibreOffice en/ files> data
