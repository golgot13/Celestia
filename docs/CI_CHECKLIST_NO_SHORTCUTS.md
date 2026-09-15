# Checklist CI anti raccourcis

## 1. But

Rendre verifiable en continu la politique:
- sans mock,
- sans stub,
- sans hardcode operationnel,
- sans dead code.

## 2. Jobs obligatoires

### 2.1 Gate patterns interdits
- scanner les fichiers source cibles,
- echouer sur motifs interdits,
- afficher fichier, ligne, regle violee.

### 2.2 Gate precision numerique
- executer corpus de reference,
- comparer sorties scalaire versus AVX2,
- echouer si tolerance depassee.

### 2.3 Gate performance
- benchmark kernels cibles,
- comparer a baseline versionnee,
- echouer sur regression superieure au seuil autorise.

### 2.4 Gate ABI
- verifier tailles, alignements, offsets,
- verifier version ABI,
- verifier self-test noyaux.

## 3. Rules de blocage

Le pipeline doit bloquer si:
- pattern interdit detecte,
- tests precision en echec,
- tests ABI en echec,
- regression perf non justifiee,
- artefacts de validation absents.

## 4. Evidence minimale par PR

Chaque PR doit fournir:
- resultat gate patterns,
- rapport precision,
- rapport perf,
- version corpus reference utilisee,
- note de risque residuel.

## 5. Politique exceptions

Aucune exception automatique.
Toute exception doit etre:
- datee,
- motivee,
- limitee dans le temps,
- accompagnee d'un ticket de retrait.

## 6. Scope initial recommande

Au demarrage, appliquer les gates sur:
- nextgen/**
- rust_app/**
- asm/**
- src-rs/**

Puis elargir le scope par vagues de migration.
