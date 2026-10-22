# Fonts

These unmodified WOFF2 files come from the official [Fontsource font packages](https://github.com/fontsource/font-files). Both packages are version 5.3.0 and licensed under SIL Open Font License 1.1; each directory includes the package's original `LICENSE` as `OFL.txt`.

- [Inter 5.3.0](https://registry.npmjs.org/@fontsource/inter/-/inter-5.3.0.tgz): Latin subset, normal style, weights 400, 500, and 600. See [OFL.txt](inter/OFL.txt).
- [IBM Plex Sans Arabic 5.3.0](https://registry.npmjs.org/@fontsource/ibm-plex-sans-arabic/-/ibm-plex-sans-arabic-5.3.0.tgz): Arabic subset, normal style, weights 400, 500, and 600. See [OFL.txt](ibm-plex-sans-arabic/OFL.txt).

The files match the jsDelivr Fontsource URLs previously used by Topcoat 0.10.0. `src/app.rs` declares them with the same family names, weights and swap display. Each file holds only its own script, so no Unicode ranges are needed: the stylesheet lists Inter first and the browser takes each character from the font that has it. Topcoat bundles these local files, so builds and integration tests do not download fonts.

When updating, copy the same six files from the packages' `files/` directories, keep each package's license, update the versions and source links here, and check both Arabic and English pages.
