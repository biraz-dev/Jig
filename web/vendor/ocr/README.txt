# Optional local OCR assets

These files are loaded only when an image or scanned PDF needs OCR. They are
served from Jig's own origin. Document bytes and recognized text are never sent
to an OCR service; they stay in browser memory. Tesseract language caching is
disabled so financial documents do not create IndexedDB records.

Run `python3 scripts/fetch_ocr_assets.py` before building Jig's browser version.
The script downloads SHA-256-pinned official packages/model data and validates
every output against manifest.json. Engines, character maps and fonts are
generated build assets and ignored by Git; only this manifest and the license
notices are versioned. The same build step runs before Trunk in GitHub Actions.
It needs Python's standard library, not npm or a runtime CDN connection.

Vendored versions and official sources:

- Tesseract.js **6.0.1**, from the official npm `tesseract.js` package. Apache-2.0;
  see `LICENSE-tesseract.js.txt` and the bundled `.LICENSE.txt` notices.
- Tesseract.js-core **6.0.0**, from the official npm `tesseract.js-core` package.
  Apache-2.0; see `LICENSE-tesseract-core.txt`. The two LSTM-only `.wasm.js` files
  contain embedded WebAssembly; only the SIMD-compatible variant selected by
  the browser is loaded. The legacy OCR engine is omitted.
- English fast LSTM data from `tesseract-ocr/tessdata_fast`, pinned to commit
  `923915d4ced2a7235221788285785a29c4a42d4a`. Apache-2.0; the repository's
  current license text is retained in `LICENSE-tessdata-fast.txt` (license blob
  `d645695673349e3947e8e5ae42332d0ac3164cd7`). The model is reproducibly gzip
  compressed with timestamp zero.
- PDF.js **4.10.38**, from the official npm `pdfjs-dist` package. Apache-2.0;
  see `LICENSE-pdfjs.txt`. Only the minified display/worker modules, character
  maps, and standard fonts are included. The maps and fonts retain their
  separate Adobe, Foxit, and Liberation notices in their directories.

`manifest.json` records every engine asset's size, source version and SHA-256.
The complete optional bundle is approximately 13.2 MiB. A screenshot loads one
OCR core variant and the English model (approximately 6 MiB before HTTP
compression); PDFs additionally load their local renderer. No engine assets
are loaded at application startup. No CDN fallback is configured.

English OCR is an estimate. Users must review recognized dates, signs, decimal
separators, descriptions and amounts before importing rows into their workbook.
The bridge limits individual files to 10 MiB, PDFs to 100 pages, rendered OCR
images to 6 megapixels and recognized text to 1 MiB. Image screenshots larger
than 6 megapixels must be cropped or resized before import.
