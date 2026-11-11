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
- Couleurs de catégories : ordre et teintes fixes (voir le tableau du cahier des charges), jamais recalculées selon le rang. Lire les couleurs depuis le thème, pas en dur.
- Aucune requête réseau, aucune télémétrie. Les données restent locales.
- Un import est atomique et réimporter le même fichier ne crée aucune opération.
- Chaque graphique a un libellé accessible et une version texte (liste ou tableau). Aucune information portée par la seule couleur.
- Textes de l'interface en français, majuscule en début de phrase seulement, montants au format `fr-FR`.

## Avant de coder les importeurs
Le format **Banque Populaire** est vérifié sur un export réel (voir `FINARYX-SPEC.md`, section Import CSV, et `data/banque_populaire_category_map.csv`). Pour **Boursorama** et **Revolut**, demander à l'utilisateur un exemple anonymisé : leurs en-têtes dans le cahier des charges ne sont pas vérifiés. Ne jamais committer un export bancaire réel.

## Commandes
À compléter après le squelette Meson : lancer l'application, lancer `pytest`, construire le Flatpak.
