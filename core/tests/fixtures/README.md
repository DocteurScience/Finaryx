# Fichiers de test inventés

Ces fichiers sont **entièrement inventés** : mêmes en-têtes, encodage et séparateurs que les formats réels, mais aucun montant, libellé, référence ou numéro issu d'un vrai relevé. Ne jamais ajouter ici un export bancaire réel, même partiel.

- `bp_octobre.csv`, `bp_novembre.csv` : format Banque Populaire (ISO-8859-1, `;`, virgule décimale), avec références répétées (virements en série), deux achats identiques le même jour, accents, dates de comptabilisation différentes de la date d'opération, une catégorie de la banque absente de la table de correspondance. Le second recouvre en partie le premier.
- `generic_us.csv` : format inconnu avec lignes de métadonnées avant l'en-tête, séparateur `,`, point décimal, dates MM/JJ/AAAA et colonne de solde.
