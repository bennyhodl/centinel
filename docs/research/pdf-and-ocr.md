# PDF Extraction & OCR — Capability Research

**Status:** complete · researched 2026-08-02
**Scope:** Centinel v2 — library + CLI + server + MCP. Retains original PDF bytes; produces a markdown/text rendition for semantic indexing. Language undecided (Rust / Python / TypeScript). **This document does not pick a language.**
**Sourcing rule:** primary sources only — crates.io / PyPI / npm registry pages, repository source, official docs, changelogs, published benchmark repos. Every claim carries a URL.

---

## Executive summary

1. **The Rust ecosystem is genuinely weak here, and the operator's suspicion is correct.** There is no pure-Rust
   layout-aware, table-aware, or markdown-emitting PDF extractor. The two capable Rust options are bindings over
   native C/C++ libraries (`pdfium-render` → Google PDFium; `mupdf` → MuPDF), and one of them is AGPL. Pure Rust
   cannot even rasterise a PDF page in production, which means **Rust cannot do OCR at all without a native
   library or a subprocess**. See §1.1, §4.4, §9.2.
2. **AGPL removes the single best tool in the survey.** PyMuPDF / `pymupdf4llm` / `mupdf-rs` / MuPDF.js are all
   AGPL-3.0. Centinel ships a server, so AGPL §13's network clause would bind every operator and every fork.
   Disqualifying. See §2.1.
3. **`docling` (MIT, IBM/LF AI & Data) is the closest thing to a single answer** — it covers PDF extraction, OCR,
   layout, reading order, table structure, page+bbox+charspan provenance, markdown output, and DOCX/XLSX/PPTX/HTML/
   images, in one MIT dependency. It is Python-only. See §1.2, §5.2, §6.2.
4. **OCR on degraded scans is unsolved by everyone.** Best "old scans" score on olmOCR-Bench is 50.4 out of 100.
   Do not promise clean text from 1990s scanned municipal records. See §8.1.
5. **A paid OCR API is neither the cheapest nor the best option.** Mistral OCR is $4/1000 pages and scores 72.0;
   self-hosted olmOCR claims "less than $200 USD per million pages" and scores 82.4. The API's value is avoiding a
   GPU, not quality. See §3.3.
6. **Extract to a structured representation and render markdown from it.** Markdown-first extraction destroys page
   numbers and bounding boxes, breaking citability, and makes version-diffing unreliable. See §5.4, §7.2.
7. **Decide OCR per page, never per document,** and detect *garbage* text layers (broken `ToUnicode` → `"�����"`),
   not just missing ones. Mixed born-digital/scanned packets are the normal case in `.gov`. See §4.
8. **The polyglot option is the one the evidence actually supports if Rust is wanted:** Rust for crawl/store/hash/
   diff, a separate extraction worker (subprocess `pdftotext`/`pdftoppm`/`tesseract`, or a Python `docling`
   sidecar). See §9.2, §9.5.

---

## 1. PDF text extraction, per language

### 1.1 Rust

Registry metadata pulled from the crates.io API on 2026-08-02.

| Crate | Latest | Released | License | Total downloads | Pure Rust? |
|---|---|---|---|---|---|
| [`lopdf`](https://crates.io/crates/lopdf) | 0.44.0 | 2026-07-10 | MIT | 13,898,428 | Yes |
| [`pdf-extract`](https://crates.io/crates/pdf-extract) | 0.12.0 | 2026-06-25 | MIT | 3,516,480 | Yes (built on `lopdf`) |
| [`pdfium-render`](https://crates.io/crates/pdfium-render) | 0.9.3 | 2026-07-14 | MIT OR Apache-2.0 | 1,802,628 | **No — binding over Google's PDFium (C++)** |
| [`pdf`](https://crates.io/crates/pdf) (pdf-rs) | 0.10.0 | 2026-03-02 | MIT | 566,771 | Yes |
| [`mupdf`](https://crates.io/crates/mupdf) (mupdf-rs) | 0.8.0 | 2026-06-22 | **AGPL-3.0** | 1,533,414 | No — binding over MuPDF (C) |
| [`extractous`](https://crates.io/crates/extractous) | 0.3.0 | 2024-12-21 | Apache-2.0 | 631,451 | No — natively-compiled Apache Tika (Java/GraalVM) |

**`lopdf`** — a PDF *object model* library, not a text extractor. Its own description is "A Rust library for PDF
document manipulation" ([crates.io](https://crates.io/crates/lopdf)). It parses the file structure, xref tables,
dictionaries and content streams. Getting readable text out of it means implementing font decoding, CMap/ToUnicode
handling, and text-positioning logic yourself. It is the correct foundation layer, not the answer.

**`pdf-extract`** — the closest thing Rust has to a batteries-included extractor. It is built on `lopdf` and is
pure Rust. Public API ([docs.rs](https://docs.rs/pdf-extract/latest/pdf_extract/)) exposes `extract_text()`,
`extract_text_by_pages() -> Vec<String>`, `output_doc_page()`, and an `OutputDev` trait with three implementations:
`PlainTextOutput`, `HTMLOutput`, and `SVGOutput`. It carries `MediaBox` and uses `euclid` for geometry, so
coordinate data exists internally. Two honest caveats: (a) the README documents no layout, column, table, or OCR
capability at all — the "See also" section points at other projects "with more advanced features like layout
parsing", which is a tacit admission; (b) it is a text-layer extractor only. Multi-column government documents come
out in content-stream order, which is frequently not reading order. Tables come out as a stream of cell text with no
structure. Scanned pages come out empty.

**`pdfium-render`** — a "high-level idiomatic Rust wrapper around Pdfium, the C++ PDF library used by the Google
Chromium project". **This is the crux the brief asks about.** Its README states plainly:
> "`pdfium-render` does not include Pdfium itself."

Three acquisition paths, all of which are a native-binary dependency:
1. **Dynamic linking at runtime** — you ship a `libpdfium.{so,dylib,dll}` next to the executable or use a
   system copy. Prebuilt binaries come from third-party GitHub release repos
   ([`bblanchon/pdfium-binaries`](https://github.com/bblanchon/pdfium-binaries),
   [`paulocoutinhox/pdfium-lib`](https://github.com/paulocoutinhox/pdfium-lib)).
2. **Static linking** via the `static` feature and `Pdfium::bind_to_statically_linked_library()`, pointing
   `PDFIUM_STATIC_LIB_PATH` at a Pdfium you built or sourced yourself. Building Chromium's PDFium from source
   requires `depot_tools` and is a multi-GB, multi-hour affair.
3. **WASM**, where "packaging an external build of Pdfium as a separate WASM module is essential."

So: choosing `pdfium-render` is choosing to vendor and version-manage a ~10 MB C++ blob per target triple,
sourced from a community binary-distribution repo rather than from Google. Deployment-burden-wise that is *the same
class of problem* as shelling out to `pdftotext`, and calling it "a Rust library" obscures that. It does have the
best capability of the Rust options — real text extraction with `PdfPageText`, character-level bounds, and page
rendering to bitmaps (which is what you need to feed OCR). Licence-wise it is clean: the wrapper is MIT OR Apache-2.0 and
PDFium's own [LICENSE file](https://raw.githubusercontent.com/chromium/pdfium/main/LICENSE) carries a BSD-3-Clause
notice followed by the full Apache-2.0 text.

**`pdf` / pdf-rs** — pure Rust reader. README says "Modifying and writing PDFs is still experimental" and the
contribution ask is literally "add different PDF files to `tests/files` and see if they pass the tests", which
tells you the compatibility surface is community-tested rather than systematically validated. Text extraction
exists as an example (`cargo run --example text`); fidelity is unspecified. Rendering lives in a separate repo
(`pdf_render`, via Pathfinder). Lower download count than the others. Not a production text pipeline.

**`mupdf` (mupdf-rs)** — technically the strongest extraction engine available to Rust (MuPDF has good
structured-text output, `stext` with per-char bounding boxes), **but it is AGPL-3.0**. See §2. For an MIT project
that encourages forks, this is disqualifying unless you buy Artifex's commercial licence.

**`extractous`** — interesting outlier: Apache-2.0, wraps Apache Tika compiled to a native image via GraalVM, so
one call handles PDF/DOCX/XLSX/PPTX/email/etc. Last release 0.3.0 on **2024-12-21**, i.e. ~19 months stale as of
this writing. It is Rust-callable but is not Rust — it is a JVM ecosystem inside a native image. Worth flagging
in §6 as the "one library covers all formats" option for Rust.

**Honest summary for Rust:** there is no pure-Rust, layout-aware, table-aware, markdown-emitting PDF extractor.
The pure options (`pdf-extract`, `pdf-rs`) are text-layer-only and layout-naive. The capable options
(`pdfium-render`, `mupdf`) are native-library bindings, and one of them is AGPL. Nothing in Rust is comparable to
`pdfplumber`, `docling`, or `marker`. This is a genuine ecosystem gap, not a search failure.

### 1.2 Python

Metadata from PyPI project pages, fetched 2026-08-02.

| Package | Latest | Released | License | Native dep? |
|---|---|---|---|---|
| [`pypdf`](https://pypi.org/project/pypdf/) | 6.14.2 | 2026-06-23 | BSD-3-Clause | No — pure Python |
| [`pdfminer.six`](https://pypi.org/project/pdfminer.six/) | 20260107 | 2026-01-07 | MIT | No — pure Python |
| [`pdfplumber`](https://pypi.org/project/pdfplumber/) | 0.11.10 | 2026-06-15 | MIT | No (built on pdfminer.six) |
| [`PyMuPDF`](https://pypi.org/project/PyMuPDF/) | 1.28.0 | 2026-06-29 | **AGPL-3.0 or Artifex commercial** | Bundled MuPDF (C), wheels ship it |
| [`pymupdf4llm`](https://pypi.org/project/pymupdf4llm/) | 1.28.0 | 2026-06-29 | **AGPL-3.0 or Artifex commercial** | via PyMuPDF |
| [`docling`](https://pypi.org/project/docling/) | 2.117.0 | 2026-07-30 | MIT | Models downloaded; no system binary required for default path |
| [`marker-pdf`](https://pypi.org/project/marker-pdf/) | 2.0.0 | 2026-07-20 | Apache-2.0 code / **OpenRAIL-M weights** | GPU strongly recommended |
| [`unstructured`](https://pypi.org/project/unstructured/) | 0.25.0 | 2026-07-31 | Apache-2.0 | **Yes — poppler-utils, tesseract-ocr, libmagic, libreoffice** |
| [`markitdown`](https://pypi.org/project/markitdown/) | 0.1.7 | 2026-07-29 | MIT | No |
| [`mineru`](https://pypi.org/project/mineru/) | 3.4.4 | 2026-07-10 | MinerU OSS License (Apache-2.0 based) | Models; GPU optional |

**`pypdf`** — pure Python, BSD-3-Clause, actively maintained. Describes itself as "capable of splitting, merging,
cropping, and transforming the pages of PDF files" plus "retrieving text and metadata". It is a document-object
library first and a text extractor second. Its extraction is text-layer only, no layout analysis, no table
structure, no OCR. It does have a `layout` extraction mode that approximates visual positioning by inserting
whitespace, which is better than nothing for two-column text but is not real column detection. Correct role:
metadata, page count, splitting, encryption handling — not the primary extractor.

**`pdfminer.six`** — MIT, pure Python, the foundation almost everything else in Python sits on. Ships "automatic
layout analysis", per-character positioning, CJK/vertical writing, Type1/TrueType/Type3/CID fonts, embedded image
extraction (JPG/PNG/TIFF/JBIG2), RC4/AES decryption, and hOCR output. Slow — it is pure Python doing per-glyph
work — but correctness and coordinate fidelity are its whole point.

**`pdfplumber`** — MIT, built on pdfminer.six, and the best-documented option for *structured* extraction from
born-digital PDFs. `.extract_words()` "returns a list of all word-looking things and their bounding boxes";
character objects carry `x0/x1/y0/y1/top/bottom`. Table extraction is a first-class feature with a documented
algorithm (it "borrows heavily from Anssi Nurminen's master's thesis") plus `.find_tables()`, `.extract_tables()`,
and pluggable line/word detection strategies, and a visual debugger that renders the detected table grid over the
page image. It is explicit about its boundary: it "works best on machine-generated, rather than scanned, PDFs" and
lists "Optical character recognition (OCR)" among the things it **does not do**. For agenda packets with dense
tables and no scanning, this is the strongest non-ML option in any language.

**`PyMuPDF` / `pymupdf4llm`** — the most capable single library on this list technically. Text extraction with
font/size/colour/position, table detection, image extraction, page rendering, Tesseract integration, annotations,
redaction, forms. `pymupdf4llm` emits "GitHub-compatible Markdown with headings, bold, italic, monospace
formatting, code blocks, tables, image references, and lists", plus a JSON mode carrying "bounding box coordinates,
layout element types, and font metadata", multi-column handling, header/footer removal, and page-chunked output for
vector stores. **It is AGPL-3.0 or Artifex commercial.** See §2 — for Centinel this is almost certainly out.

**`docling`** (IBM, MIT) — the standout for this project's constraints. Inputs: "PDF, DOCX, PPTX, XLSX, HTML, EPUB,
WAV, MP3, WebVTT, Box Notes, email formats (EML, MSG), images (PNG, TIFF, JPEG, ...), LaTeX, DocLang, plain text",
plus video and ODF. Outputs: Markdown, HTML, DocLang, DocTags, and "lossless JSON". Features named on the project
page: "page layout, reading order, table structure, code, formulas, image classification", "extensive OCR support
for scanned PDFs and images", multiple VLM backends, local-only processing, and a "unified, expressive
DoclingDocument representation" that carries spatial information. MIT-licensed *and* covers §6 (other formats)
*and* §7 (markdown) *and* §5 (provenance) in one dependency. Also ships an MCP server and an API server, which maps
onto Centinel's own shape.

**`marker-pdf`** (Datalab) — fastest high-quality option. Inputs PDF/images/PPTX/DOCX/XLSX/HTML/EPUB; outputs
Markdown, JSON, HTML, chunks. Published numbers on its own PyPI page: olmocr-bench (1,403 PDFs, ~8,400 tests)
**76.0% overall / 83.5% digital-only in balanced mode**, 66.6%/71.6% in fast mode, at 2.9 pages/s (balanced GPU)
and 7.4 pages/s (fast GPU); claims to beat MinerU and docling on accuracy and speed. **Licensing is split**: code
Apache-2.0, model weights under "a modified AI Pubs Open Rail-M license (free for research, personal use, and
startups under $5M funding/revenue)". That threshold is a real constraint for a fork-encouraged MIT repo — see §2.

**`unstructured`** — Apache-2.0, 60+ file types, but the system-dependency list is the story:
`libmagic-dev`, `poppler-utils`, `tesseract-ocr`, `libreoffice`, `pandoc`. It is a Python *orchestrator* over a pile
of external binaries. That is a legitimate design, but it means "Python covers all formats" is partly a claim about
apt-get, not about Python.

**`markitdown`** (Microsoft, MIT) — converts PDF/XLSX/PPTX/DOCX/Outlook/audio/YouTube to Markdown. Lightweight and
MIT, but its PDF path is thin (it delegates to `pdfminer.six`) and it does no OCR and no layout analysis. Good for
the *non-PDF* half of §6, weak as a PDF extractor.

**`mineru`** — "A practical document parsing tool for converting PDF, images, DOCX, PPTX, and XLSX into Markdown and
JSON", formula→LaTeX, table→HTML, OCR across 109 languages, reading-order output, header/footer removal. Licensed
under a "MinerU Open Source License (based on Apache 2.0)" — a bespoke licence, so read it before adopting. Hardware
floor is documented: 16 GB RAM, 4 GB VRAM (pipeline backend, CPU supported) or 8 GB VRAM (VLM backend, **no CPU
support**).

**Summary for Python:** this is where the ecosystem actually lives. Three separate MIT/Apache options
(`pdfplumber`, `docling`, `unstructured`) each solve a different part of the problem, and the best of them
(`docling`) covers extraction, OCR, tables, provenance, markdown output, and non-PDF formats in one MIT dependency.
The one library that is technically strongest (`PyMuPDF`) is the one that is licence-blocked.

### 1.3 TypeScript / Node

Metadata from the npm registry (`registry.npmjs.org/<pkg>/latest`), fetched 2026-08-02.

| Package | Version | License | Native dep? | Notes |
|---|---|---|---|---|
| [`pdfjs-dist`](https://www.npmjs.com/package/pdfjs-dist) | 6.2.108 | Apache-2.0 | Optional `@napi-rs/canvas` | Mozilla PDF.js generic build |
| [`pdf-parse`](https://www.npmjs.com/package/pdf-parse) | 2.4.5 | Apache-2.0 | via `@napi-rs/canvas` | Wraps `pdfjs-dist` 5.4.296 |
| [`unpdf`](https://www.npmjs.com/package/unpdf) | 1.8.0 | MIT | Optional `@napi-rs/canvas` | Serverless-friendly PDF.js repack |
| [`@opendocsg/pdf2md`](https://www.npmjs.com/package/@opendocsg/pdf2md) | 0.2.7 | MIT | via `unpdf` | Heuristic PDF→Markdown |
| [`mupdf`](https://www.npmjs.com/package/mupdf) | 1.28.0 | **AGPL-3.0-or-later** | WASM build of MuPDF | Official Artifex MuPDF.js |
| [`node-poppler`](https://www.npmjs.com/package/node-poppler) | 10.0.1 | MIT | **Yes — poppler-utils binaries** | Subprocess wrapper |
| [`tesseract.js`](https://www.npmjs.com/package/tesseract.js) | 7.0.0 | Apache-2.0 | No (WASM) | See §3.1 |

**Everything real in this column is PDF.js.** `pdfjs-dist` is "Generic build of Mozilla's PDF.js library",
Apache-2.0. `pdf-parse` v2 depends on `pdfjs-dist: 5.4.296`. `unpdf` is a repack of PDF.js for "all JavaScript
runtimes" (workers, edge, Deno, Bun) with the DOM assumptions stripped. `@opendocsg/pdf2md` depends on `unpdf`,
which depends on PDF.js. So the TypeScript ecosystem is one engine with three ergonomic wrappers, and the ceiling
of the whole column is the ceiling of PDF.js's `getTextContent()`.

What PDF.js gives you is a per-page array of text items, each with a transform matrix (so you *do* get
position), font name, width and height, plus `hasEOL`. That is enough to reconstruct reading order and columns
**if you write the geometry logic yourself** — which is exactly what `@opendocsg/pdf2md` does heuristically. It is
not enough for table structure without significant custom work; there is no equivalent of `pdfplumber`'s ruling-line
table detector in this ecosystem.

**`pdf-parse` v2** — worth noting because it is a genuine rewrite by a new maintainer (`mehmet-kozan`), not the
long-abandoned 1.x. Self-described as "Pure TypeScript, cross-platform module for extracting text, images, and
tabular data from PDFs." Its README documents `getText({ partial: [3] })` for per-page extraction, `getTable()` for
tabular data, and `getImage()` with an `imageThreshold` filter to drop decorative images. It does **not** document
OCR, bounding boxes, or markdown output, and `getHeader()` is "Node only, will not work in browser environments".
The `getTable()` claim should be validated against real agenda-packet tables before being relied on — there is no
published benchmark for it.

**`@opendocsg/pdf2md`** — MIT, the only direct PDF→Markdown option in Node. It is heuristic: it clusters text items
by font size to guess heading levels and by x-position to guess paragraphs. It has no layout model, no table
support, and no OCR. At version 0.2.7 with three dependencies (`unpdf`, `enumify`, `minimist`) it is a small script,
not a document-understanding system. Fine for simple linear reports; it will mangle a 400-page packet.

**`mupdf` (MuPDF.js)** — Artifex's official JS/WASM build, version 1.28.0, **AGPL-3.0-or-later**. Same capability
story as the Rust and Python MuPDF bindings, same licence problem. See §2.

**`node-poppler`** — MIT wrapper that shells out to `pdftotext`, `pdftoppm`, `pdftohtml`, etc. Honest about what it
is ("Asynchronous Node.js wrapper for the Poppler PDF rendering utilities"). It ships an optional
`node-poppler-win32` package for Windows binaries but on Linux/macOS you install poppler yourself. `pdftotext -layout`
is genuinely good at multi-column reading order — better than PDF.js out of the box — so this is a real option, but
it is subprocess-plus-external-binary, and `-bbox-layout` is the only way to get coordinates back.

**Summary for TypeScript:** one engine (PDF.js), Apache-2.0/MIT throughout, no native dependency required for the
text-layer path, and genuinely good runtime portability. But there is no table-structure extractor, no layout model,
no reading-order model, and no document-understanding stack. Anything beyond "flatten the text layer" is code you
write. The ML/VLM tier does not exist in this column at all.

### 1.4 The binding question (pure-language vs. native dependency)

The brief asks that this not be hidden by the language label, so, plainly:

| Option | Language label | What actually runs | Deployment burden |
|---|---|---|---|
| `pdf-extract`, `lopdf`, `pdf-rs` | Rust | Rust | **None** — static binary |
| `ocrs` | Rust | Rust + RTen (Rust ML runtime) | Model files downloaded at first run |
| `pdfium-render` | Rust | **Google PDFium (C++)** | Ship/locate `libpdfium` per target triple |
| `mupdf` (Rust) | Rust | **MuPDF (C)** | Compiled in; AGPL |
| `extractous` | Rust | **Apache Tika (Java, GraalVM native image)** | Large native image |
| `pypdf`, `pdfminer.six`, `pdfplumber` | Python | Python | Python runtime only |
| `PyMuPDF` | Python | **MuPDF (C)**, bundled in wheels | Wheel install; AGPL |
| `docling`, `marker`, `mineru`, `surya` | Python | Python + PyTorch/ONNX + model weights | GB of weights, optional GPU |
| `unstructured` | Python | **poppler + tesseract + libreoffice + pandoc** | apt-get list |
| `pdfjs-dist`/`unpdf`/`pdf-parse` | TypeScript | JavaScript (WASM-free for text) | Node runtime only |
| `mupdf` (npm) | TypeScript | **MuPDF compiled to WASM** | Self-contained; AGPL |
| `node-poppler` | TypeScript | **poppler binaries via subprocess** | Install poppler |
| `tesseract.js` | TypeScript | **Tesseract compiled to WASM** | Self-contained (see §3.1) |

The pattern: Rust's *capable* PDF options are all native-library bindings, so Rust's headline advantage — a single
static binary with no runtime — evaporates the moment you need real extraction quality. TypeScript's options are
genuinely dependency-free but cap out at flattening a text layer. Python's options require a Python runtime you were
going to need anyway, and only `unstructured` forces external binaries on you.

---

## 2. Licenses

Centinel is MIT and explicitly encourages forks. That makes copyleft — especially network copyleft — a
first-order filter, not a footnote.

### 2.1 The disqualifying tier: AGPL (MuPDF family)

**Every MuPDF binding, in every language, is AGPL-3.0.** This is one upstream licence propagating through three
ecosystems:

| Package | Language | License field |
|---|---|---|
| [`PyMuPDF`](https://pypi.org/project/PyMuPDF/) | Python | "Dual Licensed - GNU AFFERO GPL 3.0 or Artifex Commercial License" |
| [`pymupdf4llm`](https://pypi.org/project/pymupdf4llm/) | Python | same |
| [`mupdf`](https://crates.io/crates/mupdf) (mupdf-rs) | Rust | `AGPL-3.0` |
| [`mupdf`](https://www.npmjs.com/package/mupdf) (MuPDF.js) | TypeScript | `AGPL-3.0-or-later` |

PyMuPDF's own documentation confirms it:
> "PyMuPDF and MuPDF are now available under both, open-source AGPL and commercial license agreements."
> "If you determine you cannot meet the requirements of the AGPL, please contact Artifex for more information
> regarding a commercial license."
> — [pymupdf.readthedocs.io/en/latest/about.html](https://pymupdf.readthedocs.io/en/latest/about.html)

**Why this matters more than plain GPL for Centinel specifically.** Centinel ships a **server**. AGPL §13 extends
the source-disclosure obligation to users who interact with the software *over a network*, not just to those who
receive a copy. So an operator who runs a Centinel server with an AGPL extractor linked in owes AGPL source to
everyone hitting that server, including their private modifications. That obligation is inherited by every fork.
"MIT with forks encouraged" and "AGPL dependency in the extraction path" are not compatible positions.

**Verdict: PyMuPDF, pymupdf4llm, mupdf-rs, and MuPDF.js are out**, unless the project buys an Artifex commercial
licence — which forks cannot inherit, so it does not solve the fork problem either.

This is a genuine loss. PyMuPDF is the fastest and most feature-complete extractor in Python, and `pymupdf4llm` is
the single best off-the-shelf PDF→Markdown converter. Being unable to use it removes the most convenient answer in
the entire survey.

### 2.2 The "read the fine print" tier: split code/weights licences

`marker-pdf` and `surya-ocr`, both from Datalab, have **Apache-2.0 code and non-open model weights**:

> "modified AI Pubs Open Rail-M license (free for research, personal use, and startups under $5M funding/revenue)"
> — [pypi.org/project/surya-ocr](https://pypi.org/project/surya-ocr/), same wording on
> [pypi.org/project/marker-pdf](https://pypi.org/project/marker-pdf/)

Practical read for Centinel:
- The **code** is Apache-2.0 and safe to depend on and fork.
- The **weights** are not open source. OpenRAIL-M also carries use restrictions (behavioural clauses), which by
  definition fail the OSI "no discrimination against fields of endeavour" test.
- The $5M funding/revenue threshold is a per-downstream-user condition. Centinel cannot grant it and cannot know
  whether a fork qualifies. Any fork operated by a >$5M entity would need to buy a Datalab licence.

That is not automatically disqualifying — it is a *pluggable backend* question. Making marker/surya an optional
opt-in backend, defaulted off, with the licence stated at the point of enabling it, is defensible. Making it the
default extraction path is not.

`mineru` ships a bespoke "MinerU Open Source License (based on Apache 2.0)". A bespoke licence needs to be read in
full before adoption; "based on Apache 2.0" is not a licence identifier.

`docling` is MIT for the codebase but the README notes: "for individual model usage, please refer to the model
licenses found in the original packages." The layout and TableFormer models it uses are IBM-published; each needs
checking individually if you ship weights rather than downloading them at runtime.

### 2.3 The clean tier

Safe for an MIT project with forks encouraged:

| Component | License | Source |
|---|---|---|
| `lopdf`, `pdf-extract`, `pdf` (pdf-rs) | MIT | crates.io |
| `pdfium-render` | MIT OR Apache-2.0 | crates.io |
| PDFium (the C++ library) | BSD-3-Clause + Apache-2.0 | [LICENSE](https://raw.githubusercontent.com/chromium/pdfium/main/LICENSE) |
| `ocrs` | MIT OR Apache-2.0 | crates.io |
| `leptess` | MIT | crates.io |
| `rusty-tesseract` | MIT | crates.io |
| `extractous` | Apache-2.0 | crates.io |
| `pypdf` | BSD-3-Clause | PyPI |
| `pdfminer.six`, `pdfplumber` | MIT | PyPI |
| `docling` (code) | MIT | PyPI + README |
| `unstructured` | Apache-2.0 | PyPI |
| `markitdown` | MIT | PyPI |
| `pdfjs-dist`, `pdf-parse` | Apache-2.0 | npm |
| `unpdf`, `@opendocsg/pdf2md`, `node-poppler` | MIT | npm |
| Tesseract | Apache-2.0 | [README](https://github.com/tesseract-ocr/tesseract) |
| `tesseract.js` | Apache-2.0 | npm |
| PaddleOCR | Apache-2.0 | PyPI |
| EasyOCR | Apache-2.0 | PyPI |
| olmOCR | Apache-2.0 | README |

### 2.4 The subprocess-boundary case: poppler

Poppler (`pdftotext`, `pdftoppm`) is GPL-licensed. `node-poppler` is MIT *as a wrapper* because it invokes the
poppler CLI tools as **separate processes** rather than linking them. Under the conventional reading of the GPL,
invoking a GPL program as a subprocess across a process boundary does not make the caller a derivative work — this
is why MIT/BSD tools routinely shell out to `ffmpeg`, `git`, and `pdftotext`.

Two consequences for Centinel:
1. **Shelling out to `pdftotext` is licence-safe** for an MIT project. Linking poppler as a library is not.
2. It makes the subprocess design *more* attractive than usual: it is simultaneously the language-portability
   answer and the licence answer.

The same reasoning applies to any GPL tool Centinel invokes rather than links. It does **not** rescue AGPL library
bindings, which are linked in-process by construction.

### 2.5 Summary

- **AGPL — out:** PyMuPDF, pymupdf4llm, mupdf-rs, MuPDF.js. This costs the project its single best tool.
- **Restricted weights — optional backend only:** marker, surya.
- **Bespoke licence — read before use:** mineru.
- **Clean:** everything else surveyed, including Tesseract, PDFium, PDF.js, pdfplumber, docling, and ocrs.
- **Subprocess is a licence tool, not just a portability tool:** it is what makes GPL poppler usable.

---

## 3. OCR

### 3.1 Tesseract — bindings vs. subprocess

Tesseract itself is **Apache-2.0**, currently v5.x (5.0.0 shipped 2021-11-30), with an LSTM neural engine as
default and the legacy pattern engine behind `--oem 0`. Critically for §5, its README confirms:
> "Tesseract supports various output formats: plain text, hOCR (HTML), PDF, invisible-text-only PDF, TSV, ALTO and PAGE."

hOCR, TSV, ALTO and PAGE all carry **bounding boxes**. So Tesseract can satisfy the anchoring requirement — but only
if you ask for a structured format instead of plain text. `tesseract in.png out` gives you a bag of words with no
coordinates; `tesseract in.png out tsv` gives you per-word `left/top/width/height/conf`.

**Per-language binding situation:**

| Option | Language | Mechanism | License | Last release | Verdict |
|---|---|---|---|---|---|
| [`leptess`](https://crates.io/crates/leptess) | Rust | FFI to `libtesseract` + `libleptonica` | MIT | **0.14.0, 2023-02-21** | Stale ~3.5 yrs |
| [`rusty-tesseract`](https://crates.io/crates/rusty-tesseract) | Rust | Wraps the `tesseract` CLI | MIT | 1.1.10, 2024-03-25 | Subprocess in a trenchcoat |
| `pytesseract` | Python | Wraps the `tesseract` CLI | Apache-2.0 | active | Subprocess |
| `tesserocr` | Python | Cython FFI to `libtesseract` | MIT | active | True binding |
| [`tesseract.js`](https://www.npmjs.com/package/tesseract.js) | TypeScript | **WASM port** | Apache-2.0 | 7.0.0 | See below |

**`leptess`** is the only real Rust FFI binding and it needs system headers: its README says "Make sure you have
clang, Leptonica and Tesseract installed" and gives `sudo apt-get install libleptonica-dev libtesseract-dev clang`.
It requires "Tesseract ... version 4.0.0 or above". Last published **February 2023** — that is a long time for a
crate sitting in a build-critical position. Choosing it means a `clang` + dev-headers build dependency *and* an
unmaintained-crate risk, in exchange for avoiding a subprocess.

**`rusty-tesseract` and `pytesseract` are not bindings** — they shell out to the `tesseract` executable and parse
stdout. Using them is functionally identical to running `tesseract` yourself, minus the control. If you are going
to shell out anyway, shelling out directly is more honest and gives you the full flag surface (`--psm`, `--oem`,
`-c preserve_interword_spaces=1`, TSV/hOCR/ALTO output).

**Does `tesseract.js` (WASM) change the deployment story meaningfully? Yes — it is the single strongest
deployment-story argument in this entire document, and it belongs to TypeScript.**

Its README: it "works by wrapping a WebAssembly port of Tesseract." No native binary, no `apt-get`, no per-platform
build, no version skew between dev and prod. It runs identically in Node, Deno, Bun, browsers, and edge runtimes.
v7 supports `hocr: true` and a `blocks` output for "granular data [word/symbol level]" bounding boxes. v6 fixed a
memory leak and reduced runtime and memory; v5 cut language-data size 54% (English) / 73% (Chinese) and roughly
halved cold-start runtime.

The honest caveats, from the project itself:
> "This project does not modify core Tesseract features. Most notably, Tesseract.js does not support PDF files and
> does not modify the Tesseract recognition model to improve accuracy."

So: same accuracy as Tesseract 5 (no better), **no PDF input** — you must rasterise pages yourself and feed images —
and WASM is slower than native, typically by a meaningful multiple, though the README does not publish a number.
Language `.traineddata` files are downloaded at runtime and cached, which means a network dependency on first use
per language — relevant for a tool that "runs unattended on a schedule" behind a firewall. Pre-seeding the cache
is possible and should be done.

Note also that WASM is not exclusive to Tesseract: `mupdf` on npm is MuPDF-as-WASM with the same
zero-native-dependency property (but AGPL), and `pdfium-render` supports a WASM path. The general point stands:
**WASM converts a native dependency into a portable artifact, and only the JS ecosystem gets it for free.**

### 3.2 PaddleOCR / Surya / EasyOCR

| Engine | License | Latest | Languages | Setup burden |
|---|---|---|---|---|
| [PaddleOCR](https://pypi.org/project/paddleocr/) | Apache-2.0 | 3.7.0 (2026-06-11) | PP-OCRv6: 50 in one model; PaddleOCR-VL: 111 | **High** — PaddlePaddle framework installed separately |
| [Surya](https://pypi.org/project/surya-ocr/) | Apache-2.0 code / **OpenRAIL-M weights** | 0.22.1 (2026-07-20) | 90+ (87.2% pass over a 91-language bench) | Moderate — PyTorch + weights; GPU wanted |
| [EasyOCR](https://pypi.org/project/easyocr/) | Apache-2.0 | 1.7.2 (**2024-09-24**) | 80+ | Low-moderate — PyTorch; `gpu=False` works |

**PaddleOCR** is the accuracy leader among non-VLM OCR and is fully Apache-2.0 including weights, which makes it
the most licence-clean high-quality option. Its capability list goes well past OCR: "table, formula, and chart
recognition", "document layout analysis and structure parsing", "multi-page document handling with hierarchical
heading identification". The cost is setup: PaddlePaddle is a separate deep-learning framework with its own CUDA
matrix, and it is the least pleasant of the major frameworks to install reproducibly. Its own PyPI page hedges —
"only minimal core dependencies are required for basic text recognition; additional dependencies for document
parsing and information extraction can be installed as needed" — which is another way of saying the full
document-parsing path pulls in a lot.

**Surya** is the best quality-per-setup ratio: a 650M-parameter VLM scoring "83.3% on olmOCR-bench (top under 3B
params)" at 5 pages/s on an RTX 5090, with layout analysis, **reading order detection**, table row/column
recognition, and LaTeX for equations. It is small enough to be practical. The blocker is the weights licence (§2.2).

**EasyOCR** is the easiest to install and the weakest of the three. Last release **September 2024** — nearly two
years stale. It is a detector+recognizer with no layout model, no reading-order model, and no table support: it
returns boxes and strings. For government documents that means a bag of words with coordinates, and you rebuild
reading order yourself. Its main virtue is that it is Apache-2.0 end to end and runs on CPU.

**All three are Python-only.** There are no maintained Rust or TypeScript bindings for any of them. Rust's nearest
equivalents are ONNX re-implementations of PaddleOCR models (`ocr-rs`, `pure-onnx-ocr-sync`, `oar-ocr` on
crates.io), which are small, young, single-maintainer projects with no published benchmarks — usable in principle,
unproven in practice.

**Rust's own native option is [`ocrs`](https://crates.io/crates/ocrs)** (MIT OR Apache-2.0, 0.12.2, 2026-03-27),
which runs PyTorch-trained models exported to ONNX on the RTen runtime — genuinely pure Rust, no native deps,
models auto-downloaded to `~/.cache/ocrs`. But read its README before getting excited:
> "ocrs is currently in an early preview. Expect more errors than commercial OCR engines."
> "ocrs currently recognizes the Latin alphabet only (eg. English). Support for more languages is planned."

English-only is survivable for `.gov`. "Expect more errors" is not survivable when the whole point is a searchable
corpus of public records. There are no published accuracy numbers for `ocrs` at all — no olmOCR-Bench entry, no
comparison to Tesseract. **This is the clearest single gap in the Rust story.**

### 3.3 VLM-based OCR

This tier has moved fast and now clearly beats classical OCR on scanned documents. The authoritative primary
source is **[olmOCR-Bench](https://github.com/allenai/olmocr)** (AI2), which is pass/fail and machine-checkable
rather than fuzzy-edit-distance:
> "All facts checked about documents are either pass/fail. We want it to be very clear if your OCR system fails a test."

Its seven categories map unusually well onto government documents: arXiv Math, **Old Scans Math**, **Tables**,
**Old Scans** (historical letters and typewritten documents), **Headers/Footers** (text that *should* be excluded),
**Multi Column**, and Long Tiny Text. Test classes are text presence, text absence, **natural reading order**,
table accuracy, and math formula accuracy.

Leaderboard as published in the olmOCR README (higher is better; the "Old scans" and "Multi column" columns are the
ones that matter most for `.gov`):

| System | ArXiv | Old scans math | Tables | Old scans | Headers & footers | Multi column | Long tiny text | Base | **Overall** |
|---|---|---|---|---|---|---|---|---|---|
| Chandra OCR 0.1.0 | 82.2 | 80.3 | 88.0 | **50.4** | 90.8 | 81.2 | 92.3 | 99.9 | **83.1±0.9** |
| Infinity-Parser 7B | 84.4 | **83.8** | 85.0 | 47.9 | 88.7 | **84.2** | 86.4 | 99.8 | **82.5** |
| **olmOCR v0.4.0** | 83.0 | 82.3 | 84.9 | 47.7 | 96.1 | 83.7 | 81.9 | 99.7 | **82.4±1.1** |
| PaddleOCR-VL | **85.7** | 71.0 | 84.1 | 37.8 | **97.0** | 79.9 | 85.7 | 98.5 | **80.0±1.0** |
| Marker 1.10.1 | 83.8 | 66.8 | 72.9 | 33.5 | 86.6 | 80.0 | 85.7 | 99.3 | **76.1±1.1** |
| DeepSeek-OCR | 77.2 | 73.6 | 80.2 | 33.3 | 96.1 | 66.4 | 79.4 | 99.8 | **75.7±1.0** |
| MinerU 2.5.4 | 76.6 | 54.6 | **84.9** | 33.7 | 96.6 | 78.2 | 83.5 | 93.7 | **75.2±1.1** |
| **Mistral OCR API** | 77.2 | 67.5 | 60.6 | 29.3 | 93.6 | 71.3 | 77.1 | 99.4 | **72.0±1.1** |
| Nanonets-OCR2-3B | 75.4 | 46.1 | 86.8 | 40.9 | 32.1 | 81.9 | 93.0 | 99.6 | **69.5±1.1** |

Four things to take from that table:

1. **The "Old scans" column is brutal for everyone** — the best score is 50.4. Degraded scanned documents remain
   genuinely hard. Do not promise clean text from a 1990s scanned minutes packet.
2. **Local open models beat the hosted API.** olmOCR at 82.4 and Chandra at 83.1 beat Mistral OCR API at 72.0.
   Mistral's Tables score of 60.6 is the worst on the board among serious systems, which matters a lot for agenda
   packets. **Paying for an API does not buy better quality here.**
3. **Marker at 76.1 (this leaderboard) vs. 76.0 balanced-mode (Marker's own PyPI page)** — the two independent
   sources agree, which is a good sign for both.
4. Header/footer suppression varies wildly (32.1 to 97.0). For a change-tracking corpus this matters: page furniture
   that drifts between versions creates false diffs.

**Cost and hardware:**

| Option | Cost | Hardware |
|---|---|---|
| [olmOCR](https://github.com/allenai/olmocr) (Apache-2.0, 7B) | "less than $200 USD per million pages converted" (self-hosted) | "Recent NVIDIA GPU (tested on RTX 4090, L40S, A100, H100) with at least 12 GB of GPU RAM", 30 GB disk |
| [Mistral OCR API](https://mistral.ai/pricing/api) | **"OCR $4 / 1000 pages"** ($4,000/M); Document AI "$5 / 1000 pages" | None |
| Surya | free (weights licence permitting) | 5 pages/s on RTX 5090 |
| Marker | free (code) | 2.9 pages/s balanced GPU, 7.4 fast GPU |

olmOCR's own $200/M figure is ~20x cheaper than Mistral's $4,000/M **and scores 10 points higher**. That is a
strong argument against the API for bulk work — but it assumes you own or rent the GPU. For a self-hosted Centinel
running on a VPS with no GPU, the comparison flips: the API needs no capex and no ops.

**Is an API dependency tolerable for an unattended scheduled tool?** Arguments against, specific to this project:
- **It is a `.gov` archival tool.** Sending public records to a third party is defensible; the more serious problem
  is that the archive's *reproducibility* now depends on a vendor's model version. Re-running extraction in 2028
  against `mistral-ocr-4` may not produce the 2026 output, which undermines the "retain every version, track change
  over time" premise. A silent model upgrade would look like document churn.
- **Unattended + metered = unbounded cost.** A crawler that finds a 4,000-page packet costs $16 in one call. A
  misconfigured recrawl costs real money with no human in the loop.
- **Availability and rate limits** become crawl failures.

Arguments for: zero setup, zero GPU, no model weights to ship, and it is genuinely good at multi-column and
headers. **The defensible design is a pluggable OCR backend** — Tesseract as the always-available default, a local
VLM for quality when a GPU exists, and an API as an explicit opt-in — with the chosen backend and its version
recorded in the store alongside the extracted text, so provenance survives a backend swap.

### 3.4 Reading order and table structure

This is the axis that actually separates the options, more than raw character accuracy.

**Bag-of-words (coordinates but no structure)** — you must rebuild reading order yourself:
- Tesseract plain-text output, `tesseract.js`, EasyOCR, `ocrs`
- `pdf-extract` (`PlainTextOutput`), `pdf-rs`
- PDF.js `getTextContent()` and everything built on it (`unpdf`, `pdf-parse`)

Tesseract is a partial exception: with `--psm 1` (automatic page segmentation with OSD) plus hOCR output it emits
a block/paragraph/line/word hierarchy, which is a real reading-order signal. It is unreliable on complex
multi-column layouts, which is exactly where you need it.

**Reading-order aware:**
- `pdftotext -layout` (poppler) — old, fast, and surprisingly strong on two-column government text
- `pymupdf4llm` — "handles multi-column layouts" (AGPL)
- `docling` — "page layout, reading order, table structure"
- Surya — explicit reading-order detection model
- MinerU — "output following natural reading order with automatic header/footer removal"
- olmOCR — "natural reading order, even in the presence of figures, multi-column layouts, and insets"

**Table structure (emits an actual grid, not cell soup):**
- `pdfplumber` — ruling-line and word-alignment strategies, MIT, born-digital only. Best non-ML option.
- `docling` — TableFormer model
- MinerU — table→HTML
- Surya — row/column recognition
- Marker / olmOCR / Chandra — markdown or HTML tables (84.9–88.0 on olmOCR-Bench)
- `pdf-parse` v2 `getTable()` — claimed, unbenchmarked
- **Rust: nothing.** No Rust crate does table structure recognition.

---

## 4. Deciding when to OCR

There is no single reliable heuristic. Mature pipelines all use a **layered** decision, and every one of them
operates **per page**, not per document. That per-page granularity is the most important finding in this section.

### 4.1 What mature pipelines actually do

**OCRmyPDF** (MPL-2.0 — its README: "This license permits integration of OCRmyPDF with other code, included
commercial and closed source, but asks you to publish source-level modifications you make to OCRmyPDF") is the most
battle-tested pipeline here — "battle-tested on millions of PDFs" — and its whole design is about this decision.
Its default is conservative:
> "If a page in a PDF seems to have text, by default OCRmyPDF will exit without modifying the PDF."

Three explicit modes, and the distinctions are exactly the ones that bite:

- `--skip-text` (`--mode skip`): "no image processing or OCR will be performed on pages that already have text.
  The page will be copied to the output." **This is the mixed-document answer** and the docs say so directly — it is
  the recommended mode "for documents containing both scanned and digital pages".
- `--redo-ocr` (`--mode redo`): distinguishes **visible** text (real born-digital text) from **invisible** text (a
  previous OCR layer someone else added). It strips the invisible layer, masks the visible text, rasterises, OCRs
  the remainder, and merges. The docs: "If a file contains a mix of text and bitmap images that contain text,
  OCRmyPDF will locate the additional text in images without disrupting the existing text." This handles the very
  common `.gov` case of a scanned document that already went through some agency's low-quality OCR.
- `--force-ocr` (`--mode force`): rasterises everything, "discard[s] any hidden OCR text, rasteriz[es] any printable
  text, and flattens form fields." Destructive; the escape hatch for garbage text layers.

**`unstructured`** implements the fallback pattern in its `auto` strategy: choose `fast` (pdfminer text layer) for
"PDFs with readable text", falling back to `ocr_only` "when text extraction fails". Its own docs state: "If the PDF
text is not extractable, `partition_pdf` will fall back to `ocr_only`." Also worth noting: its docs say `hi_res`
"struggles with multi-column layouts" and recommends `ocr_only` for "multi-column documents lacking extractable
text" — i.e. even *with* layout detection, columns are hard.

**`pymupdf4llm`** has the most sophisticated published heuristic (AGPL, so read it as a *design reference* rather
than a dependency). Its "hybrid OCR strategy" inspects each page and triggers on two distinct conditions:
1. The page "contains roughly no text but is covered with images or many character-sized vectors" — and it then
   assesses whether text is probably detectable, explicitly "to distinguish image-based text (e.g. a scanned
   document) from ordinary pictures like photographs."
2. The page contains text but "too many characters are unreadable (e.g. `"�����"`)" — in which case it OCRs
   "the affected text areas only, not the full page."

Default is `use_ocr=True` with `force_ocr=True` as an override, and "pages that contain native text only are never
sent through OCR". They claim this "reduces OCR processing time by around 50% while improving recognition accuracy."

**`docling`** exposes a related but different knob: `bitmap_area_threshold`, default **0.05** — skip OCR on bitmap
regions smaller than 5% of page area, so logos and icons do not get OCR'd. With `do_ocr=True` (the default) it OCRs
bitmap regions; `force_full_page_ocr=True` on any `OcrOptions` subclass forces whole-page OCR regardless of text
layer. Engine options are pluggable: `EasyOcrOptions`, `TesseractOcrOptions` (libtesseract),
`TesseractCliOcrOptions` (subprocess, with `tesseract_cmd`), `RapidOcrOptions`, `OcrMacOptions` (native macOS
Vision), plus `OcrAutoOptions` for automatic engine selection and `KserveV2OcrOptions` for remote inference.

### 4.2 The heuristics, ranked by reliability

1. **Extracted character count per page** — the baseline. Extract the text layer, count characters. A near-zero
   count on a page with a large image is a scan. Threshold in practice is a small number (tens of characters, not
   zero) because scanned pages often carry a header, a stamp, or a Bates number as real text. **Failure mode:** a
   page that is genuinely near-empty (a section divider, a blank signature page) trips the same condition and gets
   pointlessly OCR'd. Cheap enough to not care.

2. **Unicode-replacement / undecodable character ratio** — the one most people miss and the one that matters most
   for `.gov`. A PDF with a broken or absent `ToUnicode` CMap extracts a *high* character count of complete garbage.
   Character-count heuristics pass it; the index gets poisoned with junk that is worse than nothing because it is
   invisible in a "did we extract text?" check. `pymupdf4llm`'s `"�����"` condition is the published version of
   this. Any implementation should compute a replacement-char / non-printable ratio per page and treat a high ratio
   as "needs OCR".

3. **Image coverage ratio** — what fraction of the page area is covered by raster images. High coverage plus low
   text is a strong scan signal; it is also what distinguishes a scanned page from a photo-heavy brochure page,
   which is precisely the distinction `pymupdf4llm` calls out. `docling`'s `bitmap_area_threshold` is the inverse
   application of the same measurement.

4. **Presence of font resources** — a page with no `/Font` in its resource dictionary cannot have a text layer.
   Cheap and definitive as a *negative* test. Useless as a positive test: a scanned page with an invisible OCR
   layer has fonts.

5. **Invisible-text detection (`Tr 3` render mode)** — text drawn in PDF text-rendering mode 3 is invisible, which
   is how OCR layers are stored. Detecting it tells you the text is *someone else's OCR output*, not original
   digital text. This is what drives OCRmyPDF's `--redo-ocr`. For a corpus that cares about fidelity, knowing "this
   text came from an unknown upstream OCR of unknown quality" is provenance worth recording.

6. **A confidence check on the result** — after OCR, mean word confidence (Tesseract TSV gives per-word `conf`).
   Low mean confidence on a page means the rendition is unreliable and should be flagged rather than silently
   indexed.

### 4.3 Mixed documents

Government agenda packets are the worst case for this: a 400-page PDF assembled from a born-digital staff report,
a scanned signed resolution, a scanned map, an exported spreadsheet, and public-comment emails that were printed
and re-scanned. Every published pipeline handles this the same way — **decide per page, not per document** — and
several say so explicitly (OCRmyPDF's `--skip-text` "works well for documents combining both digital and scanned
content"; `pymupdf4llm` inspects "each page"; `docling` operates on bitmap regions within a page).

Two refinements worth carrying into Centinel's design:

- **Sub-page granularity.** `pymupdf4llm` OCRs "the affected text areas only, not the full page", and `docling`
  OCRs bitmap *regions*. A born-digital page containing one scanned exhibit image should get OCR on the image and
  keep the native text elsewhere. Whole-page OCR of such a page is a quality regression, because rasterising good
  text and re-recognising it is strictly worse than reading it.
- **Record the decision.** Store, per page, which path was taken (native / OCR / hybrid), which engine and version,
  and the confidence. This is not bookkeeping for its own sake: without it, a re-crawl that flips a page from
  native to OCR produces a large text diff that looks like the document changed when it did not. For a system whose
  purpose is tracking change over time, **extraction-method churn is a correctness bug**, and the only defence is
  recording the method.

### 4.4 Availability of these heuristics per language

Every heuristic above requires the same three primitives: per-page text with characters, per-page image/resource
inventory, and the ability to rasterise a page to a bitmap.

- **Python:** all three available from `pypdf`/`pdfminer.six`/`pdfplumber` (MIT/BSD), rasterisation via
  `pypdfium2` (Apache/BSD) or poppler. Fully implementable on clean licences.
- **TypeScript:** PDF.js gives text items, operator lists (so image XObjects are enumerable), and page rendering to
  canvas via `@napi-rs/canvas`. All three primitives available, Apache-2.0. Implementable.
- **Rust:** `lopdf`/`pdf-extract` give text and can walk resource dictionaries; **rasterisation is the gap.** Pure
  Rust has no production PDF renderer — `pdf_render` (pdf-rs + Pathfinder) exists but is a separate, less-mature
  repo. In practice, rasterising a page in Rust means `pdfium-render` (native lib) or shelling out to `pdftoppm`.
  Since OCR *requires* rasterisation, **Rust cannot do OCR at all without one of those two.**

---

## 5. Provenance and anchoring

Treating this as a hard requirement — a search hit must be citable to *page N of document D* — eliminates several
otherwise-attractive options outright.

### 5.1 What each option can actually anchor

| Option | Page number | Bounding box | Char offset | Notes |
|---|---|---|---|---|
| `pdfplumber` | **Yes** | **Yes, per char** | derivable | Best-in-class, see below |
| `pdfminer.six` | Yes | Yes, per char | derivable | pdfplumber's engine |
| `docling` | **Yes** | **Yes** | **Yes, native** | `ProvenanceItem` — see below |
| `PyMuPDF` / `pymupdf4llm` | Yes | Yes | Yes (JSON mode) | AGPL |
| `pypdf` | Yes (per-page API) | No | No | Text-layer dump only |
| Tesseract (TSV/hOCR/ALTO) | **Yes** | **Yes, per word + confidence** | No | See below |
| `tesseract.js` | Yes | Yes (`blocks`, `hocr`) | No | Same data as native |
| PDF.js `getTextContent()` | Yes (per-page call) | **Derivable from `transform`** | No | You compute the box |
| `pdf-parse` v2 | Yes (`partial: [n]`) | **Not documented** | No | |
| `@opendocsg/pdf2md` | No | No | No | **Fails the requirement** |
| `pdf-extract` | Yes (`extract_text_by_pages`) | Internally yes, **not exposed via `PlainTextOutput`** | No | See below |
| `pdfium-render` | Yes (`PdfPageIndex`) | Yes (character bounds) | No | Native lib |
| `marker` / `olmOCR` / `mineru` | Yes (JSON output) | Varies | No | Markdown output loses it |
| Mistral OCR API | **Yes** (per-page index + dimensions) | **Yes** (paragraph-level, `include_blocks`) | No | Plus word/page confidence |
| `ocrs` | Yes (you supply the page) | Yes (line/word boxes) | No | |
| EasyOCR | n/a (image in) | Yes | No | |

### 5.2 The strong options, in detail

**`docling` has the cleanest model of the lot.** `docling-core` defines provenance as a first-class type:

```python
class ProvenanceItem(BaseModel):
    """Provenance information for elements extracted from a textual document.

    A `ProvenanceItem` object acts as a lightweight pointer back into the original
    document for an extracted element. It applies to documents with an explicit
    or implicit layout, such as PDF, HTML, docx, or pptx.
    """
    page_no: Annotated[int, Field(description="Page number")]
    bbox: Annotated[BoundingBox, Field(description="Bounding box")]
    charspan: CharSpan
```
— [`docling_core/types/doc/common/reference.py`](https://github.com/docling-project/docling-core/blob/main/docling_core/types/doc/common/reference.py)

That is page number **plus** bounding box **plus** character span, attached to every extracted element, and the
docstring says it applies "to documents with an explicit or implicit layout, such as PDF, HTML, docx, or pptx" —
so the same anchoring model covers §6's other formats. `BoundingBox` carries an explicit `coord_origin`
(`CoordOrigin`), which matters because PDF's native origin is bottom-left while every image and rendering
convention is top-left; getting this wrong silently flips every highlight vertically.

**`pdfplumber`** exposes per-character properties that make anchoring trivial — from its README:
`page_number`, `text`, `fontname`, `size`, `adv`, `upright`, `height`, `width`, `x0`, `x1`, `y0`, `y1`, `top`,
`bottom`, `doctop`, `matrix`, `mcid`, `tag`, `stroking_color`, `non_stroking_color`, `object_type`.

Three of those are unusually valuable for this project:
- **`doctop`** — "Distance of top of character from top of document", i.e. a *document-global* vertical coordinate.
  Anchoring across page boundaries becomes one number.
- **`mcid` / `tag`** — marked content section ID and tag. In **tagged** PDFs (which many federal agencies produce,
  because Section 508 accessibility requirements push them to) these give you the *logical* structure the author
  declared: heading, paragraph, table, artifact. That is a free, authoritative reading-order signal that no ML model
  has to guess at. Nothing else surveyed exposes this as plainly.
- **`upright`** — trivially filters rotated stamps and sidebar text out of the main flow.

**Tesseract's TSV output** is the anchoring answer for OCR'd pages. Columns, verbatim from the docs:
> `level   page_num        block_num       par_num line_num        word_num        left    top     width   height  conf    text`

Page, a four-level structural hierarchy (block → paragraph → line → word), a bounding box, **and a per-word
confidence**. hOCR gives the same via `bbox` and `x_wconf` attributes. `tesseract.js` exposes the equivalent through
`hocr: true` and its `blocks` output. So OCR'd content can be anchored *and quality-scored* per word — you can, for
instance, refuse to index a page whose mean confidence is below a threshold, or surface a "low-confidence OCR"
warning next to a search hit.

**PDF.js** gives you a `TextItem` per run, typed as:
> `str` (text content), `dir` (`'ttb'`/`'ltr'`/`'rtl'`), `transform` (transformation matrix), `width` and `height`
> (device space), `fontName`, `hasEOL` — [`src/display/api.js`](https://github.com/mozilla/pdf.js/blob/master/src/display/api.js)

The bounding box is not handed to you but is recoverable: `transform[4]`/`transform[5]` are x/y, combined with
`width`/`height`. So TypeScript **can** satisfy the requirement, but you write the geometry code, and you must
handle the origin flip yourself.

**Mistral OCR** is the only API surveyed that returns structured provenance: per-page indices and dimensions,
paragraph-level bounding boxes via `include_blocks`, and "confidence scores ... at word or page granularity".

### 5.3 The options that fail the requirement

- **`@opendocsg/pdf2md`** emits a markdown string. No page, no box, no offset. If provenance is a requirement, this
  is disqualified as anything but a last-resort formatter.
- **`pdf-extract`'s `PlainTextOutput`** — the crate carries `MediaBox` and uses `euclid` internally, and
  `extract_text_by_pages()` gives you page granularity, but the plain-text path throws the geometry away. Getting
  boxes out means implementing your own `OutputDev`. That is a real, tractable amount of Rust work, not a blocker,
  but it is work nobody else has to do.
- **Any markdown-only output** — this is the structural trap. `marker`, `mineru`, `olmOCR` and `pymupdf4llm` all
  *have* provenance in their JSON output and all *lose* it in their markdown output. **Extract to the structured
  format and derive markdown from it; never extract straight to markdown.** See §7.

### 5.4 Design implication

Store the structured representation (page → blocks → spans, with boxes and confidences) as the canonical
extraction artifact, and generate markdown from it as a *view*. If markdown is the only stored rendition, every
search hit degrades to "somewhere in this 400-page PDF", and the chunk-to-page mapping cannot be reconstructed
later without re-extracting — which, given model and library drift, may not even reproduce.

---

## 6. Other document formats

### 6.1 Coverage per language

| Format | Rust | Python | TypeScript |
|---|---|---|---|
| DOCX | `docx-rs`, `dotext` (thin); `extractous` (Tika) | `python-docx`, `docling`, `unstructured`, `markitdown`, `mammoth` | `mammoth`, `docx4js` (both thin) |
| XLSX | `calamine` (**good**) | `openpyxl`, `pandas`, `docling`, `markitdown` | `sheetjs`/`xlsx` (**good**, licence-check) |
| CSV | `csv` (**excellent**) | `csv`, `pandas` | `papaparse`, `csv-parse` |
| PPTX | `extractous` only | `python-pptx`, `docling`, `unstructured`, `markitdown` | none serious |
| Images | `image` crate (**excellent** decode); OCR via `ocrs` | Pillow + any OCR | `sharp`/`jimp`; OCR via `tesseract.js` |
| Legacy `.doc`/`.xls` | `extractous` (Tika) | `unstructured` (via LibreOffice) | none |

**`calamine`** is the one place Rust clearly wins: a fast, pure-Rust reader for XLS/XLSX/XLSB/ODS with no native
dependency and no Excel install. Nothing in the survey beats it on deployment simplicity for spreadsheets.
Rust's `csv` crate and `image` crate are similarly best-in-class. Rust's weakness is concentrated in PDF and
office-document *understanding*, not in file parsing generally.

### 6.2 Does one library cover the spread?

Four candidates, and the answer differs sharply by language.

**`docling` (Python, MIT)** — the most complete single answer. Inputs: "PDF, DOCX, PPTX, XLSX, HTML, EPUB, WAV,
MP3, WebVTT, Box Notes, email formats (EML, MSG), images (PNG, TIFF, JPEG, ...), LaTeX, DocLang, plain text",
plus video and ODF and XBRL. One `DoclingDocument` type, one provenance model, one markdown exporter across all of
them. Hosted in the LF AI & Data Foundation, started by IBM Research Zurich. For Centinel's requirements this is
the closest thing to a drop-in answer that exists in any language.

**`unstructured` (Python, Apache-2.0)** — 60+ types, but the coverage is purchased with system packages:
`libmagic-dev`, `poppler-utils`, `tesseract-ocr`, `libreoffice`, `pandoc`. Note what `libreoffice` implies: a
headless office suite in your container image, hundreds of MB, and a conversion step that occasionally hangs. It
is how you get legacy `.doc`/`.xls`/`.ppt`, which nothing else here handles natively.

**`markitdown` (Python, MIT)** — MS Office formats plus Outlook, audio transcription and YouTube, all to markdown,
with almost no dependencies. Its PDF path is weak (pdfminer.six, no OCR, no layout). The sensible read is
**markitdown for the non-PDF long tail, something else for PDFs** — and note that its YouTube handling overlaps
Centinel's YouTube-channel requirement.

**Apache Tika** — the veteran; 1,000+ formats, Apache-2.0, and the only realistic "all formats" answer for Rust or
TypeScript. Two integration modes:
- **Tika Server** — run the JAR, POST files, get text/metadata back over HTTP. Language-agnostic. Costs you a JVM
  in the deployment.
- **[`extractous`](https://crates.io/crates/extractous)** (Rust, Apache-2.0) — Tika compiled to a native image via
  GraalVM, callable from Rust with no JVM at runtime. Genuinely clever, and the right answer if Rust is chosen. But
  its last release is **0.3.0 on 2024-12-21** — roughly 19 months stale as of this writing — and it is a
  single-vendor project (yobix-ai). Adopting it means accepting maintenance risk on a load-bearing component.

**TypeScript has no equivalent.** There is no Node "all formats" library. The options are per-format packages
(`mammoth`, `xlsx`, `papaparse`) plus Tika Server over HTTP.

### 6.3 The honest framing

For everything *except* PDF, all three languages are adequate — these are structured formats (XML in a zip, mostly)
and parsing them is not hard. **The entire difficulty of this project is concentrated in PDF and OCR.** Do not let
DOCX/XLSX/CSV support influence the language decision; it should not carry weight.

---

## 7. Output to markdown

### 7.1 Who emits markdown directly

| Option | Markdown? | Quality |
|---|---|---|
| `docling` | **Yes, native** | Headings, tables, lists, formulas, images; from a structured doc model |
| `pymupdf4llm` | **Yes** — "GitHub-compatible Markdown with headings, bold, italic, monospace formatting, code blocks, tables, image references, and lists" | Excellent — **but AGPL** |
| `marker` | **Yes** | Markdown / JSON / HTML / chunks |
| `mineru` | **Yes** | Markdown + JSON, formula→LaTeX, table→HTML |
| Mistral OCR API | **Yes** — "Returns results in markdown format for easy parsing and rendering" | Good, weak tables (60.6) |
| olmOCR | **Yes** | Markdown with tables and equations |
| `markitdown` | **Yes** | Good for Office, weak for PDF |
| `@opendocsg/pdf2md` | **Yes** | Heuristic; font-size→heading guessing, no tables |
| `pdfplumber` | No | Text + structured tables; you write the serialiser |
| `pypdf`, `pdfminer.six` | No | Plain text |
| `pdf-extract` | No (HTML/SVG, not markdown) | `HTMLOutput` → markdown is a second hop |
| `pdfjs-dist` / `unpdf` / `pdf-parse` | No | Text items; you write everything |
| Tesseract | No (text/hOCR/ALTO/TSV) | hOCR → markdown is a second hop |
| `ocrs`, EasyOCR | No | Lines and boxes |

**Rust emits markdown from nothing.** Not one crate surveyed produces markdown from a PDF. `pdf-extract`'s
`HTMLOutput` is the closest starting point, and HTML→markdown in Rust is a solved problem
(`htmd`, `html2md`), so the path is HTML → markdown, with the caveat that `HTMLOutput`'s HTML is
positional rather than semantic — it will not give you `<h2>` and `<table>`, so the markdown will not have real
headings or tables either.

### 7.2 The trap worth naming

Markdown is lossy. The instant an extractor emits markdown, page boundaries, bounding boxes, and confidence scores
are gone. Since §5 makes provenance a requirement, **markdown must be a rendering of a structured artifact, not the
extraction output itself.** Concretely: extract to a structured representation (docling's `DoclingDocument`,
Mistral's page/block JSON, marker's JSON mode, or your own page→block→span type), persist that, and serialise
markdown from it. The extractors that offer both a JSON and a markdown mode — `docling`, `marker`, `mineru`,
`pymupdf4llm`, Mistral — should always be used in JSON mode.

A second, subtler point for a system that diffs versions over time: **markdown serialisation must be
deterministic.** If the serialiser's heading-level heuristics or table-alignment padding shift between library
versions, every document in the corpus appears to change at once. Pin the serialiser, version the rendition
format, and store the serialiser version alongside the output.

Two structural elements need an explicit policy, because they have no good markdown representation:
- **Tables** — GitHub-flavoured markdown pipe tables cannot express merged cells, which appear constantly in
  government budget and agenda tables. Options are HTML tables embedded in the markdown (ugly but lossless) or
  accepting the flattening. `mineru` chose table→HTML for exactly this reason.
- **Multi-page tables** — a budget table spanning 12 pages has to either be split at page boundaries (breaking the
  table) or joined across them (breaking page provenance). Pick one deliberately.

---

## 8. Quality comparison / benchmarks

Two credible published benchmarks exist. Both are Python-ecosystem artefacts. **Neither includes a single Rust or
TypeScript tool.**

### 8.1 olmOCR-Bench (AI2)

[Dataset](https://huggingface.co/datasets/allenai/olmOCR-bench) · [code](https://github.com/allenai/olmocr/tree/main/olmocr/bench) ·
[paper: *olmOCR 2: Unit Test Rewards for Document OCR*, arXiv:2510.19817](https://arxiv.org/abs/2510.19817)

**1,403 PDF files and 7,010 unit test cases.** It deliberately rejects CER/WER in favour of pass/fail assertions:
> "All facts checked about documents are either pass/fail. We want it to be very clear if your OCR system fails a test."

Seven categories (arXiv math, old scans math, tables, old scans, headers/footers, multi-column, long tiny text) and
five test classes (text presence, text absence, natural reading order, table accuracy, math formula accuracy).
Old-scan material is drawn from the Library of Congress. Test cases were built by combining "manual design and
review with prompting GPT-4o".

Full leaderboard is reproduced in §3.3. The three numbers that matter for `.gov`:

- **Best "Old scans" score on the board is 50.4** (Chandra). Every system is at or near coin-flip on degraded
  scanned documents. This is the single most important calibration fact in this document: **a corpus of scanned
  1990s municipal records will not be cleanly searchable with any tool available today.**
- **Best "Multi column" is 84.2** (Infinity-Parser), with olmOCR 83.7 and Marker 80.0. Multi-column is solved-ish.
- **Best "Tables" is 88.0** (Chandra); Mistral OCR API is 60.6, the worst of the serious systems.

Caveat to state plainly: AI2 publishes this leaderboard and olmOCR is AI2's system. It scores third, behind
Chandra and Infinity-Parser, which is a point in favour of the leaderboard's honesty, but self-published
benchmarks always warrant a discount.

### 8.2 OmniDocBench (OpenDataLab)

[github.com/opendatalab/OmniDocBench](https://github.com/opendatalab/OmniDocBench)

**1,651 PDF pages, 10 document types, 5 layout types, 5 languages** — including academic papers, financial reports,
newspapers, and handwritten notes. Annotations cover "28 block-level and 4 span-level document elements" plus
reading-order annotations and attribute tags. Evaluates end-to-end parsing, layout detection, table recognition,
formula recognition, and text OCR, using normalized edit distance, BLEU, METEOR, TEDS, and COCODet. Composite
metric:
> `(1−Text Edit Distance)×100 + Table TEDS + Formula CDM) / 3`

Headline results:

| Model | Type | Overall |
|---|---|---|
| PaddleOCR-VL-1.6 | Specialized VLM | **96.34** |
| MinerU2.5-Pro | Specialized VLM | 95.75 |
| GLM-OCR | Specialized VLM | 95.22 |
| Gemini 3 Pro | General VLM | 92.91 |
| Marker | Pipeline Tool | 78.44 |

Its own conclusion: "Specialized vision-language models substantially outperform general VLMs and traditional
pipeline tools on document parsing tasks."

Two things to take from this:
1. **Purpose-built document VLMs beat general-purpose frontier VLMs.** Gemini 3 Pro at 92.91 loses to PaddleOCR-VL
   at 96.34. Reaching for a general vision model because it is already in your stack is not the quality choice.
2. **The pipeline-vs-VLM gap is large** (78.44 vs 96.34) — much larger than olmOCR-Bench's spread suggests, because
   OmniDocBench weights tables and formulas heavily. Different benchmarks, different orderings; do not treat either
   as definitive.

Note the disagreement between benchmarks: Marker is competitive on olmOCR-Bench (76.1, mid-pack) and last on
OmniDocBench (78.44 vs 96.34). Benchmark choice materially changes the conclusion.

### 8.3 Where benchmarks do not exist — say so

No published benchmark data was found for any of the following. These are stated as gaps, not as poor performance:

- **`pdf-extract`, `lopdf`, `pdf-rs`** — no accuracy benchmarks of any kind.
- **`ocrs`** — no accuracy numbers published. Its README says only "Expect more errors than commercial OCR engines."
  Nobody has measured how many.
- **`pdfium-render` / PDFium text extraction quality** — unbenchmarked in this survey's sources.
- **PDF.js `getTextContent()` extraction accuracy** — no published benchmark. Widely used, never measured in these
  terms.
- **`pdf-parse` v2's `getTable()`** — claimed in the README, no evidence.
- **`@opendocsg/pdf2md` markdown quality** — no evaluation.
- **`pdfplumber` vs `pdfminer.six` vs `pypdf` on born-digital text accuracy** — surprisingly, no authoritative
  head-to-head. The benchmark ecosystem has jumped straight to the OCR/VLM tier and skipped the boring case.
- **`pdftotext -layout` reading-order accuracy** — no modern benchmark, despite being a reasonable baseline.
- **Anything on `.gov` documents specifically.** Neither benchmark includes municipal agenda packets, meeting
  minutes, or public-comment compilations. The "old scans" and "tables" categories are the closest proxies.

**Recommendation regardless of language:** build a small internal benchmark — 30–50 real documents pulled from
target `.gov` domains, spanning born-digital reports, scanned resolutions, dense budget tables, and a mixed agenda
packet — with hand-checked assertions in the olmOCR-Bench style (this string must appear; this header must not;
these two cells must be in the same row). It is a day of work and it is the only evidence that will actually
apply to this corpus.

---

## 9. What this means for the language decision

Not picking. Laying out the tradeoffs the evidence supports.

### 9.1 The compressed version

| | Rust | Python | TypeScript |
|---|---|---|---|
| Text-layer extraction | Adequate (`pdf-extract`) | Excellent | Adequate (PDF.js) |
| Layout / reading order | **None pure-Rust** | Excellent | None built-in |
| Table structure | **None** | Excellent (`pdfplumber`, `docling`) | **None** (unproven claim only) |
| OCR | Weak (`ocrs` preview, English-only) or FFI | Excellent, every option | Good (`tesseract.js`, WASM) |
| Page rasterisation (OCR prerequisite) | **Requires native lib or subprocess** | `pypdfium2` | `@napi-rs/canvas` |
| VLM document models | **None** | All of them | **None** |
| Markdown output | **None** | Several | One heuristic tool |
| Provenance (page/bbox/offset) | Possible, hand-rolled | Native (`docling`, `pdfplumber`) | Possible, hand-rolled |
| Other formats | `calamine` excellent; `extractous` (stale) | `docling`/`unstructured` cover all | Per-format packages |
| Licence cleanliness | Good (avoid `mupdf`) | Good (avoid PyMuPDF) | Good (avoid `mupdf`) |
| Deployment | Static binary — **until you need PDFium** | Python + wheels + optional GB of weights | Node runtime; WASM = no native deps |

### 9.2 If Rust

**The operator's suspicion is correct: the ecosystem is weaker, and materially so.** Concretely, choosing Rust means:

- No table structure extraction. At all. You would write it, from `lopdf` primitives.
- No layout or reading-order model. Multi-column government documents come out interleaved unless you write the
  geometry.
- No markdown generation. `pdf-extract`'s `HTMLOutput` → HTML→markdown is the nearest path, and its HTML is
  positional, not semantic.
- OCR requires rasterisation, and **pure Rust cannot rasterise a PDF in production**. So OCR in Rust means
  `pdfium-render` (ship `libpdfium` per platform — note it pulls `libloading` on non-WASM targets precisely to
  dlopen the native library) or `pdftoppm` as a subprocess.
- The only native Rust OCR engine, `ocrs`, self-describes as "an early preview", is Latin-alphabet only, and has
  no published accuracy numbers.
- The AGPL trap is present here too: `mupdf-rs` is the most capable Rust binding and is AGPL-3.0.

What Rust *does* buy: a single static binary for the crawler, `calamine` and `csv` and `image` are best-in-class,
and the crawl/store/hash/diff half of Centinel — which is most of the codebase — is a natural Rust fit.

The honest architecture if Rust is chosen: **Rust for crawl, store, hashing, versioning, diffing, and search; a
separate extraction worker for PDFs.** That worker is either (a) subprocess calls to `pdftotext`/`pdftoppm`/
`tesseract` — licence-safe per §2.4, three binaries, well-understood — or (b) a Python sidecar running `docling`,
talking over a queue or HTTP. Pretending Rust can do this in-process without a native dependency is the thing to
avoid; that is the claim this research does not support.

### 9.3 If Python

Everything in this document is available, most of it MIT or Apache-2.0, and `docling` alone satisfies §1, §3, §4,
§5, §6, and §7 in a single MIT dependency with an MCP server already in the box. The whole benchmark ecosystem is
here. Every OCR option, classical and VLM, local and hosted, is here.

Costs: a Python runtime and dependency tree in the deployment; multi-GB model weights if you use the ML tier;
optional GPU; and `docling`/`marker`/`mineru` are heavier processes than a Rust or Node worker. The one library you
would most want (`PyMuPDF`) is licence-blocked, which is a real loss but not a fatal one given `docling` exists.

### 9.4 If TypeScript

Genuinely good deployment story — no native dependencies anywhere on the critical path, `tesseract.js` puts OCR in
WASM with no `apt-get`, and one runtime for library, CLI, server, and MCP. Licences are clean (Apache-2.0/MIT
throughout, avoiding `mupdf`).

Costs: the ceiling is PDF.js's text layer. No layout model, no reading-order model, no table structure, no
markdown generation worth the name, no VLM tier, and OCR quality capped at Tesseract 5 (which the benchmarks show is
well below the current state of the art on scans). Anything beyond flattening a text layer is code you write, and
it is the same code Python already has.

### 9.5 The decision axes that actually matter

1. **Is OCR quality on scanned documents a core requirement or a nice-to-have?** If core, Python's advantage is
   decisive and hard to work around, because the entire VLM tier is Python-only and §8 shows classical OCR is
   ~30 points behind on old scans. If OCR is best-effort, all three are viable.
2. **Is table structure required?** Government budget and agenda tables are a large fraction of the value in this
   corpus. Only Python has an answer. Rust has nothing; TypeScript has an unproven claim.
3. **Is "one language, one binary, no native deps" a hard constraint?** Only TypeScript delivers it end-to-end
   including OCR. Rust delivers it right up until PDF rasterisation, then does not.
4. **Is a polyglot deployment acceptable?** If yes, the constraint dissolves — Rust or TypeScript for the crawler
   and store, a Python extraction worker, communicating over a queue. This is what the evidence actually
   recommends if the operator wants Rust, and it isolates the ecosystem gap to one replaceable component rather
   than letting it dictate the whole project.
5. **Subprocess tolerance.** If shelling out to `pdftotext`, `pdftoppm`, and `tesseract` is acceptable, Rust and
   TypeScript both become substantially more viable — and §2.4 shows the subprocess boundary is also what keeps
   GPL poppler licence-compatible with an MIT project. Three binaries in a container image is a modest, honest
   dependency. It is strictly less capable than `docling`, but it is not nothing, and it is a fraction of the
   operational weight of a PyTorch stack.

### 9.6 Constraints that hold regardless of language

- **AGPL is out.** No MuPDF binding in any language. This costs the project its single best tool and there is no
  way around it short of buying a commercial licence that forks cannot inherit.
- **Extract to a structured representation; render markdown from it.** Markdown-first extraction destroys the
  provenance that §5 requires and makes version diffing unreliable.
- **Decide OCR per page, not per document,** and prefer per-region where the extractor supports it.
- **Detect garbage text layers, not just missing ones.** A broken `ToUnicode` CMap produces high character counts
  of junk that every naive heuristic passes.
- **Record the extraction method, engine, version, and confidence per page.** Otherwise a backend change or a
  library upgrade manifests as corpus-wide false change — which for a change-tracking system is a correctness bug,
  not a cosmetic one.
- **Make the OCR backend pluggable.** Tesseract as the always-available floor; a local VLM when a GPU exists; an
  API strictly opt-in. §3.3 shows the API is neither the cheapest nor the most accurate option.
- **Build a `.gov`-specific test corpus.** No published benchmark covers this document class.

---

## Sources

All fetched 2026-08-02.

**Registries**
- crates.io API: [`pdf-extract`](https://crates.io/crates/pdf-extract) · [`lopdf`](https://crates.io/crates/lopdf) ·
  [`pdfium-render`](https://crates.io/crates/pdfium-render) · [`pdf`](https://crates.io/crates/pdf) ·
  [`mupdf`](https://crates.io/crates/mupdf) · [`extractous`](https://crates.io/crates/extractous) ·
  [`ocrs`](https://crates.io/crates/ocrs) · [`leptess`](https://crates.io/crates/leptess) ·
  [`rusty-tesseract`](https://crates.io/crates/rusty-tesseract) · [`calamine`](https://crates.io/crates/calamine)
- PyPI: [`pypdf`](https://pypi.org/project/pypdf/) · [`pdfminer.six`](https://pypi.org/project/pdfminer.six/) ·
  [`pdfplumber`](https://pypi.org/project/pdfplumber/) · [`PyMuPDF`](https://pypi.org/project/PyMuPDF/) ·
  [`pymupdf4llm`](https://pypi.org/project/pymupdf4llm/) · [`docling`](https://pypi.org/project/docling/) ·
  [`marker-pdf`](https://pypi.org/project/marker-pdf/) · [`unstructured`](https://pypi.org/project/unstructured/) ·
  [`markitdown`](https://pypi.org/project/markitdown/) · [`mineru`](https://pypi.org/project/mineru/) ·
  [`surya-ocr`](https://pypi.org/project/surya-ocr/) · [`easyocr`](https://pypi.org/project/easyocr/) ·
  [`paddleocr`](https://pypi.org/project/paddleocr/)
- npm: [`pdfjs-dist`](https://www.npmjs.com/package/pdfjs-dist) · [`pdf-parse`](https://www.npmjs.com/package/pdf-parse) ·
  [`unpdf`](https://www.npmjs.com/package/unpdf) · [`@opendocsg/pdf2md`](https://www.npmjs.com/package/@opendocsg/pdf2md) ·
  [`mupdf`](https://www.npmjs.com/package/mupdf) · [`node-poppler`](https://www.npmjs.com/package/node-poppler) ·
  [`tesseract.js`](https://www.npmjs.com/package/tesseract.js)

**Repository source and READMEs**
- [pdf-extract](https://github.com/jrmuizel/pdf-extract) · [docs.rs/pdf-extract](https://docs.rs/pdf-extract/latest/pdf_extract/)
- [pdfium-render](https://github.com/ajrcarey/pdfium-render) · [pdf-rs](https://github.com/pdf-rs/pdf)
- [ocrs](https://github.com/robertknight/ocrs) · [leptess](https://github.com/houqp/leptess)
- [pdfplumber](https://github.com/jsvine/pdfplumber) — char property table
- [pdf.js `src/display/api.js`](https://github.com/mozilla/pdf.js/blob/master/src/display/api.js) — `TextItem` typedef
- [pdf-parse v2](https://github.com/mehmet-kozan/pdf-parse) · [docling](https://github.com/docling-project/docling)
- [`docling_core/types/doc/common/reference.py`](https://github.com/docling-project/docling-core/blob/main/docling_core/types/doc/common/reference.py) — `ProvenanceItem`
- [tesseract-ocr/tesseract](https://github.com/tesseract-ocr/tesseract) · [tesseract.js](https://github.com/naptha/tesseract.js)
- [OCRmyPDF](https://github.com/ocrmypdf/OCRmyPDF) · [olmocr](https://github.com/allenai/olmocr)
- [PDFium LICENSE](https://raw.githubusercontent.com/chromium/pdfium/main/LICENSE)

**Official documentation**
- [PyMuPDF licensing](https://pymupdf.readthedocs.io/en/latest/about.html)
- [pymupdf4llm](https://pymupdf.readthedocs.io/en/latest/pymupdf4llm/index.html) — selective OCR
- [OCRmyPDF advanced features](https://ocrmypdf.readthedocs.io/en/latest/advanced.html) — skip/redo/force modes
- [OCRmyPDF introduction](https://ocrmypdf.readthedocs.io/en/latest/introduction.html)
- [Docling pipeline options](https://docling-project.github.io/docling/reference/pipeline_options/)
- [Unstructured partitioning strategies](https://docs.unstructured.io/open-source/core-functionality/partitioning)
- [Tesseract CLI usage](https://tesseract-ocr.github.io/tessdoc/Command-Line-Usage.html) — TSV columns
- [Mistral OCR](https://docs.mistral.ai/capabilities/OCR/basic_ocr/) · [Mistral API pricing](https://mistral.ai/pricing/api)

**Benchmarks**
- [olmOCR-Bench dataset](https://huggingface.co/datasets/allenai/olmOCR-bench) ·
  [bench code](https://github.com/allenai/olmocr/tree/main/olmocr/bench) ·
  [olmOCR 2 paper, arXiv:2510.19817](https://arxiv.org/abs/2510.19817)
- [OmniDocBench](https://github.com/opendatalab/OmniDocBench)
## Correction: `pdf-inspector` (added on review, 2026-08-02)

**The research missed `firecrawl/pdf-inspector`, and it materially changes §1, §7, and §9.** It appears in `crawling-and-sitemaps.md` only as a name in Firecrawl's `Cargo.toml` dependency list; it was never evaluated on its own. Verified directly during review.

| Field | Value |
|---|---|
| Repo | <https://github.com/firecrawl/pdf-inspector> — 6,038 stars, MIT, **pure Rust** |
| Created / last push | 2026-02-06 / 2026-08-02 |
| Dependencies | `lopdf`, `ttf-parser`, `regex`, `unicode-normalization`, `thiserror`. **No FFI, no pdfium, no poppler, no ML models.** Compiles to WASM. |
| Published | crates.io `0.1.7` · npm `@firecrawl/pdf-inspector` `1.11.2` · PyPI `0.2.6` |

### Claims in this document that it falsifies

- ~~"Rust has zero table-structure extraction"~~ — `src/tables/` is ~580 KB across four independent strategies: ruled lines, rectangles, PDF structure tree, and text-alignment heuristics. Handles financial tables, footnotes, and **continuation tables across pages**.
- ~~"Rust has zero markdown generation"~~ — `src/markdown/` is ~295 KB. Headings via font-size ratios, lists, code blocks (monospace detection), tables, bold/italic, URL linking, page breaks.
- ~~"`pdf-extract`/`lopdf`/`pdf-rs` are text-layer-only and layout-naive"~~ — true of those crates, but `src/extractor/` includes `reading_order.rs`, `layout.rs`, and multi-column plus RTL detection.

### Published benchmark

opendataloader-bench (200 PDFs, Apple M4 Pro, refreshed 2026-07-31, reproducible results branch published):

| Engine | Overall | Reading order | **Tables** | **Speed** |
|---|---|---|---|---|
| **pdf-inspector** | **0.875** | **0.915** | **0.814** | **0.470s** |
| liteparse | 0.873 | 0.913 | 0.693 | 0.750s |
| pymupdf4llm | 0.735 | 0.886 | 0.401 | 17.117s |
| markitdown | 0.589 | 0.844 | 0.273 | 16.165s |

**It beats `pymupdf4llm` — the AGPL tool §2 disqualifies — on every axis, at 36× the speed, under MIT.**

### It independently implements two constraints this document derived

§4 argued that OCR routing must detect **garbage** text layers, not merely missing ones, and that **mixed documents** need per-page decisions. `pdf-inspector` does both:

- *"Encoding issue detection — automatically flags broken font encodings so callers can fall back to OCR"*, backed by a 110 KB `tounicode.rs` and a dedicated `text_quality.rs`.
- Returns **`pages_needing_ocr`** — specific page numbers, enabling **per-page OCR routing instead of all-or-nothing**. Classification runs in 10–50 ms by sampling content streams.

§5's provenance requirement is also served: extraction is position-aware, carrying font info and X/Y coordinates.

### What it does NOT change

**It does not do OCR and does not rasterise.** Explicitly: *"all without OCR."* It is a **classifier and router** — Firecrawl built it to skip OCR for *"the ~54% of PDFs that don't need them,"* which concedes ~46% still do.

So §1's hardest finding survives: **pure Rust still cannot rasterise a page, so Rust still cannot OCR without PDFium or a `pdftoppm` subprocess.**

### Revised conclusion for §9

Rust's PDF gap narrows from *"extraction, tables, markdown, layout, and OCR"* to **OCR alone** — and OCR is a subprocess or service call in **every** language, since §3 established that tesseract bindings carry the same deployment burden as shelling out. This is a materially stronger case for Rust than this document concluded.

### Caveats

- **Young and pre-1.0.** Repo ~6 months old; the crate was first published 2026-06-05, ~2 months ago.
- **Version fragmentation reveals the priority order:** npm is at **1.11.2 across 50 releases**; crates.io is at **0.1.7 across 8**. Firecrawl is a TypeScript shop, and the Rust crate is the least-released surface of their own Rust library — the same batch-port pattern seen in their SDKs. Mitigation: depend on the git repo rather than crates.io.
- The benchmark is **vendor-run**, though on a third-party corpus with published reproducible results.
- **The corpus is not `.gov` agenda packets.** §8's recommendation of a 30–50 document internal benchmark still stands — it now has a specific first candidate to test.

---

## Correction: `anydoc` (added on review, 2026-08-05)

**§6 is out of date, and the finding it drove — that office formats push the language choice toward Python — no longer holds.** `firecrawl/anydoc` did not exist when §6 was written; it was first published on crates.io three days before this correction. Verified directly against the source during review.

| Field | Value |
|---|---|
| Repo | <https://github.com/firecrawl/anydoc> — MIT, **pure Rust** |
| Published | crates.io `0.1.3` (2026-08-04) · npm `@firecrawl/anydoc` · PyPI `firecrawl-anydoc` |
| Dependencies | `calamine`, `cfb`, `csv`, `encoding_rs`, `flate2`, `pdf-inspector`, `quick-xml`, `zip`. **No ML models, no external services, no JVM, no LibreOffice.** |
| Formats | 14: `.doc` `.docx` `.docm` · `.ppt` `.pps` `.pot` `.pptx` `.pptm` `.ppsx` `.ppsm` · `.xls` `.xlsx` `.xlsm` `.xlsb` · `.odt` `.ods` `.odp` · `.rtf` · `.epub` · `.csv` · `.pdf` |

### Claims in §6.1 that it falsifies

The coverage table said Rust's options were thin bindings or a stale Tika port. Every row it touches is now wrong:

- ~~"DOCX: `docx-rs`, `dotext` (thin); `extractous` (Tika)"~~ — full WordprocessingML, Transitional and Strict.
- ~~"PPTX: `extractous` only"~~ — PresentationML including speaker notes.
- ~~"Legacy `.doc`/`.xls`: `extractous` (Tika)"~~ — read natively through `cfb`, with no LibreOffice and no GraalVM.

§6.2's conclusion that **"TypeScript has no equivalent"** and that `docling` was *"the closest thing to a drop-in answer that exists in any language"* is superseded for everything except PDF understanding and OCR: `anydoc` covers more formats than `docling` (14 against 4 in its own benchmark) in-process, with no Python runtime.

### Published benchmark

100 real-world documents, LLM judge (Claude Sonnet 5) scoring blind against LibreOffice-rendered ground truth, each pair judged twice with outputs swapped to cancel position bias:

| tool | formats | median ms | score |
|---|---|---|---|
| **anydoc** | **14/14** | **4.7** | **80** |
| unstructured | 8/14 | 572.9 | 65 |
| markitdown | 6/14 | 134.8 | 65 |
| pandoc | 5/14 | 102.1 | 57 |
| docling | 4/14 | 513.6 | 57 |
| libreoffice | 12/14 | 1129.5 | 40 |

Vendor-run, and the corpus is not redistributable — so unverifiable independently, exactly like the `pdf-inspector` benchmark above. The format coverage claim, unlike the scores, was verified from source.

### What it does NOT change

**It is not a better PDF reader, and adopting it for PDFs would be a regression.** Its PDF path calls the same `pdf_inspector::process_pdf_mem` this project already calls, then collapses the result to a `String`: `pages_needing_ocr`, `has_encoding_issues`, `page_count` and `title` all go to `log::warn!` and are discarded, and a PDF with no text layer returns `Err(Unsupported)` rather than a per-page routing decision.

That is precisely the structure §4 argued for and the correction above credited `pdf-inspector` with providing. **`extract_pdf` therefore stays on `pdf-inspector` directly**, and the dispatcher never routes a PDF to `anydoc`.

Spreadsheets are the same story one level down: `anydoc` reads them through `calamine` — the crate this project already uses — and renders markdown tables, which `extract_spreadsheet` rejected on purpose for 40-column `.gov` budget sheets. Workbooks are routed back to the existing path.

So the gain is **the formats this pipeline could not read at all**: Word, PowerPoint, OpenDocument, RTF and EPUB.

### Caveats

- **Younger than `pdf-inspector` was at its correction.** `0.1.3`, published 2026-08-04, ~500 downloads at time of review.
- **Same vendor as `pdf-inspector`**, which is already load-bearing here. This adds surface, not a new single-vendor bet.
- **`ConvertError` is `#[non_exhaustive]`** — matches on it need a wildcard arm and will keep needing one.
- **`Format::from_bytes` needs the whole file.** ZIP package identity lives in the central directory at the end of the file, so it cannot run against the 4 KB head `content_kind` classifies from. The format is therefore decided at extraction, not at classification.
