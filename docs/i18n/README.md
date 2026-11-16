# Documentation localization

The site and operator guide support English, Simplified Chinese, Hindi,
Spanish, Modern Standard Arabic, French, Bengali, Portuguese, Indonesian,
Russian, and German. English has unprefixed site URLs. Other editions use
the language tags 'zh-Hans', 'hi', 'es', 'ar', 'fr', 'bn', 'pt', 'id',
'ru', and 'de'. The site sets the document language and writing direction.
Arabic commands and code samples use left-to-right layout. Every edition
has a language switcher linking to the equivalent page.

## Coverage

- The landing, download, mail, Winlink Express, operator, and documentation
  index pages have localized copy. The operator guide is also available in
  Markdown, with identical CLI commands and file examples across editions.
- The reference catalog indexes Markdown, roff, LaTeX, PDF, and HTML documentation
  from the repository, including installation, PKI, packaging, test plans,
  architecture, whitepapers, and the lab notes. Each document has an entry
  in every locale. Entries with only an English reference label that source.
- Complete editions in all eleven languages cover **linkwise-station(5)**,
  **linkwise-modemd(8)**, and the Winlink Express, macOS, Ubuntu, and Fedora
  installation guides. The website publishes these alongside their sources.
- The CLI manual **linkwise(1)** has a localized overview followed by
  the complete English text. The operating system and terminal determine
  shaping, font fallback, and bidirectional display for UTF-8 man pages.

## Build

Use Node.js 22.19 or later. From the repository root, run:

~~~sh
node docs/i18n/manuals.mjs
MANPATH="$PWD/docs/build/i18n/man/fr" man 1 linkwise
cd website
npm ci
npm run build
npm run check
~~~

The man generator accepts an optional output directory and writes a
'man1', 'man5', and 'man8' directory per locale. The website publishes
downloadable roff sources under '/linkwise/manuals/LANGUAGE/' and
Markdown operator guides under '/linkwise/guides/LANGUAGE.md'.

## Maintain editions

Update the English source alongside code changes. Message bundle fields
are defined by 'locales.mjs'. When adding a field, update every bundle.
The command samples in 'operator.mjs' are shared with the website,
Markdown guides, and bilingual CLI manual.

For a complete document translation, mirror the path below
'docs/i18n/translations/LANGUAGE/'. For example, a translated German guide
for 'rust/docs/pki.md' belongs at
'docs/i18n/translations/de/rust/docs/pki.md'. Keep source-relative
cross-references valid in the overlay. For a complete man translation,
include the section and retain the literal command names, arguments, and
English roff macros. The manual generator uses a complete overlay in
preference to a bilingual edition. The reference entry always retains a
link to the authoritative English source.
