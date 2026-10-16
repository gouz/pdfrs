# pdfrs

Generates PDFs from HTML/CSS, **without a browser**: a Rust port of [`@gouz/pdfjs`](https://github.com/gouz/pdfjs), shipped as a small HTTP API in a ~80 MB distroless Docker image, **~4.7× faster per core and ~7× leaner in memory** (see [Performance](#performance)).

- HTML parsing by [html5ever](https://github.com/servo/html5ever) (Servo's parser), CSS selectors by [`selectors`](https://crates.io/crates/selectors)
- Layout by [Taffy](https://github.com/DioxusLabs/taffy): block (with margin collapsing), flexbox, **grid**, float — the counterpart of Yoga in react-pdf
- Text by [Parley](https://github.com/linebender/parley): shaping, Unicode line breaking, justification, per-character font fallback
- PDF by [krilla](https://github.com/LaurenzV/krilla) (Typst's PDF engine): subset embedded fonts, selectable text, clickable links

## Getting started

```bash
docker run -p 3000:3000 ghcr.io/gouz/pdfrs:main
# or from source
docker build -t pdfrs . && docker run -p 3000:3000 pdfrs
```

The image (amd64 and arm64) is published by CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)): `:main` and `:sha-xxxxxxx` on every push to `main`, `:1.2.3`, `:1.2` and `:latest` for a `v1.2.3` tag. Pull requests run the tests and build the image without publishing it.

```bash
# raw HTML in the body, options in the query string
curl --data-binary @invoice.html 'http://localhost:3000/?size=A4&margin=40' -o invoice.pdf

# or as JSON
curl -H 'content-type: application/json' \
  -d '{"html": "<h1>Hello</h1>", "footer": "<p style=\"text-align:right\">{page} / {pages}</p>"}' \
  http://localhost:3000/ -o hello.pdf
```

| Route | |
|---|---|
| `POST /` or `POST /pdf` | body: the HTML (`text/html`, or anything), or `{"html": …, …options}` / `{"template": …, "data": {…}}` / `{"template_name": …, "data": {…}}` (`application/json`, see [Templates](#templates)). Response: `application/pdf` |
| `GET /health` | `ok` |
| `GET /` | usage |

Errors: `400` for an invalid option or a template / data error (the message says which one, and where), `500` if rendering fails.

### Options

As query parameters or JSON fields:

| Option | Example | |
|---|---|---|
| `size` | `A5`, `LETTER`, `400x600` | page size (`A4` by default, `A3`, `A6`, `B5`, `LEGAL`, `TABLOID`, or `WIDTHxHEIGHT` in pt) |
| `landscape` | `true` | landscape orientation |
| `margin` | `40`, `60 40 50 40` | margins in pt (40 by default), CSS order top right bottom left, commas accepted |

`size`, `landscape` and `margin` can also come from the document: `@page { size: A5 landscape; margin: 10mm }`. An option passed with the request wins over `@page`.
| `font` | `Times-Roman` | default font (`Helvetica` by default) |
| `fontSize` | `11` | default size (12) |
| `header` | `<p>Report</p>` | header repeated on every page, vertically centered in the top margin |
| `footer` | `<p>{page} / {pages}</p>` | footer repeated on every page |
| `title`, `author`, `subject`, `keywords` | | PDF metadata (`title` falls back to `<title>`) |

### Environment variables

| Variable | Default | |
|---|---|---|
| `PORT` | `3000` | |
| `WORKERS` | number of cores | render threads; extra requests wait their turn |
| `FONTS_DIR` | `/fonts` | font directories (`.ttf`, `.otf`, `.ttc`, `.woff`, `.woff2`, recursive), separated by `:` |
| `IMAGES_DIR` | — | directory for local images, stylesheets and fonts, see [Images](#images) |
| `TEMPLATES_DIR` | — | directory of the templates (`template_name`), see [Templates](#templates); also the image directory when `IMAGES_DIR` is not set |
| `LOCALE` | `fr` | default locale of the `money` / `number` / `date` template filters |
| `REMOTE_RESOURCES` | off | `1`: also load http(s) images, stylesheets and fonts, see [Remote resources](#remote-resources) |
| `REMOTE_HOSTS` | any public host | comma separated allow-list: `cdn.example.com,*.example.org` |
| `REMOTE_TIMEOUT_MS` / `REMOTE_MAX_MB` | `5000` / `10` | per download |
| `REMOTE_ALLOW_PRIVATE` | off | `1`: allow loopback and private addresses (a trusted internal network only) |
| `MAX_BODY_MB` | `20` | maximum request size |

## Templates

Instead of HTML, send a [MiniJinja](https://github.com/mitsuhiko/minijinja) template (Jinja / Twig syntax) and its data: pdfrs fills the template, then renders it. Callers send data, the layout lives in the templates.

```bash
# a template of TEMPLATES_DIR
curl -H 'content-type: application/json' -d '{
  "template_name": "facture.html",
  "data": { "numero": "2026-0142", "client": { "nom": "ACME" }, "lignes": [{ "libelle": "Café", "quantite": 2, "prix": 3 }] },
  "size": "A4"
}' http://localhost:3000/ -o facture.pdf

# or the template itself, inline
curl -H 'content-type: application/json' -d '{
  "template": "<h1>Facture {{ numero }}</h1>{% for l in lignes %}<p>{{ l.libelle }} : {{ l.prix | money }}</p>{% endfor %}",
  "data": { "numero": 42, "lignes": [{ "libelle": "Café", "prix": 3 }] },
  "locale": "en"
}' http://localhost:3000/ -o facture.pdf

docker run -p 3000:3000 -v ./templates:/templates:ro -e TEMPLATES_DIR=/templates ghcr.io/gouz/pdfrs:main
```

| Field | |
|---|---|
| `template` / `template_name` | the template's text, or the name of a file of `TEMPLATES_DIR` (`facture.html`, `clients/acme.html`); one of them, or `html` |
| `data` | a JSON object: the template's variables |
| `locale` | `fr` (default, or `LOCALE`), `en`, `de`, `es`, `it`, `fr-CH`…: digit grouping, decimal mark, currency position, month names |
| `header`, `footer` | templates too (same data), with `{page}` / `{pages}` |
| every other option | `size`, `margin`, `title`… as for HTML |

- **Syntax**: `{{ client.nom }}`, `{% for l in lignes %}…{% endfor %}`, `{% if remise is defined %}…{% endif %}`, `{% set ns = namespace(total=0) %}` for sums in a loop, `loop.index`, `{# comments #}`, macros, and every [built-in filter](https://docs.rs/minijinja/latest/minijinja/filters/) (`default`, `length`, `round`, `sum`, `upper`, `join`, `sort`, `groupby`, `tojson`…).
- **Layouts**: `{% extends "base.html" %}` and `{% include "parts/address.html" %}` read other templates of `TEMPLATES_DIR` (an inline template can extend them too). A template edited on disk is picked up by the next request, no restart.
- **Strict**: a variable missing from the data is an error (`400`, with the template and the line), so a typo in the data does not print an empty invoice. Optional fields: `{{ client.tva | default("") }}`, `{% if client.tva is defined %}`.
- **Escaped**: every `{{ … }}` is HTML-escaped, a `<` or `</td>` in the data cannot break the document or inject markup. `| safe` opts out, for trusted HTML only.
- **Bounded**: a template only reads files of its directory (no `../`, absolute path or symlink out of it), a recursion limit, and a budget of instructions (an endless loop is a `400`, not a stuck render thread).
- Images, stylesheets and fonts of the templates (`<img src="logo.png">`, `<link href="style.css">`) come from `TEMPLATES_DIR` unless `IMAGES_DIR` is set.

Filters for business documents:

| Filter | `fr` | `en` |
|---|---|---|
| `{{ 1234.5 \| money }}` | `1 234,50 €` | `€1,234.50` |
| `{{ 99 \| money("USD") }}` | `99,00 $` | `$99.00` |
| `{{ total \| money(decimals=0) }}` | `1 235 €` | `€1,235` |
| `{{ 1234.5 \| number }}` / `number(2)` | `1 235` / `1 234,50` | `1,235` / `1,234.50` |
| `{{ "2026-10-15" \| date }}` | `15/10/2026` | `10/15/2026` |
| `{{ "2026-10-15" \| date("%A %e %B %Y") }}` | `jeudi 15 octobre 2026` | `Thursday 15 October 2026` |
| `{{ adresse \| nl2br }}` | line breaks as `<br>`, escaped | |

Amounts are rounded half away from zero (`2.675` → `2,68`), dates are `YYYY-MM-DD` with an optional time (`2026-10-15T08:30:00Z`, `%H:%M`), any filter takes `locale="…"` to override the request's.

From the command line, `--data` makes the input a template of its folder:

```bash
pdfrs demo/templates/facture.html facture.pdf --data demo/templates/facture.json --locale fr
```

[`demo/templates/`](demo/templates) holds a complete invoice: `base.html` (styles, `@page`, numbered footer), `facture.html` (`extends` it; totals computed in the template), `facture.json`, rendered as [`demo/facture.pdf`](demo/facture.pdf) (2 pages, the table header repeated).

## Deploy on Google Cloud Run

A stateless HTTP container that starts in milliseconds: a good fit for [Cloud Run](https://cloud.google.com/run). The server listens on `$PORT`, which Cloud Run sets.

**1. Put the image in Artifact Registry.** Cloud Run pulls from Artifact Registry or Docker Hub, not from ghcr.io. Either copy the published image:

```bash
PROJECT=my-project REGION=europe-west1
gcloud artifacts repositories create pdfrs --repository-format=docker --location=$REGION
gcloud auth configure-docker $REGION-docker.pkg.dev
docker pull --platform linux/amd64 ghcr.io/gouz/pdfrs:0.3.0
docker tag ghcr.io/gouz/pdfrs:0.3.0 $REGION-docker.pkg.dev/$PROJECT/pdfrs/pdfrs:0.3.0
docker push $REGION-docker.pkg.dev/$PROJECT/pdfrs/pdfrs:0.3.0
```

or build it from the sources with Cloud Build (it uses the `Dockerfile`):

```bash
gcloud builds submit --tag $REGION-docker.pkg.dev/$PROJECT/pdfrs/pdfrs:0.3.0
```

**2. Deploy.**

```bash
gcloud run deploy pdfrs \
  --image=$REGION-docker.pkg.dev/$PROJECT/pdfrs/pdfrs:0.3.0 \
  --region=$REGION \
  --cpu=2 --memory=512Mi \
  --set-env-vars=WORKERS=2 \
  --concurrency=40 \
  --min-instances=0 --max-instances=10 \
  --no-allow-unauthenticated
```

- **CPU**: rendering is CPU-bound, one render thread per vCPU (`WORKERS` = `--cpu`); each vCPU renders ~1,000 of the benchmark documents per second. Cloud Run scales out by instances, so 1 or 2 vCPUs per instance is enough.
- **Memory**: a render thread needs ~15 MB plus the documents in flight; 512 MiB leaves room for large documents and images.
- **Concurrency**: requests beyond `WORKERS` wait in the instance's queue. A lower `--concurrency` makes Cloud Run add instances sooner (lower latency), a higher one packs more requests per instance (lower cost). ~20 per vCPU is a good start.
- **Request size**: Cloud Run accepts up to 32 MB per request; `MAX_BODY_MB` (20 by default) is checked first.
- **Cold start**: the server answers `/health` ~50 ms after its container starts (fonts loaded), measured locally; Cloud Run adds the instance start-up on top. `--min-instances=1` keeps one instance warm.

**3. Call it.** Unauthenticated calls are refused (`--no-allow-unauthenticated`): callers need the `roles/run.invoker` role and an identity token.

```bash
URL=$(gcloud run services describe pdfrs --region=$REGION --format='value(status.url)')
curl -H "Authorization: Bearer $(gcloud auth print-identity-token)" \
  --data-binary @invoice.html "$URL/?size=A4" -o invoice.pdf
```

From another Cloud Run service or a Cloud Function, fetch the identity token for the audience `$URL` from the metadata server (the Google client libraries do it: `google.oauth2.id_token.fetch_id_token`, `GoogleAuth.getIdTokenClient`…). Use `--allow-unauthenticated` only for a public demo: anyone could then make the service render (and pay for the CPU).

**Images, stylesheets and fonts from Cloud Storage.** Mount a bucket and point `IMAGES_DIR` (and `FONTS_DIR`) at it:

```bash
gcloud run services update pdfrs --region=$REGION \
  --add-volume=name=assets,type=cloud-storage,bucket=my-pdf-assets,readonly=true \
  --add-volume-mount=volume=assets,mount-path=/assets \
  --update-env-vars=IMAGES_DIR=/assets,FONTS_DIR=/fonts:/assets/fonts
```

Templates the same way: `--update-env-vars=TEMPLATES_DIR=/assets/templates` serves the templates of `gs://my-pdf-assets/templates/` (edited templates are picked up without redeploying). `<img src="logo.png">` then reads `gs://my-pdf-assets/logo.png`, and the fonts of `gs://my-pdf-assets/fonts/` are loaded at start-up (redeploy to pick up new fonts). The service account of the service needs `roles/storage.objectViewer` on the bucket.

**Remote resources on GCP.** If you enable them (`REMOTE_RESOURCES=1`), keep the guards on: the metadata server (`169.254.169.254`, `metadata.google.internal`) hands out access tokens of the service account, and is precisely what an SSRF goes after. pdfrs refuses link-local and private addresses, so it is not reachable from a document; never set `REMOTE_ALLOW_PRIVATE=1` on Cloud Run. Prefer `REMOTE_HOSTS` with the hosts your templates actually use. Through a VPC connector with all traffic routed to the VPC, remote resources need Cloud NAT to reach the internet.

## Command line

The same binary converts a file:

```bash
cargo run --release -- invoice.html invoice.pdf --size A5 --footer '<p>{page} / {pages}</p>'
# or inside the image
docker run --rm -v "$PWD":/w -w /w --entrypoint /pdfrs pdfrs invoice.html invoice.pdf
```

Options: `--size`, `--landscape`, `--margin`, `--font`, `--font-size`, `--header`, `--footer`, `--title`, `--remote`, `--data data.json` (the input is a [template](#templates)), `--locale`. Local images, stylesheets and fonts are allowed from the HTML file's folder.

## Fonts

The image ships **Liberation Sans / Serif / Mono**, metric-compatible with Helvetica / Times / Courier (a document takes the same space as with pdfjs), and **DejaVu Sans** as a fallback for missing characters (arrows, symbols, Greek, Cyrillic…).

- `Helvetica`, `Arial`, `sans-serif`, `system-ui` → Liberation Sans; `Times-Roman`, `serif` → Liberation Serif; `Courier`, `monospace` → Liberation Mono.
- As in a browser, a stack like `font-family: "Segoe UI", Roboto, sans-serif` uses the first available font. If none is available, the declaration is ignored and the parent's font applies.
- A document can bring its own fonts with `@font-face` (`font-family`, `src`, `font-weight`, `font-style`): `src: url(data:font/woff2;base64,…)`, or a file of `IMAGES_DIR` (`src: url(fonts/Inter.woff2)`). TrueType, OpenType, WOFF and WOFF2. They are visible to that document only (and its header / footer).
- To add fonts for every document, mount them into the container: `docker run -v ./fonts:/fonts/extra pdfrs` (all of `FONTS_DIR` is scanned). They are usable by family name, in CSS as well as in the `font` option. Bold and italic use the family's matching files when they exist.
- No CJK font in the image: add for example Noto Sans CJK to `FONTS_DIR` for Chinese, Japanese or Korean.

## Images

The HTML may come from an untrusted source, so **by default only `data:` images are loaded**.

| Source | Allowed if… |
|---|---|
| `data:image/...` | always |
| `logo.png`, `img/a.jpg` | `IMAGES_DIR` is set and the file is **inside** that directory (`../`, absolute paths and symlinks pointing outside are refused) |
| `https://...` | remote resources are enabled (`REMOTE_RESOURCES=1`, `--remote`), see below |
| `file://`, others | never |

```bash
docker run -p 3000:3000 -v ./templates/img:/images:ro -e IMAGES_DIR=/images pdfrs
```

A refused or missing image is not displayed and a warning is written to stderr.

Formats: PNG, JPEG, GIF, WebP and SVG (in `<img>` or inline `<svg>…</svg>` in the HTML). Images are inline, as in a browser: they sit in the line of text (`vertical-align` applies, `text-align: center` centers them); `display: block`, a float or being a flex / grid item takes them out of the line. An image without a size keeps its natural size (1 px = 1 pt), without exceeding the available width; `width` / `height` (attributes or CSS) are honored, keeping the aspect ratio when only one is given. `object-fit` and `border-radius` are supported.

### Remote resources

Off by default: the HTML may be untrusted, and fetching a URL it names is the server making a request on its author's behalf (SSRF: cloud metadata endpoints, internal services). With `REMOTE_RESOURCES=1` (or `--remote`), `http(s)` images, background images, `<link>` / `@import` stylesheets and `@font-face` fonts are downloaded, with these guards:

- `http` and `https` only;
- addresses resolving to loopback, private, link-local, unique-local, carrier-grade NAT or multicast networks are refused, checked on every connection: redirects (3 at most, each checked) and DNS rebinding cannot get around it;
- optional host allow-list (`REMOTE_HOSTS`), timeout and size limit per download;
- each URL is downloaded once per document, the `<img>` of a document 8 at a time.

A refused or failed download is reported on stderr and rendered as a missing image / stylesheet / font.

## Supported CSS

As in pdfjs, **1 px = 1 pt**.

- **Layout**: `display` `block` / `inline` / `inline-block` / `flex` / `inline-flex` / `grid` / `inline-grid` / `none` / `contents` / `list-item` / `table…`, margins that collapse like in a browser, `float` / `clear` with **text wrapping around floats** (images, boxes, drop caps: lines beside a float are shortened, then take the full width below it; a line with no room beside it goes below), `position: absolute` (relative to the parent).
- Full **flexbox**: `flex-direction`, `flex-wrap`, `flex` / `flex-grow` / `flex-shrink` / `flex-basis`, `justify-content`, `align-items`, `align-self`, `align-content`, `gap`, `order`.
- **Grid**: `grid-template-columns` / `-rows` (`fr`, `repeat()`, `minmax()`, `auto`…), `grid-column` / `grid-row`, `grid-auto-flow`, `gap`, `justify-items`, `place-items`…
- **Boxes**: `width` / `height` (+ `min-` / `max-`), `box-sizing`, `margin`, `padding`, `border` (width, `solid` / `dashed` / `dotted`, color per side), `border-radius`, `background-color`, `opacity`, `overflow: hidden`, `aspect-ratio`.
- **Backgrounds**: colors, gradients and `url()` images, **several layers** (first on top), with `background-size` (`cover`, `contain`, lengths, `%`), `background-position` (keywords, lengths, `%`, `right 10px bottom 5px`), `background-repeat`, `background-origin` and `background-clip` (`border-box`, `padding-box`, `content-box`, and `text`: gradient text, with `-webkit-text-fill-color`), `background-attachment: fixed` (against the page, on every page); all in the `background` shorthand too. The `html` / `body` background covers the whole page, margins included, like a browser's canvas.
- **Shadows**: `box-shadow` with offsets, blur, spread, color, `inset`, several shadows; a blurred shadow is drawn as a bitmap (~2 px per pt), an unblurred one as a crisp vector shape.
- **Transforms**: `transform` (`translate`, `scale`, `rotate`, `skew`, `matrix`, their `X` / `Y` forms, combined) and `transform-origin`. As in CSS, the layout is unchanged; links in a transformed box keep their untransformed area.
- **Generated content**: `::before` / `::after` (and `:before` / `:after`) with `content` strings, escapes (`"\201C"`), `attr()`, `open-quote` / `close-quote`, `counter()` / `counters()`, any style; `display: block` makes a box. `<q>` gets quotes. `::marker` styles list markers and can replace them (`content: "✓ "`); `::first-letter` styles the first letter of a block (floated: a drop cap the text wraps around); `::first-line` styles the first line as laid out (its text is shaped again until the styled line fits).
- **Counters**: `counter-reset`, `counter-increment`, `counter-set`, scoped as in CSS (a reset is visible to the following siblings), the implicit `list-item` counter of lists (`<ol start>`, `<li value>`), any list style (`counter(chapter, upper-roman)`); table rows and cells count too.
- **Values**: custom properties `--name` and `var(--name, fallback)` (inherited, nestable), `calc()` (`+ - * /`, parentheses, any mix of units, `calc(100% - 20px)` included), `min()` / `max()` / `clamp()` on absolute lengths.
- **At-rules**: `@media` (`print` / `all` match, `screen` does not; `min-` / `max-width` / `-height`, range syntax `(width >= 600px)`, `orientation`, `not`, lists; evaluated against the page area), `<style media="…">`, `<link rel="stylesheet">` and `@import` (files of `IMAGES_DIR`, or remote resources; `url()` relative to the stylesheet), `@supports` (assumed true), `@font-face`, and `@page`: size, margins, `:first` / `:left` / `:right` (mirrored margins move the column, its width stays), named pages (`@page wide { size: A4 landscape }` + `page: wide` on a top-level element: a section laid out at its own width, numbering and counters carry on), every margin box (`@top-left` / `-center` / `-right`, `@bottom-…`, corners `@top-left-corner`…, sides `@left-top` / `-middle` / `-bottom`, `@right-…`) with `content`, `counter(page)` / `counter(pages)` and any text style; the `header` / `footer` options win over the top / bottom boxes.
- **Right-to-left**: `dir="rtl"` / `direction: rtl` (bidi text, alignment, flex and grid order, list markers on the right). Hebrew and Arabic are shaped with DejaVu Sans.
- **Inline boxes**: `inline-block` (and `inline-flex` / `-grid` / `-table`) and images sit in the line of text like in a browser: padding, borders and backgrounds are kept, the box shrinks to fit the line when it is too wide, and `vertical-align` (`baseline`, `middle`, `top`, `bottom`, `text-top`, `text-bottom`) places it; `text-align` centers an image. Direct children of a flex or grid box are items instead, as in CSS.
- **Text**: `vertical-align` (`sub`, `super`, lengths, `%` of the line height) for text and inline boxes, `font` / `font-family` / `font-size` / `font-weight` / `font-style`, `color`, `text-align` (including `justify`), `line-height`, `text-decoration`, `text-transform`, `letter-spacing`, `word-spacing`, `white-space` (`pre`, `nowrap`…), background of inline elements (`<mark>`, `<span style="background">`).
- **Units**: `px`, `pt`, `em`, `rem`, `%`, `mm`, `cm`, `in`, `pc`. Colors: names, `#hex`, `rgb()`, `hsl()`…
- **Pagination**: automatic page breaks between lines of text (never through a line, an image or a table cell that fits on a page), `page-break-before` / `-after: always` (or `break-before: page`), `page-break-inside: avoid`. A table that continues on the next page repeats its `<thead>` at the top, and its `<tfoot>` at the bottom of each page it breaks on.
- **HTML**: tables (grid with `colspan` / `rowspan`, `border-collapse`, `border-spacing`, `vertical-align`), lists (`disc`, `circle`, `square`, `decimal`, `lower-alpha`, `upper-roman`…, `<ol start>`, `<li value>`), clickable links (`http(s):`, `mailto:`, `tel:`), `<br>`, `<pre>`, `<svg>`, browser default styles (`h1`…`h6`, `b`, `i`, `code`…), `align` / `bgcolor` attributes.
- `<style>` with all CSS selectors (`>`, `+`, `~`, `:first-child`, `:nth-child()`, `[attr]`, `:not()`…), specificity and `!important`.

**Not supported**: a first page wider or narrower than the others (the text would have to change width mid-paragraph; a named page covers a different cover page), floats that do not precede text in the same block (a float followed by blocks narrows them, the text does not flow back under it), `clear` on text after a float, `background-clip: text` with images (gradients only), `@page` named pages for nested elements (top-level ones only).

**Differences from pdfjs**:
- Vertical margins collapse like in a browser (react-pdf adds them up): paragraphs are slightly tighter.
- No infinite loop or crash on a `min-height` taller than the page, `gap: normal`, `border: medium` or an unknown font: nothing to work around.
- Much more CSS: grid, gradients, transforms, shadows, `::before` / `::after` / `::marker`, counters, `var()`, `calc()`, `@media`, `@page` with margin boxes, `@font-face` (WOFF2 too), `<link>` stylesheets, right-to-left text, repeated table headers and footers.
- Remote images, stylesheets and fonts only when enabled, with SSRF guards.

## Examples

| Source | Output | |
|---|---|---|
| [`demo/example.html`](demo/example.html) | [`demo/example.pdf`](demo/example.pdf) | simple invoice |
| [`demo/flex.html`](demo/flex.html) | [`demo/flex.pdf`](demo/flex.pdf) | flexbox |
| [`demo/images.html`](demo/images.html) | [`demo/images.pdf`](demo/images.pdf) | images and the image policy |
| [`demo/report.html`](demo/report.html) | [`demo/report.pdf`](demo/report.pdf) | 3 pages: grid, lists, SVG, `<pre>`, long table with a repeated header, forced page break (rendered with a header and a footer) |
| [`demo/inline.html`](demo/inline.html) | [`demo/inline.pdf`](demo/inline.pdf) | badges, buttons, cards and images in the line of text, shrink-to-fit, `vertical-align`, `<sub>` / `<sup>` |
| [`demo/backgrounds.html`](demo/backgrounds.html) | [`demo/backgrounds.pdf`](demo/backgrounds.pdf) | `box-shadow`, `background-image` (tiles, `cover`, `contain`, position), a 3-page table repeating its `<thead>` |
| [`demo/floats.html`](demo/floats.html) | [`demo/floats.pdf`](demo/floats.pdf) | text wrapping around a floated image and box, a floated drop cap, justified text |
| [`demo/print.html`](demo/print.html) + [`print.css`](demo/print.css) | [`demo/print.pdf`](demo/print.pdf) | `<link>` stylesheet, `@page` A5 with margin boxes (`Page 1 sur 4`) and a different first page, counters (`I.`, `1.1`, `counters()`), `::marker`, `::first-letter`, 3 background layers, `inset` shadow, repeated `<thead>` and `<tfoot>` |
| [`demo/features.html`](demo/features.html) | [`demo/features.pdf`](demo/features.pdf) | gradients, transforms, `calc()`, `var()`, `::before` / `::after`, `@media`, `@page`, right-to-left |

## Performance

Measured with [k6](https://k6.io), same scenario as pdfjs (`bench/k6.js`: POST the three documents in `demo/` in turn, PDF response). **Everything runs in Docker on the same VM** (12 CPUs, k6 included) for a like-for-like comparison: pdfrs 0.3.0 in its image, pdfjs 1.1.0 with `bench/server.ts` on `node:26-alpine` (`NODE_ENV=production`, `--max-old-space-size=128`). Each stage lasts 15 s; memory is the server container's peak (`docker stats`, rounded to the nearest GB above 1 GB). The five configurations are measured in the same run: compare them with each other, not with another run (the machine's speed varies from one run to the next, by 15 to 25 % for both servers).

![Throughput by number of concurrent users: pdfrs climbs to ~6,000 PDF/s, pdfjs tops out at ~1,400 PDF/s with 12 Node processes and ~280 PDF/s with one](bench/chart.svg)

![Peak memory: ~150 MB for pdfrs on 12 cores, ~25 MB on one core, ~210 MB for Node × 1, ~2 GB for Node × 12](bench/memory.svg)

| Server | Max throughput | p95 at 10 VUs | p95 at 200 VUs | Memory at 200 VUs | Errors |
|---|---|---|---|---|---|
| **pdfrs, 12 threads** | **6,044 PDF/s** | **4 ms** | **43 ms** | 152 MB | 0 % |
| pdfjs, Node × 12 (cluster) | 1,414 PDF/s | 18 ms | 289 ms | ~2 GB | 0 % |
| pdfjs, Node × 1 | 280 PDF/s | 52 ms | 1,475 ms | 213 MB | 0 % |
| **pdfrs, 1 core** (`--cpus 1`, `WORKERS=1`) | **1,057 PDF/s** | 16 ms | 271 ms | **29 MB** | 0 % |
| pdfjs, Node × 1 on 1 core | 225 PDF/s | 77 ms | 4,677 ms | 203 MB | 0 % |

- **Core for core, pdfrs renders ~4.7× more PDFs than pdfjs, with ~7× less memory.** On the whole machine: ~4.3× the throughput of clustered Node, for ~13× less memory.
- Rendering is CPU-bound: render threads (`WORKERS`, one per core by default) work in parallel within a single process, without duplicating fonts or the runtime; extra requests wait their turn, so latency rises without losing throughput.
- Under load, Node × 1 loses throughput (GC) as the queue grows (225 → 89 PDF/s at 200 users on one core); pdfrs stays stable.
- The features added since 0.1 (grid, gradients, shadows, counters, floats, named pages, remote resources…) cost ~2–3 % per document on these documents (`examples/bench.rs`, 0.2.0 against 0.3.0, same machine and run).
- The image sets `MIMALLOC_PURGE_DELAY=0` and `MIMALLOC_ARENA_EAGER_COMMIT=0`: the allocator returns freed memory right away (−50 % memory under load, same throughput).
- Container startup: a few milliseconds; ~80 MB distroless image (binary with its HTTPS client + 14 fonts, glibc, CA certificates, tzdata), ~25 MB of memory at idle.

To reproduce (Docker is enough, with pdfjs cloned alongside):

```bash
PDFJS_DIR=../pdfjs sh bench/sweep.sh > bench/results.csv   # ~10 min: rs, js1, jsN, rs1, js1c
node bench/chart.ts                                         # → bench/chart.svg, memory.svg, latency.svg
```

## Development

No need for Rust locally: everything runs in Docker.

```bash
# dev container, cargo caches in volumes
docker run -d --name pdfrs-dev -v "$PWD":/src -v pdfrs-cargo:/usr/local/cargo/registry -v pdfrs-target:/src/target \
  -w /src rust:alpine sleep infinity
docker exec pdfrs-dev apk add musl-dev font-liberation font-dejavu
docker exec pdfrs-dev cargo test
docker exec pdfrs-dev cargo run --release -- demo/report.html report.pdf
```

With Rust installed, `cargo test` and `cargo run` work the same way, with `FONTS_DIR` pointing to a font directory. `cargo run --release --example bench` times the benchmark documents without HTTP (µs per document).

| File | |
|---|---|
| `src/css.rs` | stylesheets, declarations, specificity, cascade |
| `src/style.rs` | computed style: inheritance, units, shorthands (`margin`, `border`, `flex`, `font`…) |
| `src/layout.rs` | DOM → Taffy box tree, Parley text, tables as grids, lists, images |
| `src/render.rs` | pagination and krilla drawing (backgrounds, borders, text, images, SVG, links) |
| `src/lib.rs` | `html_to_pdf(html, &Options)`: pages, header / footer, metadata |
| `src/main.rs` | HTTP server (axum) and command line |

## License

[MIT](LICENSE)
