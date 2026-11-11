# Premier message à donner à Claude Code

Copiez le texte ci-dessous dans une session Claude Code lancée à la racine du dossier du projet (celui qui contient `FINARYX-SPEC.md`, `CLAUDE.md`, `design/` et `reference/`).

---

Lis `CLAUDE.md` puis `FINARYX-SPEC.md` en entier, et regarde `design/` et `reference/tooltips.png`.

Avant d'écrire du code, pose-moi en une seule fois les questions de la section 9 du cahier des charges (pile technique, multi-devises, virements entre mes comptes, chiffrement) et demande-moi des exemples de CSV anonymisés de mes banques.

Ensuite, travaille dans l'ordre du plan de la section 8, une étape à la fois :
1. squelette Meson/Flatpak et fenêtre libadwaita avec navigation et thème clair/sombre ;
2. base SQLite, migrations, modèles et données de démonstration reprenant les chiffres de la maquette.

Après chaque étape : l'application se lance, les tests passent, et tu me résumes en quelques lignes ce qui a été fait et ce qui vient ensuite. Ne passe pas à l'étape suivante sans mon accord.

Pour tout ce qui est visuel, la maquette fait foi pour la structure, les textes et les mesures ; le GNOME HIG fait foi pour les composants. En cas de conflit, dis-le-moi plutôt que de choisir seul.
