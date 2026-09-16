# UI 3D App (egui + wgpu)

Date: 2026-09-16

## Objectif

Ce binaire est l'application desktop complete: page d'accueil, menu lateral rabattable
et trois espaces de travail bases sur le coeur Rust existant.

Le flux est:
1. lecture d'un fichier de campagne,
2. validation runtime avec le moteur applicatif existant,
3. creation d'une scene 3D basee sur les cibles RA/DEC,
4. rendu en temps reel avec controle camera + interface egui.

## Espaces de travail

- **Accueil**: presentation du logiciel, capacites de chaque volet et etat de la session.
- **Cartographie & pilotage**: carte azimutale temps reel du ciel local (cibles de
  campagne, planetes, satellites et asteroides), temps sideral local, angle horaire,
  altitude, azimut et masse d'air; consigne de monture corrigee du modele de pointage
  et de la refraction atmospherique; planification de la file d'observation par merite
  et commande d'acquisition.
- **Exploitation scientifique**: indexation d'un repertoire de cliches FITS (suivi
  direct possible), lecture FITS native, calibration offset/noir/plage plate, detection
  multi-sources robuste, photometrie d'ouverture, metrologie PSF (FWHM, rondeur),
  controle qualite et empilement (moyenne, mediane, rejet sigma).
- **Simulateur spatial**: moteur 3D temps reel du systeme solaire, rotations propres,
  enveloppes internes et atmospheres animees, visee de la camera depuis la Terre vers
  la cible active de l'espace d'observation.

Le menu lateral gauche se rabat via le bouton fleche de la barre superieure.

## Lancement

Depuis le dossier `nextgen/rust_app`:

```powershell
$env:CARGO_TARGET_DIR='C:\rust-target\celestia'
cargo run --release --bin ui3d_app -- sample_campaign.cfg --width 1600 --height 900 --output-dir out
```

Afficher l'aide:

```powershell
cargo run --quiet --bin ui3d_app -- --help
```

Le repertoire passe a `--output-dir` sert de repertoire de reception des cliches pour
l'espace d'exploitation scientifique; il peut etre change a chaud dans l'interface.

## Options principales

- `--filter <name>`: filtre photometrique pour la validation runtime
- `--exposure <seconds>`: temps de pose
- `--repeats <count>`: repetitions par cible
- `--output-dir <path>`: dossier de sortie runtime
- `--width <px>` / `--height <px>`: taille de fenetre
- `--fov <deg>`: champ de vision camera
- `--near <value>` / `--far <value>`: plans de clipping
- `--planet-radius <value>`: rayon de planete
- `--star-radius <value>`: rayon de la coquille d'etoiles
- `--rotation-speed <deg/s>`: vitesse de rotation de planete
- `--atmosphere <value>`: intensite du halo atmosphere
- `--no-guides`: desactive les lignes de guidage

## Controles interactifs

- souris gauche + drag: orbite camera
- molette ou `Q`/`E`: zoom
- `W`/`A`/`S`/`D` ou fleches: orbite camera
- `Space`: pause/reprise animation
- `G`: affiche/cache les lignes de guidage
- `[` et `]`: diminue/augmente la vitesse de rotation
- `-` et `+`: diminue/augmente l'atmosphere
- `R`: reset camera
- `H`: affiche les controles dans le terminal

## Rendu immersif implemente

- sphere planetaire haute resolution
- eclairage dynamique (diffus + speculaire)
- halo atmosphere (rim lighting)
- champ d'etoiles issu des cibles de campagne
- scintillation douce des etoiles
- panneau egui temps reel pour controle direct

Les raccourcis camera s'appliquent a l'espace simulateur; le rendu 3D n'est soumis au
GPU que lorsque cet espace est actif.
