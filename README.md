# Finaryx
Finaryx is a modern, open-source personal finance application for Linux. Track bank accounts, manage transactions, monitor expenses, and take control of your finances through a clean, intuitive interface.

---

Application GNOME de suivi de comptes bancaires : import de CSV de n'importe quelle banque (mappage des colonnes), base SQLite locale, évolution du mois, répartition des dépenses, cumul, prévision, catégories personnalisées, export de la base. Interface en français. Aucune donnée n'est envoyée en ligne.

État : **maquettes et cahier des charges, pas encore de code.** Dans les documents, l'application s'appelle encore « Carnet » (nom de travail).

## Contenu
- `CARNET-SPEC.md` : cahier des charges (données, import générique, écrans, prévision, export, catégories, design, plan de travail).
- `CLAUDE.md` : consignes pour Claude Code.
- `PROMPT-DEMARRAGE.md` : premier message à donner à Claude Code.
- `data/` : table de correspondance des catégories de la Banque Populaire vers celles de l'application.
- `design/` : maquettes (`.dc.html`, à lire comme des plans) en GNOME/libadwaita, et en variante Material 3 (`M3-*`, `m3.css`).
- `reference/` : rendu cible des info-bulles.

Ne jamais committer un export bancaire réel.
