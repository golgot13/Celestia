# Catalogues astronomiques libres

Le viewer charge les catalogues stellaires locaux aux formats suivants:

- `stars.dat`: format binaire Celestia avec en-tete `CELSTARS`;
- `hip_main.dat`: catalogue Hipparcos de l'ESA, format texte delimite par `|`;
- CSV ou TSV avec au minimum `ra_deg`, `dec_deg` et `apparent_magnitude`.

## Sources gratuites

- Hipparcos: https://cdsarc.cds.unistra.fr/viz-bin/Cat?I/239
- Gaia Archive: https://gea.esac.esa.int/archive/

Telecharger un export public dans ce dossier, puis lancer:

```powershell
cargo run --release --bin ui3d_app -- sample_campaign.cfg --catalog assets/catalogs/hip_main.dat --catalog-limit 10
```

Le fichier peut aussi etre fourni par la variable d'environnement `CELESTIA_STAR_CATALOG`.
Sans option et sans variable, l'application recherche automatiquement:

1. `assets/catalogs/stars.dat`
2. `assets/catalogs/hip_main.dat`
3. `stars.dat`
4. `hip_main.dat`

Si aucun fichier n'est present, un champ synthetique de secours est affiche et le terminal
indique explicitement `stellar_catalog=synthetic_fallback`. Ce champ ne doit pas etre
utilise pour une analyse scientifique.

## Autres astres

Le systeme solaire est integre dans le coeur Rust via `solar_system_catalogue()`. Il
comprend le Soleil, les planetes, la Lune, plusieurs satellites naturels et les principaux
corps mineurs. Ces donnees orbitales et physiques sont distribuees avec l'application;
aucun abonnement n'est necessaire.
