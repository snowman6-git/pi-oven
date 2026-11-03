# Interface languages

Each language is one JSON file in this folder, named by its language code: `ko.json`, `en.json`, `ja.json`, `pt-br.json`…
The app finds every file here when it is built. You do not need to change any code to add a language.

- `ko.json` (Korean) is the source. It has every key.
- A key that is missing from a language uses English instead, then Korean.
- The app starts in the language you chose last time. Otherwise it uses your browser's language if a file exists for it, and English if not.
- To change the language, use **Settings → Display → Language** or the globe button at the bottom of the session list.

## Adding a language

1. Copy `en.json` to `<code>.json`. Use a lowercase code, for example `ja.json`, `de.json` or `zh-tw.json`.
2. Set `_meta`. `name` is what the language list shows, written in the language itself. `locale` is the BCP 47 tag used for dates and numbers.
   ```json
   "_meta": { "name": "日本語", "locale": "ja-JP" }
   ```
3. Translate the values. Keep the keys exactly as they are.
4. Run the check, then build:
   ```sh
   cd web
   npm run i18n          # missing keys, broken {placeholders}, keys the code uses that ko.json lacks
   npm run build
   ```
   You can also run `npm run dev` and pick the language in Settings to see it live.

A partial translation works too: untranslated keys show in English. `npm run i18n` lists what is still missing.

## Format

- **Placeholders**: `{name}` is filled in by the app. Keep every placeholder that the Korean text has. You can move them anywhere in the sentence.
  ```json
  "newIn": "New session in {folder}"
  ```
- **Counts**: a value that depends on a number is an object of plural forms, chosen with `Intl.PluralRules` for your language. The forms are `zero`, `one`, `two`, `few`, `many` and `other`. `other` is required. Include the forms your language uses, and `{n}` is the number.
  ```json
  "commitN": { "one": "Commit {n} file", "other": "Commit {n} files" }
  ```
- **Markup**: a few values hold small HTML such as `<code>…</code>` or `<b>…</b>`. Keep those tags as they are.
- Keys are grouped by the part of the screen they belong to (`sidebar`, `settings`, `git`…). `common` holds words used in many places.

## For developers

- In components, `import { t } from '../lib/i18n.svelte'` and write `t('git.commit')` or `t('git.commitN', { n })`. The text follows a language change without a reload.
- In plain modules (and tests), import `t` from `lib/i18n`. Tests run in Korean.
- `S.name` (`lib/strings.ts`) is short for `t('common.name')`.
- New text goes into `ko.json` and `en.json` together. `npm run i18n` fails if the code asks for a key that `ko.json` does not have. Keys are typed from `ko.json`, so `svelte-check` also catches typos in literal keys.
