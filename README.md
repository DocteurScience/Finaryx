# Finaryx
Finaryx is a modern, open-source personal finance application for Linux. Track bank accounts, manage transactions, monitor expenses, and take control of your finances through a clean, intuitive interface.

---

Application GNOME de suivi de comptes bancaires : import de CSV de n'importe quelle banque (mappage des colonnes), base SQLite locale, évolution du mois, répartition des dépenses, cumul, prévision, catégories personnalisées, export de la base. Interface en français. Aucune donnée n'est envoyée en ligne.

État : **étape 4 du plan** — navigation, thème clair/sombre, base SQLite avec migrations, données de démonstration (`--demo`), page Comptes (ajout, modification, archivage, suppression, annulation d’un import), import CSV générique (détection du format, mappage des colonnes, profils, aperçu, doublons, import atomique). Les graphiques viennent ensuite.

## Compiler et lancer
```
cargo run                      # développement
cargo run -- --demo            # données de démonstration (rien n'est enregistré)
cargo test --workspace         # tests
meson setup build -Dprofile=development && meson test -C build && ./build/src/finaryx
```
Dépendances : GTK ≥ 4.14, libadwaita ≥ 1.5, Meson, Rust stable. Voir `CLAUDE.md` pour le Flatpak.

## Contenu
- `FINARYX-SPEC.md` : cahier des charges (données, import générique, écrans, prévision, export, catégories, design, plan de travail).
- `CLAUDE.md` : consignes pour Claude Code.
- `PROMPT-DEMARRAGE.md` : premier message à donner à Claude Code.
- `core/` : logique métier (crate Rust sans GTK) ; `src/` : interface GTK ; `data/` : interface (`.ui`), styles, icônes, métadonnées ; `build-aux/` : manifeste Flatpak et scripts.
- `data/` : table de correspondance des catégories de la Banque Populaire vers celles de l'application.
- `design/` : maquettes (`.dc.html`, à lire comme des plans) en GNOME/libadwaita, et en variante Material 3 (`M3-*`, `m3.css`).
- `reference/` : rendu cible des info-bulles.

Ne jamais committer un export bancaire réel.
