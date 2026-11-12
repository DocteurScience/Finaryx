# Traduire Finaryx

La langue source est le français. L’anglais est complet. Le tableau
`translations.csv` prépare seize autres langues :

| Code | Langue | Formes plurielles (form 0 / 1 / 2) |
|---|---|---|
| `en` | English | 1 / 0, 2+ |
| `de` | Deutsch | 1 / 0, 2+ |
| `es` | Español | 1 / 0, 2+ |
| `it` | Italiano | 1 / 0, 2+ |
| `pt_BR` | Português (Brasil) | 0, 1 / 2+ |
| `pt_PT` | Português (Portugal) | 1 / 0, 2+ |
| `nl` | Nederlands | 1 / 0, 2+ |
| `pl` | Polski | 1 / 2–4, 22–24… / 0, 5–21… |
| `cs` | Čeština | 1 / 2–4 / 0, 5+ |
| `sv` | Svenska | 1 / 0, 2+ |
| `da` | Dansk | 1 / 0, 2+ |
| `nb` | Norsk bokmål | 1 / 0, 2+ |
| `fi` | Suomi | 1 / 0, 2+ |
| `uk` | Українська | 1, 21… / 2–4, 22–24… / 0, 5–20… |
| `tr` | Türkçe | 1 / 0, 2+ |
| `ja` | 日本語 | toutes (form 0 seulement) |
| `zh_CN` | 简体中文 | toutes (form 0 seulement) |

## Le tableau

`translations.csv` s’ouvre dans un tableur (UTF-8, séparateur virgule). Une
ligne par message, une colonne par langue :

- `msgid` : le texte français ; `msgid_plural` : sa forme plurielle s’il en a une ;
- `form` : vide pour un message simple ; 0, 1 ou 2 pour les trois lignes d’un
  message au pluriel (chaque langue remplit les formes de sa règle, voir le
  tableau ci-dessus ; la colonne `en` en donne un exemple) ;
- `comment` : où le message apparaît, et les consignes éventuelles.

Une cellule vide garde le français. Un pluriel incomplet est ignoré en entier.

À respecter :

- garder les marqueurs entre accolades (`{n}`, `{month}`, `{amount}`…) et les
  balises (`<b>…</b>`) ; l’ordre peut changer ;
- `{of_month}` est le nom du mois : écrire par exemple « Spending in {of_month} » ;
- `{day}` est un numéro de jour : « le {day} » se traduit « on day {day} » ;
- dates, montants et pourcentages sont mis en forme par l’application selon la
  langue (`core/src/locale.rs`) : ne pas les écrire dans les traductions ;
- les noms de catégories d’origine sont traduits ici ; ceux que l’utilisateur
  crée ou renomme restent tels quels.

## Commandes

```
meson compile -C build finaryx-pot        # met à jour po/finaryx.pot depuis le code
python3 build-aux/translations.py update  # ajoute les nouveaux messages au tableau
python3 build-aux/translations.py po      # écrit po/<code>.po et po/LINGUAS
python3 build-aux/translations.py check   # marqueurs, balises, couverture
python3 build-aux/translations.py stats   # avancement par langue
```

Les fichiers `.po` sont générés : on modifie le tableau, pas les `.po`. Une
langue entre dans `LINGUAS` dès qu’elle a une traduction. `meson test` vérifie le
tableau et chaque `.po`.

Pour ajouter une langue absente : l’ajouter dans `LANGUAGES` de
`build-aux/translations.py` (nom et règle de pluriel) et dans `LOCALES` de
`core/src/locale.rs` (mois, formats de date et de montant), puis lancer
`update`.
