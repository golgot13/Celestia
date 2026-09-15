# Charte qualite Sprint 1

## 1. Objet

Ce document fixe les regles de developpement obligatoires pour la nouvelle application pro d'observation et de simulation scientifique.

Les objectifs sont:
- fiabilite operationnelle,
- precision scientifique,
- maintenabilite long terme,
- auditabilite complete.

## 2. Portee

Cette charte s'applique a tout nouveau code de la future pile:
- runtime Rust standard library,
- modules ASM64 et kernels AVX2,
- interfaces instrumentales,
- pipeline calibration et mesures.

Le code historique non migre n'est pas retro-valide par cette charte tant qu'il n'est pas modifie dans le cadre de la migration.

## 3. Regles non negociables

### 3.1 Zero mock
- Interdit de simuler des composants metier par des doubles de test de type mock pour valider le comportement final.
- Les validations de comportement metier se font sur donnees reelles ou corpus de reference versionnes.

### 3.2 Zero stub
- Interdit de merger une fonction vide, partielle ou placeholder dans le flux principal.
- Toute fonction exposee doit etre complete, testee et documentee.

### 3.3 Zero hardcode operationnel
- Interdit d'encoder en dur les chemins, offsets instrumentaux, seuils metrologiques operationnels, credentials, endpoints, ou regles de calibration.
- Toute valeur operationnelle doit provenir d'une configuration versionnee et tracable.

### 3.4 Zero dead code
- Interdit de conserver du code non appelle, des branches inaccessibles ou des options compilees mais inutilisables.
- Le retrait de code obsolete fait partie de chaque lot de migration.

## 4. Exceptions autorisees

Les exceptions ci-dessous sont autorisees sous conditions strictes:
- constantes physiques normatives,
- constantes de protocole binaire officielles,
- bornes mathematiques de securite numerique.

Conditions obligatoires:
- reference explicite a la source (norme, papier, spec),
- commentaire technique court,
- test de non-regression lie a la constante.

## 5. Definition of Done (DoD)

Une tache est terminee uniquement si tous les points sont vrais:
- implementation complete sans mock/stub,
- aucune valeur operationnelle hardcodee,
- aucun dead code introduit,
- tests unitaires, integration et regression scientifique au vert,
- metriques precision/performance conformes au budget,
- logs, erreurs et telemetrie exploitables,
- documentation technique et operatoire mise a jour.

## 6. Qualite et controle

### 6.1 Controle automatique
- gate CI anti mock/stub/hardcode/dead-code,
- gate precision numerique sur corpus de reference,
- gate performance kernels AVX2 versus baseline scalaire,
- gate style ABI Rust/ASM64.

### 6.2 Controle humain
- revue croisee runtime,
- revue croisee numerique,
- revue croisee exploitation observatoire.

## 7. Politique de rejet

Un changement est rejete si au moins un point est vrai:
- detection d'un pattern interdit,
- absence de preuve de precision,
- regression performance non justifiee,
- absence de traceabilite de configuration.

## 8. Traçabilite

Pour chaque merge:
- lier commit vers exigences techniques,
- lier tests executes et resultats,
- lier version des donnees de reference,
- lier metriques comparees avant/apres.
