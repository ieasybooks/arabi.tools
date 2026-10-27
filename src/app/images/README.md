# Images

`logo.svg` is the site's logo: the letter ع from Thmanyah Serif Display, the font of the Arabic headings, in paper color on a green tile, with a gold diamond like the ornament under page titles.

The other files are rendered from it and from the bundled fonts by `scripts/render-images.mjs`: `favicon.ico` (16, 32 and 48 pixels), `icon-192.png` and `icon-512.png` for the web manifest, `apple-touch-icon.png`, and the 1200 by 630 link-preview cards `og-ar.png` and `og-en.png`. After changing the logo or the cards' text, run `npm ci --prefix tests/browser`, then `node scripts/render-images.mjs` from the repository root.

Both cards describe the collection: audio transcription, PDF/image text recognition and automatic diacritization. Their collection summaries come from `preview_tools` in `src/i18n.rs`.
