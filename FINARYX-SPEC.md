# Finaryx — cahier des charges pour Claude Code

Application de bureau GNOME de suivi de comptes bancaires. Elle importe des exports CSV d'un ou plusieurs comptes (banques différentes), enregistre les opérations dans sa propre base locale et les affiche en graphiques : évolution du mois, répartition des dépenses par mois, cumul sur plusieurs mois, prévision d'après les habitudes.

Langue de l'interface : **français**. Code, identifiants et commentaires : anglais.

La maquette de référence est dans `design/` (pages `.dc.html` + `theme.css`). Elle donne la structure, les textes et les mesures. Dossier `design/` : les fichiers source du design (balisage lisible, styles en ligne) ; ils ne s'affichent pas correctement seuls dans un navigateur, il faut les lire comme des plans. Les chiffres de la maquette sont des données d'exemple.

---

## 1. Stack recommandée

| Sujet | Choix |
|---|---|
| Langage | Python 3.12 |
| Interface | GTK 4 + libadwaita 1.5+ (PyGObject), conformité GNOME HIG |
| Base | SQLite (module `sqlite3`), fichier dans `GLib.get_user_data_dir()/finaryx/finaryx.db` |
| Graphiques | dessin maison avec `Gtk.DrawingArea` + Cairo (pas de bibliothèque web ni matplotlib) |
| Packaging | Meson + Flatpak (`org.gnome.Platform`) |
| Tests | pytest (logique) ; pas de test d'interface au début |
| i18n | `gettext`, source en français |

Si le projet existe déjà dans un autre langage (Rust/gtk-rs, Vala), garder la même architecture et le même schéma de données.

Séparer strictement : `core/` (sans GTK, testable) et `ui/` (GTK).

```
finaryx/
  core/
    db.py            # connexion, migrations
    models.py        # dataclasses Account, Transaction, Category, ImportBatch
    importers/       # lecteur CSV générique (détection, mappage, profils) ; aucun code par banque
    categorize.py    # règles de catégorisation
    stats.py         # agrégats : solde journalier, par catégorie, cumul
    forecast.py      # prévision
  ui/
    window.py        # AdwApplicationWindow, navigation, adaptatif
    pages/           # overview, month, cumulative, forecast, accounts, import_
    charts/          # line.py, donut.py, stacked_columns.py, sparkline.py
    style.css        # variables de couleur (voir §6)
  data/ (icône, .desktop, metainfo, schémas)
tests/
```

---

## 2. Modèle de données

Montants en **centimes (INTEGER)**, jamais en flottants. Dépenses négatives, revenus positifs.

```sql
CREATE TABLE bank (
  id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE          -- "Banque Populaire", "Boursorama", "Revolut"
);
CREATE TABLE account (
  id INTEGER PRIMARY KEY, bank_id INTEGER REFERENCES bank(id),
  name TEXT NOT NULL,                                        -- "Compte courant", "Livret A"…
  kind TEXT NOT NULL DEFAULT 'checking',                     -- checking | savings
  currency TEXT NOT NULL DEFAULT 'EUR',
  opening_balance_cents INTEGER NOT NULL DEFAULT 0,          -- solde avant la 1re opération importée
  color_slot INTEGER NOT NULL DEFAULT 1,                     -- 1..4 → --a1..--a4
  archived INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE category (
  id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE,
  slot INTEGER,                                              -- 1..7 → --c1..--c7, 8 = « Autres » (gris) ; NULL = sans couleur de graphique
  is_income INTEGER NOT NULL DEFAULT 0,
  is_builtin INTEGER NOT NULL DEFAULT 0,                     -- catégories d'origine ; « Autres » ne peut pas être supprimée
  exclude_from_stats INTEGER NOT NULL DEFAULT 0              -- 1 pour « Virement interne » : ni dépense ni revenu
);
CREATE TABLE import_batch (
  id INTEGER PRIMARY KEY, account_id INTEGER NOT NULL REFERENCES account(id),
  filename TEXT, imported_at TEXT NOT NULL,                  -- ISO 8601
  rows_read INTEGER, rows_new INTEGER, rows_duplicate INTEGER, rows_review INTEGER
);
CREATE TABLE txn (
  id INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL REFERENCES account(id),
  batch_id INTEGER REFERENCES import_batch(id),
  booked_on TEXT NOT NULL,                                   -- 'YYYY-MM-DD'
  label TEXT NOT NULL,                                       -- libellé affiché
  label_raw TEXT,                                            -- libellé brut de la banque (recherche)
  posted_on TEXT,                                            -- date de comptabilisation, si le fichier la donne
  reference TEXT,                                            -- référence de la banque (non unique !)
  bank_category TEXT,                                        -- catégorie et sous-catégorie de la banque, telles qu'importées
  label_norm TEXT NOT NULL,                                  -- majuscules, sans chiffres/dates/refs
  amount_cents INTEGER NOT NULL,
  category_id INTEGER REFERENCES category(id),
  category_source TEXT NOT NULL DEFAULT 'auto',              -- auto | rule | manual | bank
  needs_review INTEGER NOT NULL DEFAULT 0,
  dedupe_key TEXT NOT NULL,
  UNIQUE (account_id, dedupe_key)
);
CREATE INDEX txn_date ON txn(booked_on);
CREATE TABLE import_profile (                                -- « Retenir ce format » (mappage de colonnes)
  id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE,         -- "Banque Populaire", "Mon export Revolut"…
  bank_id INTEGER REFERENCES bank(id),                       -- liée à une banque, réutilisable sur plusieurs comptes
  signature TEXT NOT NULL UNIQUE,                            -- hash des noms de colonnes normalisés
  is_builtin INTEGER NOT NULL DEFAULT 0,
  mapping_json TEXT NOT NULL                                 -- voir « Profils » (§3) : encodage, séparateur, colonnes par nom, formats, catégories de la banque
);
CREATE TABLE rule (                                          -- règles de catégorisation
  id INTEGER PRIMARY KEY, pattern TEXT NOT NULL,             -- sous-chaîne ou regex sur label_norm
  is_regex INTEGER NOT NULL DEFAULT 0, category_id INTEGER NOT NULL REFERENCES category(id),
  priority INTEGER NOT NULL DEFAULT 100
);
```

Catégories initiales, **dans cet ordre fixe** (la couleur suit l'entité, jamais le rang). Les 7 premières ont une couleur de graphique (ce sont en général les postes les plus lourds) :
1 Logement · 2 Alimentation · 3 Transports · 4 Prêts et crédits · 5 Shopping et animaux · 6 Abonnements et télécoms · 7 Banque et assurances · 8 Autres.
Catégories **sans couleur** (regroupées dans « Autres » dans les graphiques, séparées partout ailleurs) : Sorties et loisirs · Santé · Impôts et taxes · Administratif et éducation. Catégories spéciales : « Revenus » (`is_income`) et « Virement interne » (`exclude_from_stats`).
Les maquettes (`design/`) montrent encore l'ancien jeu de 8 catégories avec des données d'exemple : le jeu ci-dessus est celui à implémenter.

**Solde d'un compte à une date** = `opening_balance_cents` + somme des opérations jusqu'à cette date. Le solde total est la somme des comptes non archivés. Si le CSV contient une colonne de solde, l'utiliser pour recaler `opening_balance_cents` (avec confirmation).

---

## 3. Import CSV

**Principe : l'importeur est générique.** Il ne contient pas de code par banque. Il lit n'importe quel CSV, montre les colonnes présentes et laisse l'utilisateur dire à quoi sert chacune (le mappage). Les « modèles de banque » (Banque Populaire, Boursorama, Revolut…) ne sont que des profils de mappage préenregistrés, du même type que ceux que l'utilisateur crée lui-même. Un format inconnu doit toujours pouvoir être importé sans toucher au code.

Flux (écran « Importer un CSV », page `Import.dc.html`) :

1. Choisir un fichier (`Gtk.FileDialog`, filtre `.csv`). Glisser-déposer accepté.
2. **Détection automatique** de : l'encodage (UTF-8 avec ou sans BOM, puis cp1252/ISO-8859-1), le séparateur (`;`, `,`, tabulation, `|`), la **ligne d'en-tête** (certaines banques ajoutent des lignes de métadonnées avant : prendre la première ligne dont le nombre de champs est stable pour les lignes suivantes), le séparateur décimal (`,` ou `.`), le format de date (`JJ/MM/AAAA`, `AAAA-MM-JJ`, `JJ-MM-AA`, `MM/JJ/AAAA` si un jour > 12 le prouve ; ambigu → demander). Chacun est modifiable dans la section « Format du fichier ».
3. **Reconnaissance** : la signature (hash des noms de colonnes normalisés : minuscules, sans accents ni espaces superflus, ordre ignoré) est comparée aux profils enregistrés. Profil trouvé → mappage pré-rempli, bandeau « Modèle reconnu : … ». Sinon → mappage manuel avec des propositions par noms de colonnes (ex. une colonne qui contient « date » → Date, « libell »/« label »/« description » → Libellé, « debit »/« credit »/« montant »/« amount »). Ne jamais supposer : l'utilisateur confirme.
4. **Mappage des colonnes** : chaque colonne du fichier reçoit un champ via un menu déroulant, avec un exemple de valeur. Champs disponibles :
   - **Date de l'opération** (obligatoire) · Date de comptabilisation · Date de valeur (facultatives, gardées si présentes) ;
   - **Libellé** (obligatoire) · Détail / libellé brut (facultatif, concaténé pour la recherche) ;
   - **Montant**, trois modes au choix : *une colonne signée* · *deux colonnes débit et crédit* (une seule remplie par ligne ; débit lu en négatif même s'il est écrit positif) · *une colonne de montant positif + une colonne de sens* (« D »/« C », « Debit »/« Credit », à mapper) ;
   - Référence de la banque (aide l'anti-doublon) · Solde après opération (contrôle) · Devise ;
   - **Catégorie de la banque** et **Sous-catégorie de la banque** (facultatives, voir étape 5) ;
   - **Ignorer** (par défaut pour toute colonne non reconnue).
   Validation avant de continuer : date, libellé et montant sont mappés ; les 20 premières lignes se lisent sans erreur ; message clair sinon (« 3 lignes ont une date illisible : ligne 12, 40, 41 »).
5. **Catégories de la banque** (si une colonne catégorie est mappée) : l'application liste les valeurs distinctes (ou couples catégorie + sous-catégorie) avec le nombre de lignes, et propose une catégorie de Finaryx pour chacune d'après le nom (correspondance exacte, puis mots proches, sinon « À vérifier »). L'utilisateur corrige dans la liste ; la correspondance est enregistrée avec le profil et réutilisée aux imports suivants. Les valeurs nouvelles apparaissant plus tard sont proposées au même endroit. Possibilité de marquer une valeur de la banque comme « Virement interne » (ni dépense ni revenu).
6. Choisir le compte de destination (proposer d'en créer un). Le profil est lié à une banque, pas à un compte : on peut le réutiliser pour plusieurs comptes.
7. Aperçu : chaque ligne reçoit un état **Nouveau**, **Doublon** ou **À vérifier** (date invalide, montant illisible, catégorie incertaine). Le solde de contrôle, s'il est mappé, doit rester cohérent d'une ligne à l'autre ; sinon avertir sans bloquer.
8. Résumé : lignes lues / nouvelles / doublons / à vérifier ; champ « Enregistrer ce format sous : » (nom proposé d'après le fichier, modifiable) et interrupteur « Retenir ce format » (écrit `import_profile`).
9. « Importer N opérations » : une seule transaction SQL ; création d'un `import_batch`. « Annuler » ne touche à rien.

**Profils** (`import_profile.mapping_json`) : version du schéma de profil, encodage, séparateur, nombre de lignes à sauter, format de date, séparateur décimal, mode de montant, champ → nom de colonne (par **nom**, jamais par position), table de correspondance des catégories de la banque, valeurs de sens. Un profil reste valable si l'ordre des colonnes change ; si une colonne mappée disparaît, repasser par l'écran de mappage. Prévoir « Gérer les formats » (liste, renommer, supprimer, exporter/importer un profil en JSON) dans les préférences. Profils livrés avec l'application : Banque Populaire (vérifié) ; Boursorama et Revolut (non vérifiés, à confirmer avec de vrais fichiers).

Formats connus. **Seul le premier a été vérifié sur un vrai export ; les autres en-têtes viennent de ma connaissance générale et doivent être confirmés avec un fichier anonymisé avant d'en faire des profils livrés.**

- **Banque Populaire** (groupe BPCE ; format **vérifié** sur un export réel ; la Caisse d'Epargne utilise probablement le même, non vérifié) : nom de fichier `<numéro de compte>_<JJMMAAAA début>_<JJMMAAAA fin>.csv` (le numéro de compte aide à retrouver le compte). Encodage **ISO-8859-1** (essayer UTF-8 avec BOM, puis repli sur cp1252). Séparateur `;`, première ligne = en-têtes, aucune ligne de métadonnées, ordre des lignes du plus récent au plus ancien. Colonnes, à lire **par leur nom** (jamais par leur position) :
  `Date de comptabilisation;Libelle simplifie;Libelle operation;Reference;Informations complementaires;Type operation;Categorie;Sous categorie;Debit;Credit;Date operation;Date de valeur;Pointage operation`
  - Dates `JJ/MM/AAAA`. `booked_on` = `Date operation` (la date de comptabilisation en diffère souvent) ; garder l'autre en `posted_on`.
  - `Debit` est déjà **négatif** et `Credit` positif, virgule décimale, une seule des deux colonnes est remplie ; montant = `Debit` ou `Credit` converti en centimes sans flottant.
  - `Libelle simplifie` sert d'affichage, `Libelle operation` est conservé tel quel (`label_raw`) ; `Informations complementaires` contient le détail de carte (« CB****1234-… ») ; `Pointage operation` est ignoré (« 0 » presque partout).
  - `Categorie` et `Sous categorie` sont celles de la banque : voir « Table de correspondance » plus bas.
  - Piège : la colonne `Reference` **n'est pas unique** (des virements en série partagent la même référence). Ne pas s'en servir seule comme identifiant.
- **Boursorama** : séparateur `;`, colonnes du type `dateOp;dateVal;label;category;categoryParent;supplierFound;amount;comment;accountNum;accountLabel;accountbalance`, montants à virgule décimale.
- **Revolut** : séparateur `,`, colonnes du type `Type,Product,Started Date,Completed Date,Description,Amount,Fee,Currency,State,Balance` ; ignorer les lignes dont l'état n'est pas `COMPLETED`.

**Anti-doublon** : `dedupe_key = sha1(date | amount_cents | label_norm | reference | n)` (`reference` vide si la banque n'en donne pas) où `n` est le rang d'occurrence de la même clé dans le fichier (deux achats identiques le même jour restent deux opérations). Réimporter le même fichier ne crée rien. Les fichiers qui se chevauchent sont gérés par la contrainte `UNIQUE`.

**Catégorisation** (`categorize.py`), par ordre : catégorie manuelle déjà connue pour le même `label_norm` → règles utilisateur (`rule`) → catégorie fournie par la banque, convertie par une table de correspondance → mots-clés intégrés → « Autres » avec `needs_review = 1`. Une correction manuelle propose de créer une règle.

**Table de correspondance Banque Populaire** (fichier `data/banque_populaire_category_map.csv`, 42 couples catégorie + sous-catégorie, colonnes `bank_category, bank_subcategory, finaryx_category, needs_review`). Il faut partir du **couple** et non de la catégorie seule, car la banque mélange des choses différentes. Règles de la table :
- « Logement - maison » → Logement, sauf la sous-catégorie « Internet et telephonie » → Abonnements et télécoms ; « Video, Musique et jeux » → Abonnements et télécoms.
- « Alimentation » → Alimentation, sauf « Restaurant » et « Restauration rapide » → Sorties et loisirs.
- « Shopping et services » (animaux domestiques compris) → Shopping et animaux ; « Cadeaux et dons » → Sorties et loisirs.
- « Banque et assurances » → Banque et assurances. La sous-catégorie « Banque et assurance - autre » peut regrouper des choses différentes, par exemple un remboursement de prêt et une assurance. Elle est donc **marquée « À vérifier »** et l'application propose, en regroupant par libellé, de créer la règle « le libellé contient PRET → Prêts et crédits ». Mots-clés de départ à prévoir : `PRET`, `ECHEANCE PRET`, `REMBT PRET`.
- « Transaction exclue » / « Virement interne » → **Virement interne** (`exclude_from_stats`), ni dépense ni revenu. Ce sont les virements entre comptes du même foyer.
- « A categoriser - sortie d'argent » (virements émis, retraits, chèques) → Autres, **À vérifier**. « A categoriser - rentree d'argent » → Revenus, **À vérifier**.
- Autres couples : Santé → Santé ; Loisirs et vacances → Sorties et loisirs ; Impots et taxes → Impôts et taxes ; Juridique et administratif et Education et famille → Administratif et éducation ; Transports → Transports ; « Revenus et rentrees d'argent » → Revenus.

**Virements entre comptes du foyer** : une partie des « virements reçus » peut venir de membres du foyer et ne pas être de vrais revenus. Prévoir dans les préférences une liste de « noms du foyer » ; un virement dont le libellé contient l'un de ces noms est proposé comme « Virement interne » (règle créée en un clic). Quand les deux comptes sont importés, rapprocher aussi un débit et un crédit de même montant à ±3 jours.

**Confidentialité des exemples** : ne jamais committer d'export réel. Pour les tests, fabriquer des fichiers anonymisés avec les mêmes en-têtes, le même encodage et les cas ci-dessus (références répétées, virements en série, caractères accentués).

---

## 4. Écrans et comportement

Toutes les pages sont dans `design/`. Chaque page a son fichier ; les mesures ci-dessous sont celles de la maquette.

### Navigation
Barre latérale (260 px) : Vue d'ensemble, Mois en détail, Cumul, Prévisions · séparateur · Comptes, Importer un CSV. Filtre de compte en liste déroulante : « Tous les comptes » ou un compte. Le filtre s'applique à toutes les vues. Bouton de menu principal : catégories et règles, exporter les données, préférences, raccourcis, à propos.

**Adaptatif** : sous 600 sp de large, `AdwBreakpoint` replie la barre latérale et affiche une `AdwViewSwitcherBar` en bas (Aperçu, Mois, Cumul, Prévisions, Comptes). Maquettes : `Etroit.dc.html` (aperçu), `EtroitMois`, `EtroitCumul`, `EtroitPrevisions`, `EtroitComptes`, `EtroitImport`. KPI sur 2 colonnes, une seule colonne de contenu, anneau plus petit avec sa liste dessous, tableaux condensés (catégorie, montant, part). La page d'import étroite n'a pas la barre d'onglets (flux modal avec bouton retour). L'accueil vide, les opérations et le détail d'un compte n'ont pas de variante étroite dessinée : même principe à appliquer.

### Vue d'ensemble (`Main.dc.html`)
4 cartes : solde total, dépenses du mois, revenus du mois, solde prévu fin du mois suivant (avec écart). Deux colonnes : à gauche « Évolution du mois » puis « Cumul sur 6 mois » ; à droite « Où part l'argent » puis « Prévision à 6 mois ». Chaque section a un lien vers sa page détaillée.

### Mois en détail (`Mois.dc.html`)
Boutons mois précédent/suivant. Courbe du solde jour par jour avec le mois précédent en pointillé gris. Sous la courbe, barres des dépenses du jour (étiquette sur le plus gros jour). Anneau par catégorie + liste (catégorie, montant, part, écart vs mois précédent). 6 dernières opérations, lien « Tout voir » (liste complète filtrable : recherche texte, catégorie, compte ; changement de catégorie en ligne).

### Cumul (`Cumul.dc.html`)
Sélecteur 3 / 6 / 12 mois. 4 cartes (total, moyenne mensuelle, mois max, mois min). Colonnes empilées par mois (ordre de catégories fixe, total au-dessus de chaque colonne). Pourcentage de la catégorie dans le mois écrit **dans chaque segment** quand le segment fait au moins 16 px de haut ; le tableau « Part de chaque catégorie » dessous donne tous les pourcentages (6 mois + part sur la période + cumul € + moyenne/mois + ligne Total).

### Prévisions (`Previsions.dc.html`)
Deux sélecteurs : période d'apprentissage (3/6/12 mois) et horizon (3/6/12 mois). Courbe réelle en trait plein, prévue en pointillé, fourchette en aplat translucide, repère « Aujourd'hui ». Annotation du pic attendu. Liste « Mois par mois » (dépenses attendues, revenus, solde fin de mois) et « Charges récurrentes détectées » avec le total mensuel.

### Opérations (`Operations.dc.html`)
Liste complète, atteinte depuis « Tout voir ». Champ de recherche (libellé), liste déroulante de compte, liste déroulante de période, rangée de pastilles de catégories (« Toutes » + 8 catégories, une seule sélectionnée à la fois ; la sélection est entourée de la couleur d'accent). Sous les filtres : nombre d'opérations, catégorie, période, et à droite le total filtré avec sa part des dépenses. Chaque ligne : date, libellé, compte, badge « À vérifier » éventuel, bouton de catégorie (ouvre un menu pour la changer, avec proposition de créer une règle), montant. Pagination « Afficher les N suivantes ».

### Détail d'un compte (`CompteDetail.dc.html`)
Atteint depuis une ligne de la page Comptes (bouton retour vers Comptes). Carte solde + courbe des 6 derniers mois (une valeur par semaine, info-bulle au survol). Groupe « Réglages du compte » (nom, banque, type, solde initial, chacun avec un bouton Modifier). 5 dernières opérations + « Tout voir ». « Zone sensible » : Archiver (retire le compte des totaux) et Supprimer (action destructive, couleur rouge, confirmation obligatoire avec le nombre d'opérations effacées).

### Premier lancement et états vides (`Accueil.dc.html`)
Aucun compte : `AdwStatusPage` « Bienvenue dans Finaryx », bouton suggéré « Importer un CSV », bouton secondaire « Ajouter un compte d'abord », liste « Pour commencer » en 3 étapes. Même page : état « pas assez de données pour une prévision » (moins de 3 mois d'historique), avec action « Importer un export plus ancien ». Prévoir le même principe pour toute vue sans données (mois vide, catégorie vide).

### Comptes (`Comptes.dc.html`)
Carte du total avec barre de répartition par compte (info-bulle par compte). Liste des comptes : avatar à initiales, banque, mini-courbe, solde, nombre d'opérations, état de la dernière importation, chevron vers le détail. « Ajouter » (nom, banque, type, solde initial). Historique des imports (fichier, compte, date, compteurs) ; action « Annuler cet import » = supprimer les `txn` du lot (avec confirmation).

---

### Export des données (`Export.dc.html`)
Page ouverte depuis le menu principal (« Exporter les données… »), fenêtre avec bouton retour. Trois groupes :

1. **Sauvegarde complète** : « Exporter la base de données » copie le fichier SQLite. Utiliser l'API de sauvegarde SQLite (`sqlite3.Connection.backup`), pas une simple copie de fichier, pour obtenir une copie cohérente même si l'application écrit. Sélecteur de fichier `Gtk.FileDialog`, nom proposé `carnet-AAAA-MM-JJ.db`. « Restaurer une sauvegarde… » : vérifier que le fichier est une base Finaryx (`PRAGMA application_id` propre à Finaryx et `PRAGMA user_version` non supérieur à celui de l'application (à définir dans le schéma, avec les migrations)), faire une copie de sécurité de la base actuelle, demander confirmation (`AdwAlertDialog`, action destructive), remplacer, puis recharger les vues.
2. **Export CSV des opérations** : filtres compte (tous ou un compte) et période (tout, mois, 3/6/12 mois, plage libre). Colonnes : `date` (ISO `AAAA-MM-JJ`), `libelle`, `montant`, `categorie`, `compte`, `banque`. Deux formats : « Français » (séparateur `;`, virgule décimale, UTF-8 avec BOM pour Excel) et « Standard » (séparateur `,`, point décimal, UTF-8). Montants convertis depuis les centimes sans passer par un flottant. Le CSV exporté doit pouvoir être réimporté (réimport = 0 nouvelle opération grâce à l'anti-doublon).
3. **Sauvegarde automatique** (interrupteur, activé par défaut) : copie de la base après chaque import, 5 copies conservées dans `~/.local/share/finaryx/sauvegardes` (ou le dossier de données Flatpak).

Retour utilisateur : `AdwToast` « Base exportée » avec action « Ouvrir le dossier » ; message d'erreur clair si le disque est plein ou le dossier non modifiable. L'export n'envoie rien en ligne. La base n'est pas chiffrée : le rappeler en bas de page.
Tests : l'export puis la restauration redonnent une base identique (mêmes agrégats) ; le CSV français s'ouvre avec les bons montants ; export puis réimport = 0 nouvelle opération ; restauration d'un fichier invalide refusée sans toucher à la base.

### Catégories et règles (`Categories.dc.html`)
Page ouverte depuis le menu principal (« Catégories et règles… »). Elle contient :

- **Liste des catégories** (celles d'origine, puis celles créées par l'utilisateur) : pastille de couleur, nom, nombre d'opérations, accès au détail. Bouton suggéré « Nouvelle catégorie ». Une catégorie peut être renommée, recolorée, supprimée ou **fusionnée** dans une autre (les opérations et les règles sont réaffectées, avec confirmation). « Autres » est la catégorie de repli et n'est pas supprimable.
- **Création** (`AdwAlertDialog`) : nom (unique, insensible à la casse, non vide) et couleur de graphique. Choix parmi les 7 teintes `--c1` à `--c7` ; une teinte déjà prise est grisée avec le nom de la catégorie qui l'utilise ; option « Aucune couleur ».
- **Règle de couleur (important)** : les graphiques ne montrent jamais plus de 8 couleurs (7 teintes + « Autres » en gris), conformément à la palette validée. Une catégorie sans couleur est **comptée dans « Autres » dans les graphiques** (camembert, colonnes empilées, cumul) ; son détail apparaît dans l'info-bulle de « Autres », dans une fenêtre « Détail de « Autres » » ouverte par un clic sur la part « Autres » (graphique ou légende), et dans le tableau et les listes, qui la montrent séparément. La couleur suit la catégorie, jamais son rang. Libérer une teinte (suppression ou fusion) ne recolore aucune autre catégorie ; l'utilisateur peut l'attribuer à une catégorie sans couleur. Ne pas générer de teinte supplémentaire. **Le nombre de catégories n'est pas limité** (noms, règles, filtres, listes, tableaux, export) : seule la couleur de graphique l'est.
- **Fenêtre « Détail de « Autres » »** : total regroupé du mois (ou de la période affichée), puis la liste des catégories qu'elle contient, triée par montant décroissant, avec nombre d'opérations, montant et part du total regroupé ; la ligne « Non classé » (opérations « À vérifier » restées dans Autres) est incluse. Boutons « Gérer les couleurs… » (ouvre la page Catégories) et « Fermer ». Clavier : la part « Autres » est un bouton focusable ; la fenêtre est aussi atteignable depuis le tableau.
- **Menu de choix de catégorie** (depuis une ligne de `Operations.dc.html`) : liste de toutes les catégories avec une coche sur l'actuelle, puis « Nouvelle catégorie… » qui ouvre le dialogue de création et applique la catégorie créée à l'opération.
- **« Créer une règle ? »** après un changement de catégorie : texte « Les libellés qui contiennent SUPERMARCHE seront classés en Alimentation », interrupteur « Appliquer aux opérations existantes » (activé ; n'écrase pas les opérations corrigées à la main), boutons « Seulement cette opération » et « Créer la règle ».
- **Liste des règles** : motif, catégorie, nombre d'opérations concernées, suppression (ne modifie pas les opérations déjà classées).

Tests : création, doublon de nom refusé, fusion (totaux inchangés), suppression de « Autres » refusée, catégorie sans couleur regroupée dans « Autres » dans les agrégats de graphique mais séparée dans les tableaux (la somme reste égale au total du mois), règle appliquée aux anciennes opérations sauf celles corrigées à la main.
Hors maquette : les écrans étroits et la variante Material 3 de cette page.

## 5. Prévision (`core/forecast.py`)

Entrée : opérations des `N` derniers mois complets (N = période d'apprentissage), horizon `H` mois. Sortie par mois futur : dépenses attendues, revenus attendus, solde de fin de mois, demi-largeur de la fourchette.

1. **Charges récurrentes** : regrouper par `label_norm` ; une série est récurrente si ≥ 3 occurrences, intervalle médian 28–32 jours (±3), écart de montant < 15 %. Projeter chaque série au jour du mois habituel avec son montant médian. Les afficher dans « Charges récurrentes détectées ».
2. **Dépenses variables** : pour chaque catégorie, moyenne tronquée (on retire le mois le plus haut et le plus bas si N ≥ 6) du montant mensuel **hors récurrentes**.
3. **Saisonnalité** (seulement si ≥ 12 mois d'historique) : facteur par mois du calendrier = dépenses de ce mois / moyenne annuelle, borné à [0,8 ; 1,3]. Sinon facteur 1 et l'interface indique « Saisonnalité non prise en compte : moins de 12 mois d'historique ».
4. **Revenus** : séries récurrentes positives projetées de la même façon ; revenus ponctuels = moyenne des derniers mois, pondérée à 50 %.
5. **Solde** : solde actuel + somme cumulée (revenus attendus − dépenses attendues).
6. **Fourchette** : `σ` = écart-type des erreurs mensuelles de ce même modèle rejoué sur l'historique (backtest simple) ; demi-largeur au mois `h` = `1,28 · σ · √h` (≈ intervalle à 80 %).
7. Afficher clairement que c'est une estimation. Aucune recommandation financière.

Tests unitaires obligatoires : série mensuelle parfaite → prévision exacte ; une dépense exceptionnelle isolée n'est pas projetée ; moins de 3 mois d'historique → message « pas assez de données » plutôt qu'un graphique.

---

## 6. Design

### Principes
Suivre le **GNOME HIG** (https://developer.gnome.org/hig/) et libadwaita : `AdwApplicationWindow`, `AdwHeaderBar` (actions au début, titre au centre, menu principal à la fin), `AdwNavigationSplitView`, `AdwPreferencesPage` / `AdwPreferencesGroup` pour l'import, listes encadrées (`.boxed-list`), cartes (`.card`, rayon 12 px), bouton d'action suggérée (`.suggested-action .pill`), `AdwToastOverlay` pour les confirmations, `AdwStatusPage` pour l'état vide (aucun compte, aucune donnée).

Cibles tactiles ≥ 44 px dans la maquette ; libadwaita en met 34 px par défaut : **utiliser les valeurs libadwaita**, la maquette est plus grande pour la lisibilité. Police : Adwaita Sans (la maquette utilise Inter en remplacement). Français, majuscule seulement en début de phrase.

### Variante Material 3 (optionnelle)
Une seconde série de maquettes `design/M3-*.dc.html` (15 pages, mêmes contenus et mêmes données) suit **Material 3** (https://m3.material.io/). **Le design GNOME reste la référence** pour l'application GTK4/libadwaita native ; la variante M3 n'est à implémenter que si l'utilisateur la demande.

- Feuille de style `design/m3.css` : mêmes noms de variables que la version GNOME, mappés sur les rôles M3 (surface, conteneur de surface, primary, secondary container, primary container, error, outline). Les couleurs de données (catégories c1 à c8, comptes a1 à a4) ne changent pas.
- Composants : rail de navigation 80 px (indicateur en pilule 56x32, FAB « Importer »), barre d'application supérieure 64 px (titre 22 px, sous-titre 12 px), boutons segmentés, puces de filtre (rayon 8 px), barre de navigation inférieure en étroit (indicateur 64x32), boutons tonals. Police Roboto.
- Limites : libadwaita ne fournit aucun de ces composants ; il faudrait des widgets GTK personnalisés (CSS + `Gtk.Box`/`Gtk.Button` stylés) et l'application s'écarterait du HIG GNOME. Pas d'états d'élévation ni d'ondulation dans les maquettes.

### Couleurs
Dans `style.css`, reprendre exactement les variables de `design/theme.css` (clair par défaut, sombre selon `AdwStyleManager`). Les graphiques lisent ces valeurs (ne pas coder de couleur en dur dans le code Cairo ; passer par une table de couleurs construite à partir du thème courant et recalculée à `notify::dark`).

Catégories (identité fixe, ordre fixe) :

| # | Catégorie | Clair | Sombre | Texte sur la couleur (clair / sombre) |
|---|---|---|---|---|
| 1 | Logement | #2a78d6 | #3987e5 | #ffffff / #0b0b0b |
| 2 | Alimentation | #eb6834 | #d95926 | #0b0b0b / #0b0b0b |
| 3 | Transports | #1baf7a | #199e70 | #0b0b0b / #0b0b0b |
| 4 | Prêts et crédits | #eda100 | #c98500 | #0b0b0b / #0b0b0b |
| 5 | Shopping et animaux | #e87ba4 | #d55181 | #0b0b0b / #0b0b0b |
| 6 | Abonnements et télécoms | #008300 | #008300 | #ffffff / #ffffff |
| 7 | Banque et assurances | #6250d6 | #9085e9 | #ffffff / #0b0b0b |
| 8 | Autres | #898781 | #898781 | #0b0b0b / #0b0b0b |

Comptes (parts de la barre de répartition) : `--a1..--a4`. Série principale des courbes : `--s1`. Mois précédent : `--ghost`, pointillé 5/4. Fourchette de prévision : `--band`.

**Validation de la palette** (script de validation, clair et sombre) : séparation pour le daltonisme et écart en vision normale réussis. Deux points à connaître : le gris « Autres » est volontairement neutre (échec attendu du seuil de saturation) ; en mode clair, le vert Transports, le jaune Loisirs et le rose Shopping ont un contraste inférieur à 3:1 avec le fond. Pour cette raison, le nom, le montant et le pourcentage sont toujours écrits (liste, tableau, étiquettes) : ne jamais faire reposer une information sur la couleur seule.

### Règles de graphiques
- Courbes : trait 2 px, extrémités arrondies, une pastille de 12 px au dernier point avec la valeur finale à côté. Grille 1 px très discrète, axe sans cadre.
- **Anneau « Où part l'argent »** (`Main`, `Mois`, `Etroit`) : parts triées par montant décroissant, départ à midi, sens horaire. **Séparations blanches de largeur constante (3 px, couleur de la carte)** : des segments radiaux, pas des écarts angulaires. Trou central (≈ 58 % du diamètre) avec le total et « dépensés ». **Pourcentage sans décimale dans chaque part**, dans la couleur de texte de la catégorie ; **pour une part sous 5 %, le pourcentage sort de l'anneau**, juste à l'extérieur de la part, en texte secondaire. Le détail (couleur, nom, montant, part) est dans la liste à côté, pas de légende séparée ni de traits de repérage. L'anneau est centré dans son bloc ; en largeur étroite, il passe au-dessus de la liste.
- Colonnes empilées : écart de 2 px de la couleur de la carte entre les segments, coins arrondis seulement sur le segment du haut, total au-dessus.
- Mini-courbes (`sparkline`) des comptes : sans axe, 1,5 px.
- **Accessibilité** : chaque graphique expose un libellé accessible résumant son contenu (`Gtk.Accessible`), et chaque graphique a sa version tableau (liste ou tableau déjà présent dans les pages). Pas d'info-bulle seule : les informations clés sont écrites.
- Info-bulles au survol : image de référence `reference/tooltips.png` (4 cas, clair et sombre). La maquette HTML utilise des info-bulles natives (attribut `title`) sur chaque jour des courbes, chaque barre, chaque segment des colonnes et chaque point de prévision. Dans l'application, utiliser `Gtk.Tooltip` ou un popover avec le même contenu (jour + solde + dépenses du jour ; mois + catégorie + montant + part ; mois + solde, « (prévu) » si besoin). Les parts de l'anneau n'ont pas d'info-bulle dans la maquette : à ajouter.

---

## 7. Qualité

- **Confidentialité** : tout est local ; aucune requête réseau ; pas de télémétrie. Base non chiffrée par défaut (le dire dans « À propos » ou les préférences) ; option de chiffrement à étudier plus tard (SQLCipher).
- **Sauvegarde et export** : voir la section « Export des données » (écran `Export.dc.html`).
- **Performance** : 50 000 opérations doivent s'ouvrir en moins d'une seconde ; agrégats par requêtes SQL, pas en Python ; index sur `booked_on`.
- **Erreurs d'import** : jamais d'import partiel silencieux ; message clair par ligne en erreur ; l'import est atomique.
- **Formats** : montants en `fr-FR` (espace insécable fine avant `€`, virgule décimale), dates « 31 oct. » / « octobre 2026 ».
- **Tests** : importeurs (un jeu de fichiers anonymisés par banque), anti-doublon (réimport = 0 nouvelle opération), agrégats (somme des catégories = total du mois), prévision (cas ci-dessus).

## 8. Plan de travail suggéré

1. Squelette Meson/Flatpak, fenêtre libadwaita vide avec navigation et thème.
2. `core/db.py` + migrations + modèles + données de démonstration (reprendre les chiffres de la maquette : 4 comptes, 3 banques, mai–octobre 2026).
3. Page Comptes + ajout de compte.
4. Import CSV (un format, puis les autres) avec aperçu, doublons, annulation.
5. `stats.py` + vue d'ensemble, mois en détail, cumul (graphiques Cairo : courbe, anneau, colonnes).
6. Catégorisation, règles, correction manuelle.
7. `forecast.py` + page Prévisions.
8. Adaptatif (< 600 sp), thème sombre, accessibilité, info-bulles.
9. Packaging Flatpak, icône, metainfo, traductions.

À chaque étape : l'application doit rester lançable et les tests passer.

## 9. Points à décider avec l'utilisateur avant de coder

- Pile technique (Python/GTK4 recommandé ci-dessus, ou Rust).
- Exemples de vrais CSV anonymisés de chaque banque.
- Multi-devises (la maquette suppose l'euro uniquement).
- Virements entre comptes de l'utilisateur : à exclure des dépenses et revenus (détection par montant opposé, même jour, comptes différents) — à prévoir dès le modèle (`transfer_pair_id`).
- Chiffrement de la base.
