# Finaryx

Application GNOME de suivi de comptes bancaires (import CSV, base SQLite locale, graphiques, prévision). Interface en français, code et commentaires en anglais.

## À lire en premier
- `FINARYX-SPEC.md` : cahier des charges complet (données, import, écrans, prévision, design, plan de travail). C'est la référence.
- `design/` : sources de la maquette (balisage et styles en ligne, `theme.css` pour les couleurs). À lire comme des plans, pas à ouvrir dans un navigateur.
- `data/banque_populaire_category_map.csv` : table de correspondance catégories de la banque → catégories de Finaryx.
- `reference/tooltips.png` : rendu cible des info-bulles.

## Règles du projet
- GNOME HIG et libadwaita d'abord : composants natifs (`AdwHeaderBar`, `AdwNavigationSplitView`, `AdwPreferencesPage`, `AdwToastOverlay`, `AdwStatusPage`), pas de widget maison quand un composant existe.
- `core/` ne dépend pas de GTK et reste testé ; `ui/` ne contient pas de logique métier.
- Montants en centimes (entiers), jamais en flottants. Dépenses négatives.
- Couleurs de catégories : 12 teintes, ordre et teintes fixes (voir le tableau du cahier des charges), jamais recalculées selon le rang, plus le gris « Autres » (groupe 13, jamais stocké en base). Lire les couleurs depuis le thème, pas en dur.
- Aucune requête réseau, aucune télémétrie. Les données restent locales.
- Un import est atomique et réimporter le même fichier ne crée aucune opération.
- Chaque graphique a un libellé accessible et une version texte (liste ou tableau). Aucune information portée par la seule couleur.
- Textes de l'interface en français, majuscule en début de phrase seulement, montants au format `fr-FR`.

## Avant de coder les importeurs
Le format **Banque Populaire** est vérifié sur un export réel (voir `FINARYX-SPEC.md`, section Import CSV, et `data/banque_populaire_category_map.csv`). Pour **Boursorama** et **Revolut**, demander à l'utilisateur un exemple anonymisé : leurs en-têtes dans le cahier des charges ne sont pas vérifiés. Tant qu'il n'y en a pas, aucun profil n'est livré pour ces banques : leurs fichiers passent par le mappage manuel. Ne jamais committer un export bancaire réel.

## Données personnelles
Ne jamais utiliser, citer ni committer de données issues des exports bancaires de l'utilisateur (montants, libellés, nombres de lignes, noms d'organismes…), y compris dans le cahier des charges, les tests ou les données de démonstration. Les données de démonstration viennent uniquement des chiffres d'exemple de la maquette, eux-mêmes inventés (les maquettes ont été nettoyées). Les fichiers de test de l'import sont fabriqués de toutes pièces.

## Décisions prises avec l'utilisateur (section 9 du cahier des charges)
- **Pile : Rust** (gtk4-rs 0.9 + libadwaita-rs 0.7, GTK ≥ 4.14, libadwaita ≥ 1.5), Meson + Flatpak. Même architecture et même schéma de données que le cahier des charges : `core/` est la crate `finaryx-core` (sans GTK), `src/` est l'interface. Tests : `cargo test` (remplace pytest).
- **Multi-devises : chaque compte dans sa propre devise**, totaux séparés par devise, aucune conversion. Montants en unités mineures (centimes pour l'euro). Formatage : `finaryx_core::money`.
- **Virements entre comptes du foyer** : colonne `transfer_pair_id` dans `txn` dès le premier schéma ; rapprochement proposé (montants opposés, comptes différents, ±3 jours, noms du foyer), jamais appliqué sans confirmation.
- **Pas de chiffrement** pour l'instant (rappel dans « À propos » et l'export) ; ouverture de la base isolée dans `core` pour pouvoir passer à SQLCipher plus tard.
- Maquette vs HIG : entrée « Mois en détail » (et non « Liste des opérations ») ; un seul menu principal, dans l'en-tête de la barre latérale, déplacé dans l'en-tête du contenu en largeur étroite ; tailles libadwaita (pas 44 px) ; « Dernier import » = simple étiquette `dim-label`.
- Identifiant d'application : `io.github.docteurscience.Finaryx` (`.Devel` en profil développement).

## Organisation du code
- `core/src/` : logique métier testée — `db.rs` (ouverture, `application_id`, migrations dans `MIGRATIONS`, ne jamais modifier une migration publiée), `models.rs`, `repo.rs` (requêtes ; agrégats en SQL), `normalize.rs` (`label_norm`, clé anti-doublon), `money.rs` (format et saisie des montants), `dates.rs`, `forecast.rs` (prévision du §5 : séries récurrentes, dépenses variables en moyenne tronquée, saisonnalité à partir de 12 mois, fourchette à 80 % ; « aujourd'hui » passé en paramètre, mois en cours pris en compte avec ce qui est déjà passé), `accounts.rs` (page Comptes : aperçus, état des imports, parts, création, modification, archivage, suppression, annulation d'un import), `scale.rs` (graduations), `stats.rs` (agrégats en SQL sur un périmètre de comptes d'une même devise : totaux, solde jour par jour, dépenses par catégorie et par mois, regroupement en 12 couleurs + « Autres » pour les graphiques, liste filtrée des opérations), `categories.rs` (catégories : création, couleurs, fusion, suppression ; règles et leur application aux opérations existantes sauf corrections manuelles), `transfers.rs` (virements entre comptes : propositions à ±3 jours, confirmation, refus mémorisé ; noms du foyer), `categorize.rs` (correction manuelle connue → règles → catégorie de la banque → mots-clés → « Autres » à vérifier), `importers/` (`format.rs` : encodage, séparateur, en-têtes, décimales, dates ; `mapping.rs` : champs, modes de montant, signature, propositions, lecture d'une ligne ; `profiles.rs` : profil Banque Populaire livré et profils de l'utilisateur ; `bank_categories.rs` : table de correspondance ; `plan.rs` : analyse, aperçu, contrôle du solde, import atomique), `demo.rs` (données de démonstration ; `demo::today()` fixe « aujourd'hui » au 2 novembre 2026 pour que la démonstration ne dépende pas de l'horloge).
- `src/` : `application.rs` (actions globales, option `--demo`), `database.rs` (emplacement de la base), `window.rs` (navigation, accueil, rechargement, actions sur les comptes), `pages/` (`categories.rs`, `overview.rs`, `month.rs`, `cumulative.rs`, `forecast.rs` (page Prévisions, graphique du solde réutilisé par la vue d'ensemble), `operations.rs`, `accounts.rs`, `account_detail.rs`, `import.rs`, aides communes dans `mod.rs` dont la fenêtre « Détail de « Autres » »), `account_dialog.rs`, `category_dialog.rs`, `charts/` (Cairo : `line.rs`, `bars.rs`, `donut.rs`, `columns.rs`, `sparkline.rs`, `share_bar.rs` ; couleurs lues dans le thème par `charts::theme_color`), `preferences.rs`, `settings.rs`.
- Filtre de compte : un seul menu déroulant maître (Vue d'ensemble) que les autres pages reflètent ; « Tous les comptes » = comptes non archivés de la devise la plus fréquente (jamais de somme entre devises).
- Deux points de rupture : sous 860 sp les pages passent en disposition compacte (barre latérale visible) ; sous 600 sp la barre latérale se replie et la barre d'onglets apparaît.
- Largeur étroite : la fenêtre prévient les pages (`set_narrow`) quand le point de rupture s'applique ; la pile racine n'est pas homogène pour que la page d'accueil masquée n'impose pas sa largeur.
- `data/` : `ui/*.ui` (GtkBuilder), `style.css` / `style-dark.css` (couleurs de données reprises de `design/theme.css` et complétées par les couleurs 8 à 12 du cahier des charges, nommées `finaryx_c1`…`finaryx_c13`), icônes, schéma GSettings, `.desktop`, metainfo. Ressources compilées par `build.rs` et intégrées au binaire.
- Icônes de navigation : générées depuis les tracés de la maquette par `build-aux/gen-icons.py`.

## Commandes
- Dépendances (Debian/Ubuntu) : `libgtk-4-dev libadwaita-1-dev meson ninja-build desktop-file-utils appstream gettext`, plus Rust stable.
- Lancer en développement : `cargo run` (le schéma GSettings est compilé dans `target/` par `build.rs`). Base : `~/.local/share/finaryx/finaryx.db` (`finaryx-devel/` pour les builds de développement).
- Données de démonstration (en mémoire, rien n'est écrit) : `cargo run -- --demo`.
- Ouvrir directement un fichier à importer : `cargo run -- chemin/vers/export.csv` (aussi depuis le gestionnaire de fichiers).
- Fichiers de test de l'import : `core/tests/fixtures/` (inventés, voir le README du dossier) ; tests dans `core/tests/import.rs`.
- Tests de la logique : `cargo test --workspace`.
- Build Meson + tous les tests (cargo, `.desktop`, metainfo, schéma) : `meson setup build -Dprofile=development && meson test -C build`, puis `./build/src/finaryx`.
- Flatpak (développement) : `flatpak-builder --user --install --force-clean _build build-aux/io.github.docteurscience.Finaryx.Devel.json`. Le manifeste autorise le réseau **pendant la compilation** seulement (téléchargement des crates) ; l'application elle-même n'a pas accès au réseau. Le manifeste de publication (crates vendorisées) viendra à l'étape 9.
