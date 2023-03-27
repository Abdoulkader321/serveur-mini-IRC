
# Projet de cours

L'objectif du projet sera de compléter les fonctionnalités du serveur mini-IRC par des aspects de sécurité. Dans l'ordre de priorité, nous vous proposons d'implémenter les quatre aspects suivants:

 * Fonctionnalités de base du serveur mini-IRC
 * Confidentialité des échanges entre client et serveur
 * Fonctionnalités additionnelles du serveur mini-IRC: messages directs, listes d'utilisateurs par canal IRC, réservation de pseudonymes
 * Sécurité bout-en-bout entre clients

Nous fournirons à cet égard un client mini-IRC fonctionnel qu'il faudra éventuellement améliorer pour intégrer les fonctionnalités de sécurité. Vous pouvez également modifier la crate `mini-irc-protocol` afin d'intégrer directement ces fonctionnalités dans cette crate.

## Fonctionnalités de base

Les fonctionnalités attendues sont:

 * Connexion d'un utilisateur au serveur mini-IRC
 * Souscription à un canal
 * Envoi et réception de messages vers et depuis un canal
 * Départ propre y compris en cas de déconnexion brutale (le serveur doit continuer à fonctionner normalement en cas de départ d'un utilisateur, et le nom d'utilisateur doit alors redevenir disponible)

## Sécurité entre client et serveur

Vous aurez remarqué que les messages sont transmis en clair entre le client et le serveur. Implémentez une surcouche de sécurité pour obtenir la confidentialité des échanges entre les clients et le serveur, et faites-en une analyse de sécurité. Sous quel(s) modèle(s) d'attaquant votre solution est-elle sûre (i.e. préserve la confidentialité des échanges) ? Dans quel(s) modèle(s) d'attaquant la confidentialité est-elle violée ? Quid de l'authenticité et de l'intégrité des échanges ?

## Fonctionnalités supplémentatires

Ici, le but est d'ajouter des fonctionnalités qui ne sont pas fondamentales, mais permettent au protocol de se rapprocher des fonctionnalités du vrai protocol IRC.

* Messages directs: permet d'envoyer des messages à un utilisateur directement et non seulement à un canal. Que se passe-t-il si l'utilisateur n'est pas présent ?
* Liste d'utilisateurs par canal: permet au client d'afficher qui est connecté sur quel canal
* Réservation de pseudonymes: à la connexion, demandez un mot de passe à l'utilisateur. Si c'est la première fois que quelqu'un se connecte avec ce pseudonyme, cela devient le mot de passe pour ce pseudo. Sinon, l'utilisateur peut se connecter avec ce psudonyme si le mot de passe correspond, et il est rejeté sinon.

## Sécurité bout-en-bout
Si vous arrivez jusqu'ici: bravo ! L'idée est maintenant d'améliorer la sécurité et de chiffrer les communications de bout en bout : c'est aux clients de chiffrer **et** de déchiffrer les communications, non au serveur. Si vous vous sentez sûrs de vous, vous pouvez directement implémenter ce modèle de sécurité sans passer par un chiffrement uniquement entre client et serveur. Ensuite, mêmes questions: sous quelles hypothèses votre protocole est-il sûr ?


Afin d'implémenter votre projet, vous pouvez formez des binômes. Vous pouvez également utilisez les crates de votre choix, excepté des crates implémentant directement le protocole TLS pour la sécurité. En particulier, toutes les crates cryptographiques "bas-niveau" sont bien sûr autorisées. En cas de doute, n'hésitez pas à nous demander !
