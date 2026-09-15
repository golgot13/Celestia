# Plan de developpement honnete et exhaustif

## 1) Contrainte non negociable

Ce plan applique strictement les contraintes suivantes:
- aucune crate Rust externe,
- suppression totale de Julia,
- coeur de calcul en ASM64 + SIMD AVX2,
- aucune pratique mock, stub, hardcode operationnel, dead code,
- cible CPU x86_64 AVX2 en production.

Consequence directe:
- pas de wgpu (dependance crate),
- rendu GPU via API native (Vulkan, Direct3D, OpenGL) en FFI manuelle,
- charge d'ingenierie plus elevee sur fondations, tooling et qualite.

## 2) Evaluation honnete de l'effort

### 2.1 Charge globale
- Equipe recommandee: 5 a 8 ingenieurs a temps plein.
- Duree realiste: 18 a 30 mois pour une version pro robuste.
- Sous 4 ingenieurs, le risque delai/qualite devient tres eleve.

### 2.2 Pourquoi c'est long
- Zero crate externe impose d'ecrire et maintenir plus de briques maison.
- ASM64 + AVX2 exige double implementation (baseline scalaire + vectorisee).
- Le niveau metrologique impose validation scientifique continue sur donnees reelles.

## 3) Definition stricte de "sans mock, sans stub, sans hardcode, sans dead code"

### 3.1 Interdits
- Interdit: classes/mock frameworks de substitution comportementale.
- Interdit: stubs de fonctions "a faire" en production.
- Interdit: valeurs operationnelles figees en dur (paths, gains, offsets, seuils instrumentaux).
- Interdit: code non appelle, branches mortes, features inachevees compilees par defaut.

### 3.2 Autorise
- Autorise: constantes physiques normatives avec source scientifique citee.
- Autorise: jeux de donnees de reference reelles ou corpus mesures archives.
- Autorise: mode debug scalaire, mais jamais comme substitut de fonctionnalite manquante.

### 3.3 Garde-fous CI/CD
- Echec CI si presence de patterns interdits (mock, stub, FIXME de contournement).
- Echec CI si options de build activent des chemins incomplets.
- Echec CI si regression precision/performance hors budget.
- Echec CI si symboles ASM exportes sans test d'integration associe.

## 4) Cible produit

Transformer l'application en suite professionnelle pour:
- observation telescope pilotee,
- mesures scientifiques reproductibles,
- simulation astro de haute precision,
- exploitation en observatoire sur sessions longues.

Capacites minimales:
- acquisition instrumentale fiable,
- calibration photometrique/astrometrique,
- pipeline de simulation predictive valide,
- tracabilite de bout en bout (config, donnees, resultats, version).

## 5) Architecture cible executable

### 5.1 Stack technique
- Runtime: Rust standard library uniquement.
- UI: couche native OS (Win32/Qt C ABI via FFI), sans crate UI.
- Rendu: backend GPU natif via FFI.
- Calcul: noyaux ASM64 AVX2 + baseline scalaire Rust.
- Data: FITS/WCS, tables calibration, journaux d'observation.

### 5.2 Sous-systemes
- app-shell: cycle de vie, configuration, session, erreurs.
- render-native: device, swapchain, pipelines, overlays scientifiques.
- observatory-io: monture, camera, focuser, roue a filtres.
- acquisition: sequenceur, metadata complete, reprise sur incident.
- calibration: dark, bias, flat, bruit lecture, gain capteur.
- astrometry-photometry: solve, centroid, extraction, qualite.
- astro-core-asm: ephemerides et transformations de reperes.
- scientific-validation: non-regression numerique et metrologique.

### 5.3 Contrat Rust <-> ASM64
- ABI C stable et versionnee.
- Alignement memoire 32 octets minimum pour AVX2.
- API orientee batch pour amortir le cout des appels.
- Codes retour explicites, journalisation technique obligatoire.

## 6) Exigences scientifiques contractuelles

### 6.1 Temps et reperes
- Echelles: UTC, TAI, TT, TDB, UT1.
- Leap seconds versionnes et historises.
- EOP integres et versionnes.
- Budget d'erreur explicite par transformation.

### 6.2 Astrometrie
- Lecture/production FITS + WCS.
- Plate solving robuste en conditions reelles.
- Centroid sub-pixel et estimation d'incertitude.
- Validation sur champs etoiles connus.

### 6.3 Photometrie
- Pipeline dark/bias/flat complet.
- Estimation bruit lecture, gain, linearite capteur.
- Zero points par filtre et suivi derive instrumentale.
- KPI qualite: SNR, FWHM, airmass, residus.

### 6.4 Simulation
- Verification contre references JPL/IAU/SPICE.
- Tolerances numeriques par domaine (position, vitesse, temps).
- Test longue duree sur fenetres temporelles etendues.

## 7) Plan de developpement par phases

### Phase A (Semaines 1-6) - Fondations et gouvernance qualite
Objectif:
- rendre executable la contrainte zero crate/zero mock.

Livrables:
- charte qualite projet (interdits et criteres de rejet),
- spec ABI Rust/ASM64,
- baseline scalaire des kernels prioritaires,
- harnais bench + precision reproductible,
- backend rendu natif minimal (fenetre + boucle + frame vide).

Criteres de sortie:
- build propre multi-config,
- premier pipeline CI avec blocage automatique des pratiques interdites,
- rapport baseline precision + performance signe.

### Phase B (Semaines 7-16) - Noyau observation reel
Objectif:
- acquisition instrumentale operationnelle sur materiel reel.

Livrables:
- pilotage monture et camera,
- sequenceur acquisition,
- enregistrement metadata exhaustive,
- gestion erreurs/reprise,
- tests d'endurance session 8h.

Criteres de sortie:
- 3 nuits de tests terrain sans perte de donnees,
- aucun contournement hardcode en configuration operationnelle.

### Phase C (Semaines 17-28) - Astrometrie et photometrie pro
Objectif:
- rendre les mesures exploitables scientifiquement.

Livrables:
- pipeline calibration dark/bias/flat,
- solve astrometrique robuste,
- mesure centroid et extraction photometrique,
- tableaux de bord qualite metrologique.

Criteres de sortie:
- residu astrometrique median sous seuil contractuel,
- dispersion photometrique conforme sur etoiles de reference.

### Phase D (Semaines 29-44) - Migration kernels AVX2
Objectif:
- accelerer sans degrader la precision.

Livrables:
- kernels AVX2 pour interpolation ephemerides,
- kernels AVX2 pour transformations massives de reperes,
- fallback scalaire de verification pour chaque kernel,
- tests de non-regression numerique automatises.

Criteres de sortie:
- gain de performance confirme par benchs reelles,
- aucune derive precision hors tolerance specifiee.

### Phase E (Semaines 45-60) - Industrialisation et fiabilite
Objectif:
- passer de "fonctionne" a "exploitable en observatoire".

Livrables:
- observabilite complete (logs, metriques, traces),
- reprise apres panne,
- procedures operationnelles,
- paquetage deploiement et doc d'exploitation.

Criteres de sortie:
- disponibilite > 99% sur sessions longues,
- audit qualite interne valide,
- aucune dette critique ouverte.

## 8) Backlog priorise des 12 premiers sprints

Sprint 1:
- spec architecture, spec qualite, spec ABI.

Sprint 2:
- detection CPU AVX2, garde de demarrage, conventions memoire.

Sprint 3:
- baseline scalaire kernel ephemerides + tests precision.

Sprint 4:
- backend rendu natif minimal + frame timing stable.

Sprint 5:
- sequenceur acquisition v1 + metadata v1.

Sprint 6:
- I/O FITS/WCS v1 + validation format.

Sprint 7:
- pipeline calibration dark/bias/flat v1.

Sprint 8:
- solve astrometrique v1 + KPI residus.

Sprint 9:
- kernel AVX2 ephemerides v1 + comparaison scalaire.

Sprint 10:
- kernel AVX2 transformations de reperes v1.

Sprint 11:
- photometrie v1 + suivi derive instrumentale.

Sprint 12:
- hardening end-to-end + endurance 8h + gel de version alpha.

## 9) KPI contractuels

- precision pointage: RMS < 10 arcsec.
- residu astrometrique median: < 0.5 pixel.
- photometrie relative: dispersion < 1% sur references.
- disponibilite session: > 99% sur 8h.
- reproductibilite run-to-run: derive sous seuil par module.
- gain AVX2: > 20% sur kernels cibles, precision conservee.

## 10) Registre des risques et plans de mitigation

Risque: cout eleve du zero crate externe.
Mitigation:
- limiter le scope initial,
- prioriser verticalement les fonctions metier critiques.

Risque: complexite ASM64 multi-kernel.
Mitigation:
- baseline scalaire obligatoire,
- revues assembleur formelles,
- tests limites NaN/Inf/denormals.

Risque: divergence numerique scalaire vs vectorise.
Mitigation:
- comparateurs deterministes,
- corpus de reference versionne,
- seuils d'alerte CI.

Risque: dependance materielle pour tests sans mock.
Mitigation:
- banc de test permanent,
- campagnes terrain planifiees,
- relecture automatisable sur captures reelles archivees.

## 11) Definition of Done (DoD) par fonctionnalite

Une fonctionnalite n'est acceptee que si:
- code utilise en production (aucun dead code),
- aucun mock/stub introduit,
- aucune valeur operationnelle hardcodee,
- tests unitaires + integration + regression scientifique passent,
- metriques precision/performance conformes,
- documentation technique et operatoire complete,
- revue croisee signee (runtime, numerique, exploitation).

## 12) Decision gate projet

Go continu:
- si KPI monte, precision stable, dette critique nulle.

No-go ou scope cut:
- si la precision n'est pas atteinte apres 2 iterations majeures,
- si la complexite ASM compromet la maintenabilite,
- si le zero crate externe bloque un besoin reglementaire/metier critique.

Ce gate est volontairement strict pour garder un plan honnete, realiste et soutenable.
