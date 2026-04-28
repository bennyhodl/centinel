# Document Kinds — Per-Type Extraction Playbook

The Archivist's golden rule: **try Hermes `web_extract` first**, then fall back to terminal tools when web_extract returns empty, errors, or doesn't support the mime type. Every fallback below assumes the corresponding system package is installed; SKILL.md lists them.

Detect kind by magic bytes (`file --mime-type <path>`), NOT by URL extension or server `Content-Type`. Servers lie.

---

## PDF — `application/pdf`

### Primary: `web_extract`
Hermes `web_extract` will fetch a PDF URL and return markdown. For text-bearing PDFs (most council agendas, budget summaries, contracts) this is everything you need. Cache the markdown for the sidecar's "Extracted text" section.

### Fallback: `pdftotext`
```bash
pdftotext -layout -enc UTF-8 <vault_path> -
```
- `-layout` preserves columns (tables in budget PDFs render reasonably).
- If the result is mostly whitespace or fewer than `50 * page_count` characters, the PDF is image-only — escalate to OCR.

### OCR fallback: `ocrmypdf` + `tesseract`
```bash
ocrmypdf --skip-text --output-type pdf <vault_path> <_tmp>/ocr.pdf
pdftotext -layout <_tmp>/ocr.pdf -
```
Mark `ocr_engine: tesseract-<version>` in the manifest entry. Note this is slow (minutes for hundred-page books); for large docs, do it asynchronously and respond to the inbox message with an interim sidecar that has `extraction_status: ocr_pending`.

### Encrypted PDFs
```bash
pdftotext -upw '' <path> - 2>&1 || echo "ENCRYPTED"
```
Empty owner password works for ~80% of restricted PDFs. If still locked, vault the original anyway, write a sidecar with `extracted_text: "(encrypted — awaiting operator)"`, and drop an operator-queue note.

### Embedded attachments
`pdfdetach -list <path>` lists embedded files. For v0.1, log them in the sidecar (`embedded_attachments: [...]`) but do not recursively vault.

### Failure modes
- Garbage Unicode (mojibake) — usually a CID-encoded font; tesseract OCR is the cleaner path.
- Zero pages reported — file is corrupt; flag operator, do not vault corrupt files.
- Page count inflation — some PDFs have hidden form-only pages; report `pdfinfo`'s count.

---

## HTML — `text/html`

### Primary: `web_extract`
Returns rendered markdown for static pages. Good for `.gov` agenda pages, contract listings.

### Fallback (static): `curl` + `html2text` or `trafilatura`
```bash
curl -L --fail -A 'tampa-doge-archivist/0.1 (+contact@example)' <url> -o <_tmp>/page.html
trafilatura -i <_tmp>/page.html -o -        # preferred — strips chrome
# or
html2text <_tmp>/page.html
```

### JS-rendered (SPA): `browser_navigate`
Trigger: `web_extract` returned <200 chars but the page is >50KB, OR the URL host is known SPA (Granicus, OpenGov, Tyler portals).
- `browser_navigate <url>` → save rendered DOM as `<sha8>-<slug>.html`.
- Take a PNG screenshot at the same path (`<sha8>-<slug>.png`) — the screenshot has its own sha256 and gets its own manifest line under `images/`. Cross-link via `paired_with` field.

### Failure modes
- Geo-blocked / Cloudflare challenge — log and flag operator; do not retry-spam.
- Login-walled — skip; we do not vault behind logins in v0.1.
- Robots.txt disallow — respect it; flag operator if the URL was explicitly requested.

---

## Excel / `.xlsx` — `application/vnd.openxmlformats-officedocument.spreadsheetml.sheet`

### Primary: `web_extract`
Often returns markdown tables for simple sheets.

### Fallback: `ssconvert` (gnumeric)
```bash
ssconvert <vault_path> <_tmp>/out.csv         # first sheet only
ssconvert -S <vault_path> <_tmp>/sheet.csv    # one CSV per sheet
```

### Fallback to fallback: `soffice --headless`
```bash
soffice --headless --convert-to csv --outdir <_tmp> <vault_path>
```

Sidecar's "Extracted text" should include sheet names + first 50 rows of each sheet. Full data lives in the original `.xlsx` (vault file).

### Failure modes
- Macro-enabled (`.xlsm`) — convert as `.xlsx`, do not execute macros.
- Pivot tables / charts — won't render; log in sidecar.

---

## CSV — `text/csv`, `application/csv`

### Primary: `web_extract`
For URLs. Keeps it simple.

### Fallback: stdlib
```python
python3 -c "import csv,sys; [print(','.join(r)) for r in csv.reader(open(sys.argv[1]))]" <path>
```
Sidecar should list columns + first 20 rows + row count. Keep raw CSV in vault unmodified.

### Failure modes
- Encoding (latin-1 vs utf-8) — sniff with `file -i`; convert with `iconv -f <src> -t utf-8` if needed but **vault the original bytes**, not the converted version.

---

## DOC / DOCX — Word

### Primary: `web_extract`
### Fallback: `soffice --headless --convert-to txt`
**Never** run pdftotext on `.docx` (silent gibberish).

---

## Images — `image/png|jpeg|webp|tiff`

### Primary: `web_extract`
Often skips.

### Fallback: `tesseract`
```bash
tesseract <vault_path> - -l eng
```
Sidecar's "Extracted text" is OCR output. Summary section gets a brief LLM caption ("scanned receipt for $X dated Y").

### Failure modes
- Low-DPI screenshots — OCR confidence will be poor; keep the OCR text but flag `ocr_quality: low`.
- Photos of documents (vs scans) — perspective skew; tesseract still tries; flag and rely on summary.

---

## Audio / Video — `audio/*`, `video/*`

### Primary: none — `web_extract` doesn't transcribe.

### Fallback: `whisper`
```bash
ffmpeg -i <vault_path> -ar 16000 -ac 1 <_tmp>/audio.wav
whisper <_tmp>/audio.wav --model base --output_format txt --output_dir <_tmp>
```
Or `whisper.cpp` for local. **Before transcribing anything >30 min**, drop an operator-queue note for cost confirmation — Whisper API and even local CPU transcription is real money/time.

Sidecar holds full transcript + `[HH:MM:SS]` timestamps where the model provides them. Original audio/video file stays in `transcripts/` (yes, the original — `transcripts/` holds both media and text in v0.1).

### Failure modes
- Multiple speakers without diarization — transcript reads as one voice; flag and consider `pyannote` follow-up.
- Background noise — large model handles better than base; bump model size if quality is poor.

---

## Unsupported / unknown mime

If `file --mime-type` returns something none of the above handle:
1. Vault the raw bytes anyway (the file IS the evidence).
2. Sidecar with `extraction_status: unsupported_mime` and the actual mime type.
3. Drop an operator-queue note describing the doc and its likely format.

The vault never refuses a file. It may refuse to *parse* a file, but the bytes always land.
