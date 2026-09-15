# Specification ABI Rust vers ASM64 AVX2

## 1. Objet

Definir un contrat binaire stable entre:
- couche runtime Rust (std uniquement),
- noyaux de calcul ASM64 AVX2.

Ce document est normatif pour toute implementation de kernel scientifique.

## 2. Cibles plateformes

- Windows x86_64: convention Microsoft x64.
- Linux x86_64: convention System V AMD64.

Chaque kernel est fourni via deux wrappers assembleur:
- wrapper win64,
- wrapper sysv64.

Le coeur algorithmique peut etre partage via includes assembleur, mais les wrappers ABI sont distincts.

## 3. Versionnement ABI

- ABI major: rupture de compatibilite binaire.
- ABI minor: extension retro-compatible.
- ABI patch: correction sans changement de layout.

Le runtime Rust doit verifier la version au demarrage.

## 4. Types binaires

Types obligatoires:
- u8, u16, u32, u64
- i8, i16, i32, i64
- f32, f64

Regles:
- Aucun type dependant de la taille de plateforme.
- Aucun bool dans une struct ABI (remplacer par u8).
- Aucun pointeur nullable implicite sans champ de validite explicite.

## 5. Alignement et memoire

- Toutes les structures d'entree/sortie kernels AVX2 sont alignees a 32 octets.
- Les buffers passes aux kernels AVX2 doivent etre alignes a 32 octets.
- La longueur de batch est explicite, jamais deduite.
- Pas d'allocation interne dans les kernels ASM64.

## 6. Contrat erreurs

Codes retour standards:
- 0: succes
- 1: argument invalide
- 2: pointeur nul
- 3: alignement invalide
- 4: taille invalide
- 5: CPU non compatible AVX2
- 6: plage numerique invalide
- 7: erreur interne kernel

Chaque appel expose:
- code retour,
- compteur d'items traites,
- index du premier echec si applicable.

## 7. Handshake runtime

Le module ASM64 doit exporter:
- get_abi_version_major
- get_abi_version_minor
- get_required_cpu_features
- self_test_kernel_set

Le runtime Rust:
- valide CPUID,
- valide version ABI,
- execute self-test minimal,
- refuse le mode production si un test echoue.

## 8. Prototypes C de reference

```c
#ifndef ASTRO_ASM_ABI_H
#define ASTRO_ASM_ABI_H

#include <stdint.h>

#define ASTRO_ABI_VERSION_MAJOR 1u
#define ASTRO_ABI_VERSION_MINOR 0u

struct astro_batch_header {
    uint32_t element_count;
    uint32_t reserved;
    uint64_t timestamp_tag;
};

struct astro_status {
    int32_t  code;
    uint32_t processed;
    uint32_t first_error_index;
};

int32_t get_abi_version_major(void);
int32_t get_abi_version_minor(void);
uint64_t get_required_cpu_features(void);
int32_t self_test_kernel_set(struct astro_status* status_out);

int32_t kernel_ephem_interp_f64_avx2(
    const struct astro_batch_header* hdr,
    const double* t,
    const double* coeff,
    uint32_t coeff_stride,
    double* pos_out,
    double* vel_out,
    struct astro_status* status_out);

#endif
```

## 9. Liaison Rust de reference

```rust
#[repr(C, align(32))]
pub struct AstroBatchHeader {
    pub element_count: u32,
    pub reserved: u32,
    pub timestamp_tag: u64,
}

#[repr(C, align(32))]
pub struct AstroStatus {
    pub code: i32,
    pub processed: u32,
    pub first_error_index: u32,
}

extern "C" {
    pub fn get_abi_version_major() -> i32;
    pub fn get_abi_version_minor() -> i32;
    pub fn get_required_cpu_features() -> u64;
    pub fn self_test_kernel_set(status_out: *mut AstroStatus) -> i32;
}
```

## 10. Regles numeriques

- Entrants NaN/Inf rejetes avec code erreur explicite.
- Denormals traites selon politique fixe du projet (FTZ/DAZ documentee).
- Tolerances numeriques definies par famille de kernel.
- Toute divergence scalaire versus AVX2 est tracee dans les rapports CI.

## 11. Tests obligatoires par kernel

- test ABI layout (taille, alignement, offsets),
- test argument invalides,
- test non-regression numerique sur corpus de reference,
- test performance versus baseline scalaire,
- test stabilite sur lots de grande taille.

## 12. Criteres d'acceptation

Un kernel AVX2 est validable seulement si:
- ABI conforme,
- precision conforme,
- performance conforme,
- comportement erreur conforme,
- journal de validation archive.
