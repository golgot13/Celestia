global avx2_normalize_batch

section .text
bits 64
default rel

; C ABI prototype:
; void avx2_normalize_batch(double *dst, const double *src, uint64_t count);
; This assembly entry point is reserved for the AVX2 batch path.
; The scalar Rust reference implementation remains authoritative until the
; numerical contract has been validated on real scientific datasets.
avx2_normalize_batch:
    ; The initial production path keeps the Rust scalar validation layer as the
    ; source of truth for precision and branch correctness.
    ret
