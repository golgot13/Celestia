# UI 3D App (egui + wgpu)

Date: 2026-09-16

## Objectif

Ce binaire ajoute une interface desktop en egui et un rendu 3D en wgpu au coeur Rust existant.

Le flux est:
1. lecture d'un fichier de campagne,
2. validation runtime avec le moteur applicatif existant,
3. creation d'une scene 3D basee sur les cibles RA/DEC,
4. rendu en temps reel avec controle camera + panneau egui.

## Lancement

Depuis le dossier `nextgen/rust_app`:

```powershell
$env:CARGO_TARGET_DIR='C:\rust-target\celestia'
cargo run --release --bin ui3d_app -- sample_campaign.cfg --width 1600 --height 900
```

Afficher l'aide:

```powershell
cargo run --quiet --bin ui3d_app -- --help
```

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
