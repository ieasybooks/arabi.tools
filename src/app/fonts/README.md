# Fonts

These WOFF2 files are bundled with the site.

- Gilroy Regular, Medium and Bold (400, 500, 700): Latin text and headings.
- Noto Naskh Arabic UI, variable (400 to 700): Arabic interface text.
- Thmanyah Serif Display Medium (500): Arabic headings.
- Kitab Regular (400): Arabic transcripts, recognized text and diacritization text.

`src/app.rs` declares them with their weights and Unicode ranges, so Latin characters come from Gilroy and Arabic characters from the Arabic fonts. Topcoat bundles these local files, so builds and integration tests do not download fonts.

Font license files are not included in this directory.
