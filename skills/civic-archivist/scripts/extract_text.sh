#!/usr/bin/env bash
# extract_text.sh — Type-dispatch text extractor for the Archivist.
#
# This is the *terminal fallback path*. The Archivist tries the role's web_fetch tool (TODO: stub)
# FIRST for any URL fetch; this script only runs when web_extract failed,
# returned empty, or the input is a local file (operator inbox drop).
#
# Usage:
#   extract_text.sh <vault_path>         # detect by magic + extension, dispatch
#   extract_text.sh --kind=pdf <path>    # force a kind
#
# Output: plaintext to stdout. Status messages to stderr. Exit 0 on success,
# non-zero on extractor failure (caller should mark extraction_status).
#
# Required system packages (do NOT auto-install — document only):
#   - poppler-utils         (pdftotext, pdfdetach, pdfinfo)
#   - ocrmypdf              (PDF OCR pipeline)
#   - tesseract-ocr         (image OCR; ocrmypdf depends on this)
#   - gnumeric              (ssconvert, for .xlsx)
#   - libreoffice           (soffice, fallback for .xlsx and .docx)
#   - html2text             (HTML to plaintext)
#   - python3-trafilatura   (preferred HTML extractor; pip install trafilatura)
#   - ffmpeg                (audio/video preprocessing)
#   - whisper               (pip install openai-whisper, OR whisper.cpp)
#   - file                  (magic-byte mime detection; usually pre-installed)
#   - python3               (csv stdlib fallback)
#
# Exit codes:
#   0  ok
#   1  bad usage
#   2  file not found / unreadable
#   3  extractor not installed
#   4  extractor ran but produced empty/garbage output
#   5  encrypted / locked
#   6  unsupported mime

set -euo pipefail

KIND=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --kind=*) KIND="${1#--kind=}"; shift;;
    -h|--help) sed -n '2,30p' "$0"; exit 0;;
    *) FILE="$1"; shift;;
  esac
done

if [[ -z "${FILE:-}" ]]; then
  echo "extract_text.sh: no path argument" >&2
  exit 1
fi
if [[ ! -r "$FILE" ]]; then
  echo "extract_text.sh: not readable: $FILE" >&2
  exit 2
fi

have() { command -v "$1" >/dev/null 2>&1; }

if [[ -z "$KIND" ]]; then
  MIME="$(file --mime-type -b "$FILE")"
  case "$MIME" in
    application/pdf)                                      KIND=pdf;;
    text/html|application/xhtml+xml)                      KIND=html;;
    text/csv|application/csv)                             KIND=csv;;
    application/vnd.openxmlformats-officedocument.spreadsheetml.sheet|application/vnd.ms-excel) KIND=xlsx;;
    application/vnd.openxmlformats-officedocument.wordprocessingml.document|application/msword) KIND=docx;;
    image/*)                                              KIND=image;;
    audio/*|video/*)                                      KIND=av;;
    text/plain)                                           KIND=text;;
    *) KIND=unknown;;
  esac
fi

case "$KIND" in
  pdf)
    have pdftotext || { echo "pdftotext not installed (poppler-utils)" >&2; exit 3; }
    OUT="$(pdftotext -layout -enc UTF-8 "$FILE" - 2>/dev/null || true)"
    PAGES="$(pdfinfo "$FILE" 2>/dev/null | awk '/^Pages:/{print $2}')"
    PAGES="${PAGES:-1}"
    LEN="${#OUT}"
    # Heuristic: < 50 chars per page => image-only; OCR.
    if (( LEN < 50 * PAGES )); then
      echo "extract_text.sh: PDF appears image-only ($LEN chars / $PAGES pages); OCRing" >&2
      have ocrmypdf || { echo "ocrmypdf not installed" >&2; exit 3; }
      TMP="$(mktemp --suffix=.pdf)"
      trap 'rm -f "$TMP"' EXIT
      if ! ocrmypdf --skip-text --quiet "$FILE" "$TMP" 2>/dev/null; then
        # Try forcing OCR; --skip-text bails on already-text pages.
        if ! ocrmypdf --force-ocr --quiet "$FILE" "$TMP" 2>/dev/null; then
          # Possibly encrypted.
          if pdftotext -upw '' "$FILE" - >/dev/null 2>&1; then
            pdftotext -layout -upw '' "$FILE" -
            exit 0
          fi
          echo "extract_text.sh: OCR failed; possibly encrypted/corrupt" >&2
          exit 5
        fi
      fi
      pdftotext -layout "$TMP" -
    else
      printf '%s' "$OUT"
    fi
    ;;
  html)
    if have trafilatura; then
      trafilatura -i "$FILE" || { echo "trafilatura failed" >&2; exit 4; }
    elif have html2text; then
      html2text "$FILE"
    else
      echo "no HTML extractor installed (trafilatura or html2text)" >&2
      exit 3
    fi
    ;;
  csv)
    # Just emit the file; Archivist sidecar trims to first 20 rows.
    cat "$FILE"
    ;;
  xlsx)
    if have ssconvert; then
      TMP="$(mktemp -d)"
      trap 'rm -rf "$TMP"' EXIT
      ssconvert -S "$FILE" "$TMP/sheet.csv" 2>/dev/null || true
      for f in "$TMP"/sheet.csv*; do
        [[ -f "$f" ]] || continue
        echo "## $(basename "$f")"
        cat "$f"
        echo
      done
    elif have soffice; then
      TMP="$(mktemp -d)"
      trap 'rm -rf "$TMP"' EXIT
      soffice --headless --convert-to csv --outdir "$TMP" "$FILE" >/dev/null 2>&1
      cat "$TMP"/*.csv
    else
      echo "no spreadsheet converter installed (ssconvert or soffice)" >&2
      exit 3
    fi
    ;;
  docx)
    have soffice || { echo "soffice (libreoffice) not installed" >&2; exit 3; }
    TMP="$(mktemp -d)"
    trap 'rm -rf "$TMP"' EXIT
    soffice --headless --convert-to txt --outdir "$TMP" "$FILE" >/dev/null 2>&1
    cat "$TMP"/*.txt
    ;;
  image)
    have tesseract || { echo "tesseract not installed" >&2; exit 3; }
    tesseract "$FILE" - -l eng 2>/dev/null
    ;;
  av)
    have whisper || { echo "whisper not installed (pip install openai-whisper)" >&2; exit 3; }
    have ffmpeg || { echo "ffmpeg not installed" >&2; exit 3; }
    TMP="$(mktemp -d)"
    trap 'rm -rf "$TMP"' EXIT
    ffmpeg -loglevel error -i "$FILE" -ar 16000 -ac 1 "$TMP/a.wav"
    whisper "$TMP/a.wav" --model base --output_format txt --output_dir "$TMP" >/dev/null 2>&1
    cat "$TMP"/a.txt
    ;;
  text)
    cat "$FILE"
    ;;
  unknown|*)
    echo "extract_text.sh: unsupported mime/kind: $KIND" >&2
    exit 6
    ;;
esac
