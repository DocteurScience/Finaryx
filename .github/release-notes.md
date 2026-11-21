Finaryx 0.2.0 : saisie manuelle des opérations. Vos données restent sur votre ordinateur : aucune requête réseau, aucune télémétrie.

- Ajoutez une opération à la main (Ctrl+N, menu principal, bouton « + » de la liste des opérations ou du détail d’un compte) : date, libellé, dépense ou entrée, montant, catégorie proposée d’après le libellé
- Les saisies portent le badge « Saisie manuelle » et se modifient ou se suppriment avec le crayon ; les opérations importées restent inchangées
- Comptes tenus entièrement à la main (espèces, banque sans export) : le solde actuel peut être indiqué dès la création du compte
- À l’import, une saisie retrouvée dans le relevé (même compte, même montant, à 7 jours près) est proposée au remplacement par l’opération de la banque, qui garde la catégorie choisie ; rien n’est remplacé sans votre accord
- La base de données est mise à niveau automatiquement à la première ouverture ; une sauvegarde reste conseillée avant la mise à jour (menu principal › Exporter les données)

Installation : téléchargez l’un des deux fichiers ci-dessous (ajoutés automatiquement quelques minutes après la publication).

- Flatpak : `flatpak install --user finaryx.flatpak`
- Snap : `sudo snap install --dangerous finaryx.snap` (`--dangerous` car le fichier ne vient pas du Snap Store et n’est donc pas signé). Le snap n’a pas accès au réseau.
