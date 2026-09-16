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
- `--near <value>` / `--far <value>`: plans de clipping, en unites astronomiques
- `--star-radius <value>`: rayon de la coquille d'etoiles, en unites astronomiques
- `--display-exposure <value>`: exposition du mappage tonal d'affichage
- `--time-scale <facteur>`: acceleration du temps simule, 1 = temps reel
- `--no-guides`: desactive les lignes de guidage

## Controles interactifs

- souris gauche + drag: orbite camera
- molette ou `Q`/`E`: zoom
- `W`/`A`/`S`/`D` ou fleches: orbite camera
- `Space`: fige ou relance le temps simule
- `G`: affiche/cache les lignes de guidage
- `[` et `]`: diminue/augmente l'acceleration du temps
- `-` et `+`: diminue/augmente l'exposition d'affichage
- `R`: reset camera
- `H`: affiche les controles dans le terminal

## Moteur 3D: modele physique

- profondeur inversee (near -> 1, far -> 0) pour conserver la precision du tampon de
  profondeur de 1e-6 ua a plusieurs centaines d'unites astronomiques
- positions kepleriennes des planetes et planetes mineures a partir des elements J2000,
  satellites resolus dans le plan de reference de leur corps parent
- orientation des corps par les elements rotationnels seculaires de l'IAU/WGCCRE
  (direction du pole alpha0/delta0 et meridien origine W), sans les termes periodiques
- aplatissement geometrique applique selon l'axe de rotation, avec matrice de normales
  correspondante
- eclairement solaire en 1/r^2, reflectance lambertienne ponderee par l'albedo
  geometrique mesure, mappage tonal de Reinhard a l'affichage
- photosphere solaire avec assombrissement centre-bord lineaire (u = 0.6)
- diffusion atmospherique simple avec fonction de phase de Rayleigh et epaisseur
  optique en 1/cos, coquille dimensionnee sur la hauteur d'echelle reelle
- anneaux de Jupiter, Saturne, Uranus et Neptune aux rayons publies, avec modele de
  diffusion simple dans une couche d'epaisseur optique donnee
- les corps sans carte de surface publiee (Soleil, satellites, planetes mineures) sont
  rendus par relief procedural; l'interface le signale explicitement

Le rendu 3D n'est soumis au GPU que lorsque l'espace simulateur est actif.
