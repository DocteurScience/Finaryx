# Finaryx
Finaryx is a modern, open-source personal finance application for Linux. Track bank accounts, manage transactions, monitor expenses, and take control of your finances through a clean, intuitive interface.

---

Application GNOME de suivi de comptes bancaires : import de CSV de n'importe quelle banque (mappage des colonnes), base SQLite locale, évolution du mois, répartition des dépenses, cumul, prévision, catégories personnalisées, export de la base. Interface en français. Aucune donnée n'est envoyée en ligne.

État : **étape 9 du plan** — navigation, thème clair/sombre, base SQLite avec migrations, données de démonstration (`--demo`), page Comptes (ajout, modification, archivage, suppression, annulation d’un import), import CSV générique (détection du format, mappage des colonnes, profils, aperçu, doublons, import atomique), vue d’ensemble, mois en détail, cumul et liste des opérations avec graphiques Cairo, catégories et règles, virements entre vos comptes, prévisions d’après vos habitudes ; info-bulles détaillées, largeur étroite jusqu’à 360 px, thème sombre et contraste élevé ; sauvegarde, restauration, sauvegarde automatique et export CSV ; manifeste Flatpak de publication et textes prêts à traduire.

## Compiler et lancer
```
cargo run                      # développement
cargo run -- --demo            # données de démonstration (rien n'est enregistré)
cargo test --workspace         # tests
meson setup build -Dprofile=development && meson test -C build && ./build/src/finaryx
```
Dépendances : GTK ≥ 4.14, libadwaita ≥ 1.5, Meson, Rust stable.

## Flatpak
- Développement (réseau autorisé pendant la compilation pour télécharger les crates) :
  `flatpak-builder --user --install --force-clean _build build-aux/io.github.docteurscience.Finaryx.Devel.json`
- Publication (Flathub, sans réseau) : `build-aux/io.github.docteurscience.Finaryx.json` construit l’étiquette `v0.1.0` du dépôt avec les crates déclarées dans `build-aux/cargo-sources.json`. Après tout changement de `Cargo.lock`, régénérer ce fichier : `python3 build-aux/flatpak-cargo-sources.py`.

L’application n’a jamais accès au réseau : ses fichiers passent par les portails (sélecteur de fichiers).

GitHub Actions construit le Flatpak à chaque push (workflow `Flatpak`, paquet dans les « Artifacts » du run).

## Publier une version
1. Mettre à jour le numéro (`Cargo.toml`, `meson.build`, metainfo, étiquette du manifeste de publication) et le texte de `.github/release-notes.md`.
2. Sur GitHub : *Actions* → *Flatpak* → *Run workflow* sur `main`, avec l’étiquette (par exemple `v0.1.0`). Le run construit le Flatpak, crée l’étiquette et la version, et y joint `finaryx.flatpak`. Il refuse une étiquette qui ne correspond pas à `Cargo.toml`.

## Traduire
Français (langue source), anglais, allemand, espagnol, italien, portugais du Brésil et néerlandais complets (traductions à faire relire par des locuteurs natifs). Le tableau `po/translations.csv` prépare les autres langues : une ligne par texte, une colonne par langue, à remplir dans un tableur. Les dates et les montants suivent la langue. Mode d’emploi : `po/README.md`.

## Contenu
- `FINARYX-SPEC.md` : cahier des charges (données, import générique, écrans, prévision, export, catégories, design, plan de travail).
- `CLAUDE.md` : consignes pour Claude Code.
- `PROMPT-DEMARRAGE.md` : premier message à donner à Claude Code.
- `core/` : logique métier (crate Rust sans GTK) ; `src/` : interface GTK ; `data/` : interface (`.ui`), styles, icônes, métadonnées ; `build-aux/` : manifestes Flatpak (développement et publication), sources des crates et scripts ; `data/screenshots/` : captures pour les boutiques d’applications (données de démonstration inventées).
- `data/` : table de correspondance des catégories de la Banque Populaire vers celles de l'application.
- `design/` : maquettes (`.dc.html`, à lire comme des plans) en GNOME/libadwaita, et en variante Material 3 (`M3-*`, `m3.css`).
- `reference/` : rendu cible des info-bulles.

Ne jamais committer un export bancaire réel.
