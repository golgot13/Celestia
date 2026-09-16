# Document de reprise

Date: 2026-09-16
Projet: application d'exploration astronomique Rust
Racine du projet: `F:\Celestia\nextgen\rust_app`

## 1. Perimetre

Le produit a ete recentre sur l'application Rust `observatory_core` et son interface
`ui3d_app` basee sur `wgpu`, `winit` et `egui`.

L'ancien Celestia C/C++ et son build CMake ont ete supprimes du perimetre de travail.
Le depot ne doit plus etre reconstruit avec CMake. Le point d'entree de build est:

```powershell
cargo test --release --manifest-path nextgen/rust_app/Cargo.toml
```

Attention: la suppression historique est actuellement visible comme suppressions Git.
Ne pas restaurer les sources C/C++ sans decision explicite.

## 2. Etat valide

Fonctionnalites presentes:

- coeur scientifique Rust modulaire;
- calculs orbitaux, ephemerides et positions planetaires;
- astrometrie, photometrie, calibration, PSF et controle qualite;
- acquisition, guidage, securite et orchestration de campagnes;
- interface desktop 3D `ui3d_app`;
- rendu GPU `wgpu` avec scene solaire, atmospheres, anneaux et guides;
- chargement de catalogues stellaires `stars.dat`, Hipparcos et CSV/TSV;
- index spatial par bandes de declinaison et recherche par cone;
- acceleration AVX2 en Rust pour l'interpolation d'ephemerides.

Validation executee:

```text
cargo test --release --manifest-path nextgen/rust_app/Cargo.toml
162 tests de bibliotheque et tests de binaires valides
cargo test --release --manifest-path nextgen/rust_app/Cargo.toml --bin ui3d_app
23 tests UI/WGPU valides
```

La mesure locale de l'interpolation a donne un exemple de 3.52 ms scalaire contre
2.89 ms AVX2 sur 1 000 000 echantillons. Le ratio de performance reste une mesure,
pas un critere de test deterministe.

## 3. Lancement

Depuis `F:\Celestia`:

```powershell
cargo run --release --manifest-path nextgen/rust_app/Cargo.toml --bin ui3d_app -- `
  nextgen/rust_app/sample_campaign.cfg
```

Depuis `F:\Celestia\nextgen\rust_app`:

```powershell
cargo run --release --bin ui3d_app -- sample_campaign.cfg
```

Les textures sont resolues depuis le repertoire du crate Rust. Le dossier courant
n'a donc plus d'importance pour le chargement des textures planetaires.

## 4. Cartographie stellaire

Pour utiliser un catalogue reel:

```powershell
cargo run --release --manifest-path nextgen/rust_app/Cargo.toml --bin ui3d_app -- `
  nextgen/rust_app/sample_campaign.cfg `
  --catalog "D:\catalogues\gaia.csv" `
  --catalog-limit 12
```

Le fichier doit exister. Le chemin `F:\chemin\catalogue.csv` est uniquement un
exemple et ne doit pas etre copie tel quel.

Le catalogue CSV/TSV doit fournir au minimum:

```text
ra_deg,dec_deg,apparent_magnitude
```

Colonnes optionnelles reconnues:

- identifiant: `id`, `hip`, `source_id`, `catalog_number`;
- distance/parallaxe: `distance_ly`, `dist_ly`, `parallax`, `plx`, `parallax_mas`;
- couleur: `b_v`, `bv`, `color_index`, `bp_rp`;
- type spectral: `spectral_type`, `sptype`, `spec_type`.

Aucun catalogue Gaia ou Hipparcos complet n'est fourni dans le depot. Ne pas utiliser
`out/exoplanet_lightcurve.csv`: c'est une courbe de lumiere, pas un catalogue stellaire.

Le mode sans `--catalog` utilise encore un champ stellaire synthetique de secours. Pour
une cartographie scientifique, le catalogue reel doit etre obligatoire dans la prochaine
iteration.

## 5. Architecture cible

Priorites:

1. Rust pour toute nouvelle fonctionnalite;
2. aucune dependance native Celestia historique;
3. `wgpu` pour le rendu multi-backend;
4. API Rust stable pour les kernels numeriques;
5. ASM64/AVX2 uniquement pour les chemins critiques mesures;
6. tests numeriques et tests de non-regression avant optimisation.

Etat ASM64:

- le chemin AVX2 actuel est implemente avec `std::arch` Rust;
- aucun kernel assembleur ASM64 autonome n'est encore integre;
- avant d'ajouter de l'assembleur, mesurer le gain du kernel Rust et figer une API;
- l'assembleur devra rester optionnel avec un fallback Rust exact.

## 6. Prochaines etapes recommandees

### Priorite 1: catalogue reel

- ajouter une option de chemin catalogue persistante dans la configuration;
- supprimer le champ stellaire synthetique pour les executions scientifiques;
- ajouter une validation de schema et un rapport de nombre d'etoiles chargees;
- ajouter une strategie de pagination/streaming pour les catalogues tres volumineux;
- ajouter un index 3D ou HEALPix-equivalent sans introduire de crate si possible.

### Priorite 2: rendu stellaire

- conserver la precision RA/DEC en `f64` jusqu'a la conversion GPU;
- ajouter la magnitude limite dynamique et le culling par champ de vision;
- afficher la couleur derivee du catalogue plutot qu'un blanc uniforme;
- valider la projection aux poles et le passage RA 0/360;
- tester une scene avec un vrai catalogue avant toute optimisation GPU.

### Priorite 3: ASM64

- definir une fonction pure de reference en Rust;
- definir l'ABI et les alignements des buffers;
- ajouter le kernel ASM64/AVX2 sur Windows x86_64;
- comparer sortie scalaire, AVX2 Rust et ASM64 sur des donnees deterministes;
- ne garder l'ASM64 que si le gain est reproductible et documente.

### Priorite 4: qualite produit

- ajouter un mode CI `cargo fmt --check`, `cargo clippy --all-targets` et `cargo test`;
- ajouter un test de lancement WGPU avec adaptateur logiciel si disponible;
- ajouter des logs structures pour chargement catalogue, GPU et ressources;
- produire un paquet Release avec assets et exemple de catalogue minimal.

## 7. Commandes de diagnostic

```powershell
cargo test --release --manifest-path nextgen/rust_app/Cargo.toml
cargo test --release --manifest-path nextgen/rust_app/Cargo.toml --bin ui3d_app
cargo run --release --manifest-path nextgen/rust_app/Cargo.toml --bin bench_ephemeris
cargo run --release --manifest-path nextgen/rust_app/Cargo.toml --bin ui3d_app -- --help
```

Verifier un catalogue avant lancement:

```powershell
Test-Path "D:\catalogues\gaia.csv"
Get-Content "D:\catalogues\gaia.csv" -TotalCount 3
```

## 8. Regles de reprise

- travailler dans `nextgen/rust_app`;
- ne pas relancer CMake;
- ne pas restaurer l'ancien Celestia automatiquement;
- ne pas ajouter de donnees stellaires fictives pour masquer un catalogue absent;
- ne pas annoncer une cartographie complete sans fichier catalogue reel et test de rendu;
- conserver un fallback Rust lorsque l'ASM64 n'est pas disponible;
- executer les tests cibles apres chaque modification du renderer ou du catalogue.
