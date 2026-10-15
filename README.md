# pdfrs

Génère des PDF depuis du HTML/CSS, **sans navigateur** : un portage en Rust de [`@gouz/pdfjs`](https://github.com/gouz/pdfjs), livré comme une petite API HTTP dans une image Docker de ~20 Mo, **~4,5× plus rapide par cœur et ~7× plus sobre en mémoire** (voir [Performances](#performances)).

- Parsing HTML par [html5ever](https://github.com/servo/html5ever) (celui de Servo), sélecteurs CSS par [`selectors`](https://crates.io/crates/selectors)
- Mise en page par [Taffy](https://github.com/DioxusLabs/taffy) : bloc (avec fusion des marges), flexbox, **grid**, float — l'équivalent de Yoga dans react-pdf
- Texte par [Parley](https://github.com/linebender/parley) : shaping, coupure de lignes Unicode, justification, repli de police par caractère
- PDF par [krilla](https://github.com/LaurenzV/krilla) (le moteur PDF de Typst) : polices embarquées en sous-ensemble, texte sélectionnable, liens cliquables

## Démarrer

```bash
docker run -p 3000:3000 ghcr.io/gouz/pdfrs:main
# ou depuis les sources
docker build -t pdfrs . && docker run -p 3000:3000 pdfrs
```

L'image (amd64 et arm64) est publiée par la CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) : `:main` et `:sha-xxxxxxx` à chaque push sur `main`, `:1.2.3`, `:1.2` et `:latest` pour un tag `v1.2.3`. Les pull requests lancent les tests et construisent l'image sans la publier.

```bash
# HTML brut dans le corps, options dans l'URL
curl --data-binary @facture.html 'http://localhost:3000/?size=A4&margin=40' -o facture.pdf

# ou en JSON
curl -H 'content-type: application/json' \
  -d '{"html": "<h1>Bonjour</h1>", "footer": "<p style=\"text-align:right\">{page} / {pages}</p>"}' \
  http://localhost:3000/ -o bonjour.pdf
```

| Route | |
|---|---|
| `POST /` ou `POST /pdf` | corps : le HTML (`text/html`, ou n'importe quoi), ou `{"html": …, …options}` (`application/json`). Réponse : `application/pdf` |
| `GET /health` | `ok` |
| `GET /` | aide |

Erreurs : `400` pour une option invalide (le message dit laquelle), `500` si le rendu échoue.

### Options

En paramètres d'URL ou en champs JSON :

| Option | Exemple | |
|---|---|---|
| `size` | `A5`, `LETTER`, `400x600` | format de page (`A4` par défaut, `A3`, `A6`, `B5`, `LEGAL`, `TABLOID`, ou `LARGEURxHAUTEUR` en pt) |
| `landscape` | `true` | orientation paysage |
| `margin` | `40`, `60 40 50 40` | marges en pt (40 par défaut), ordre CSS haut droite bas gauche, virgules acceptées |
| `font` | `Times-Roman` | police par défaut (`Helvetica` par défaut) |
| `fontSize` | `11` | taille par défaut (12) |
| `header` | `<p>Rapport</p>` | en-tête répété sur chaque page, centré verticalement dans la marge haute |
| `footer` | `<p>{page} / {pages}</p>` | pied de page répété sur chaque page |
| `title`, `author`, `subject`, `keywords` | | métadonnées du PDF (`title` reprend `<title>` à défaut) |

### Variables d'environnement

| Variable | Défaut | |
|---|---|---|
| `PORT` | `3000` | |
| `WORKERS` | nombre de cœurs | threads de rendu ; les requêtes en plus attendent leur tour |
| `FONTS_DIR` | `/fonts` | dossiers de polices (`.ttf`, `.otf`, `.ttc`, récursif), séparés par `:` |
| `IMAGES_DIR` | — | dossier des images locales, voir [Images](#images) |
| `MAX_BODY_MB` | `20` | taille max d'une requête |

## Ligne de commande

Le même binaire convertit un fichier :

```bash
cargo run --release -- facture.html facture.pdf --size A5 --footer '<p>{page} / {pages}</p>'
# ou dans l'image
docker run --rm -v "$PWD":/w -w /w --entrypoint /pdfrs pdfrs facture.html facture.pdf
```

Options : `--size`, `--landscape`, `--margin`, `--font`, `--font-size`, `--header`, `--footer`, `--title`. Les images locales sont autorisées depuis le dossier du fichier HTML.

## Polices

L'image embarque **Liberation Sans / Serif / Mono**, aux métriques identiques à Helvetica / Times / Courier (un document occupe la même place qu'avec pdfjs), et **DejaVu Sans** en repli pour les caractères manquants (flèches, symboles, grec, cyrillique…).

- `Helvetica`, `Arial`, `sans-serif`, `system-ui` → Liberation Sans ; `Times-Roman`, `serif` → Liberation Serif ; `Courier`, `monospace` → Liberation Mono.
- Comme dans un navigateur, une pile `font-family: "Segoe UI", Roboto, sans-serif` prend la première police disponible. Si aucune ne l'est, la déclaration est ignorée et la police du parent s'applique.
- Pour ajouter des polices, les monter dans le conteneur : `docker run -v ./fonts:/fonts/extra pdfrs` (tout `FONTS_DIR` est parcouru). Elles sont utilisables par leur nom de famille, en CSS comme dans l'option `font`. Le gras et l'italique utilisent les fichiers correspondants de la famille quand ils existent.
- Pas de police CJK dans l'image : ajouter par exemple Noto Sans CJK dans `FONTS_DIR` pour le chinois, le japonais ou le coréen.

## Images

Le HTML peut venir d'une source non fiable, donc **par défaut seules les images `data:` sont chargées**.

| Source | Autorisée si… |
|---|---|
| `data:image/...` | toujours |
| `logo.png`, `img/a.jpg` | `IMAGES_DIR` est défini, et le fichier est **dans** ce dossier (`../`, chemins absolus et liens symboliques qui en sortent sont refusés) |
| `https://...` | jamais : le rendu ne fait aucune requête réseau (inclure l'image en `data:`) |
| `file://`, autres | jamais |

```bash
docker run -p 3000:3000 -v ./templates/img:/images:ro -e IMAGES_DIR=/images pdfrs
```

Une image refusée ou introuvable n'est pas affichée et un avertissement est écrit sur la sortie d'erreur.

Formats : PNG, JPEG, GIF, WebP et SVG (dans `<img>` ou directement `<svg>…</svg>` dans le HTML). Une image sans taille garde sa taille naturelle (1 px = 1 pt), sans dépasser la largeur disponible ; `width` / `height` (attributs ou CSS) sont respectés, avec conservation des proportions si un seul est donné. `object-fit` et `border-radius` sont pris en charge.

## CSS pris en charge

Comme dans pdfjs, **1 px = 1 pt**.

- **Mise en page** : `display` `block` / `flex` / `grid` / `none` / `contents` / `list-item` / `table…`, marges qui fusionnent comme dans un navigateur, `float` / `clear`, `position: absolute` (relatif au parent).
- **Flexbox** complet : `flex-direction`, `flex-wrap`, `flex` / `flex-grow` / `flex-shrink` / `flex-basis`, `justify-content`, `align-items`, `align-self`, `align-content`, `gap`, `order`.
- **Grid** : `grid-template-columns` / `-rows` (`fr`, `repeat()`, `minmax()`, `auto`…), `grid-column` / `grid-row`, `grid-auto-flow`, `gap`, `justify-items`, `place-items`…
- **Boîtes** : `width` / `height` (+ `min-` / `max-`), `box-sizing`, `margin`, `padding`, `border` (largeur, `solid` / `dashed` / `dotted`, couleur par côté), `border-radius`, `background-color` (ou la couleur de `background`), `opacity`, `overflow: hidden`, `aspect-ratio`.
- **Texte** : `font` / `font-family` / `font-size` / `font-weight` / `font-style`, `color`, `text-align` (dont `justify`), `line-height`, `text-decoration`, `text-transform`, `letter-spacing`, `word-spacing`, `white-space` (`pre`, `nowrap`…), fond des éléments en ligne (`<mark>`, `<span style="background">`).
- **Unités** : `px`, `pt`, `em`, `rem`, `%`, `mm`, `cm`, `in`, `pc`. Couleurs : noms, `#hex`, `rgb()`, `hsl()`…
- **Pagination** : coupure automatique entre deux lignes de texte (jamais au milieu d'une ligne ou d'une image), `page-break-before` / `-after: always` (ou `break-before: page`), `page-break-inside: avoid`.
- **HTML** : tableaux (grille avec `colspan` / `rowspan`, `border-collapse`, `border-spacing`, `vertical-align`), listes (`disc`, `circle`, `square`, `decimal`, `lower-alpha`, `upper-roman`…, `<ol start>`, `<li value>`), liens cliquables (`http(s):`, `mailto:`, `tel:`), `<br>`, `<pre>`, `<svg>`, styles par défaut d'un navigateur (`h1`…`h6`, `b`, `i`, `code`…), attributs `align` / `bgcolor`.
- `<style>` avec tous les sélecteurs CSS (`>`, `+`, `~`, `:first-child`, `:nth-child()`, `[attr]`, `:not()`…), spécificité et `!important`.

**Non pris en charge** : `background-image` et dégradés, `box-shadow`, `transform`, pseudo-éléments (`::before`), `@media`, `@font-face` (monter les polices dans `FONTS_DIR`), `@page`, feuilles de style externes `<link>`, variables CSS `var()`, `calc()`, `inline-block` (traité comme `inline` : le texte reste fluide, padding et bordure ignorés), images au milieu d'une ligne de texte (elles passent à la ligne), texte qui contourne un `float`, répétition de `<thead>` sur chaque page, texte de droite à gauche non testé.

**Différences avec pdfjs** :
- Les marges verticales fusionnent comme dans un navigateur (react-pdf les additionne) : les paragraphes sont un peu plus serrés.
- Pas de boucle infinie ni de crash sur `min-height` plus grand que la page, `gap: normal`, `border: medium` ou une police inconnue : rien à contourner.
- Pas d'images distantes.

## Performances

Mesuré avec [k6](https://k6.io), même scénario que pdfjs (`bench/k6.js` : POST des trois documents de `demo/` en alternance, réponse en PDF). **Tout tourne dans Docker sur la même VM** (12 CPU, k6 compris) pour comparer à armes égales : pdfrs dans son image, pdfjs 1.1.0 avec `bench/server.ts` sous `node:26-alpine` (`NODE_ENV=production`, `--max-old-space-size=128`). Chaque palier dure 15 s ; la mémoire est le pic du conteneur serveur (`docker stats`, arrondi au Go près au-delà de 1 Go).

![Débit selon le nombre d'utilisateurs simultanés : pdfrs monte à ~6 900 PDF/s, pdfjs plafonne à ~1 900 PDF/s avec 12 process Node et ~330 PDF/s avec un seul](bench/chart.svg)

![Mémoire max : ~140 Mo pour pdfrs sur 12 cœurs, ~25 Mo sur un cœur, ~210 Mo pour Node × 1, ~2 Go pour Node × 12](bench/memory.svg)

| Serveur | Débit max | p95 à 10 VUs | p95 à 200 VUs | Mémoire à 200 VUs | Erreurs |
|---|---|---|---|---|---|
| **pdfrs, 12 threads** | **6 912 PDF/s** | **4 ms** | **36 ms** | 143 Mo | 0 % |
| pdfjs, Node × 12 (cluster) | 1 957 PDF/s | 12 ms | 232 ms | ~2 Go | 0 % |
| pdfjs, Node × 1 | 339 PDF/s | 41 ms | 1 735 ms | 216 Mo | 0 % |
| **pdfrs, 1 cœur** (`--cpus 1`, `WORKERS=1`) | **1 210 PDF/s** | 15 ms | 246 ms | **28 Mo** | 0 % |
| pdfjs, Node × 1 sur 1 cœur | 266 PDF/s | 70 ms | 3 783 ms | 210 Mo | 0 % |

- **À cœur égal, pdfrs rend ~4,5× plus de PDF que pdfjs, avec ~7× moins de mémoire.** Sur toute la machine : ~3,5× le débit de Node en cluster, pour ~15× moins de mémoire.
- Le rendu est CPU-bound : les threads de rendu (`WORKERS`, un par cœur par défaut) travaillent en parallèle dans un seul process, sans dupliquer les polices ni le runtime ; les requêtes en surplus attendent leur tour, d'où la latence qui monte sans perte de débit.
- Sous charge, Node × 1 perd du débit (GC) quand la file s'allonge, pdfrs reste stable.
- L'image règle `MIMALLOC_PURGE_DELAY=0` et `MIMALLOC_ARENA_EAGER_COMMIT=0` : l'allocateur rend la mémoire libérée tout de suite (−50 % de mémoire sous charge, débit identique).
- Démarrage du conteneur : quelques millisecondes ; image de 20 Mo (binaire statique + 14 polices), ~25 Mo de mémoire au repos.

Pour reproduire (Docker suffit, pdfjs cloné à côté) :

```bash
PDFJS_DIR=../pdfjs sh bench/sweep.sh > bench/results.csv   # ~10 min : rs, js1, jsN, rs1, js1c
node bench/chart.ts                                         # → bench/chart.svg, memory.svg, latency.svg
```

## Développement

Pas besoin de Rust en local : tout tourne dans Docker.

```bash
# conteneur de dev, caches cargo dans des volumes
docker run -d --name pdfrs-dev -v "$PWD":/src -v pdfrs-cargo:/usr/local/cargo/registry -v pdfrs-target:/src/target \
  -w /src rust:alpine sleep infinity
docker exec pdfrs-dev apk add musl-dev font-liberation font-dejavu
docker exec pdfrs-dev cargo test
docker exec pdfrs-dev cargo run --release -- demo/report.html report.pdf
```

Avec Rust installé : `cargo test` et `cargo run` fonctionnent pareil, avec `FONTS_DIR` pointant sur un dossier de polices.

| Fichier | |
|---|---|
| `src/css.rs` | feuilles de style, déclarations, spécificité, cascade |
| `src/style.rs` | style calculé : héritage, unités, raccourcis (`margin`, `border`, `flex`, `font`…) |
| `src/layout.rs` | DOM → arbre de boîtes Taffy, texte Parley, tableaux en grilles, listes, images |
| `src/render.rs` | pagination et dessin krilla (fonds, bordures, texte, images, SVG, liens) |
| `src/lib.rs` | `html_to_pdf(html, &Options)` : pages, en-tête / pied de page, métadonnées |
| `src/main.rs` | serveur HTTP (axum) et ligne de commande |

## Licence

[MIT](LICENSE)
