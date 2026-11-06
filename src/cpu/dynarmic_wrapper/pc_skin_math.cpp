// Pure PC SSE2 position math; no guest/GL/Environment access. Shadow candidate only; output never committed by the observer.
#include <cstdint>
#include <cstddef>
#include <cstring>
#include <xmmintrin.h>
#if defined(_MSC_VER)
#pragma fenv_access(on)
#pragma float_control(precise, on)
#endif
extern "C" uint32_t touchHLE_pc_skin_positions(float *dst, const float *pos,
    const float *wgt, const int8_t *idx, const float *mat, uint32_t count) {
  const uint32_t saved = _mm_getcsr();
  // ARM Advanced SIMD standard controls: RNE, flush inputs/results, no traps.
  // All host controls and sticky flags are restored exactly before returning.
  _mm_setcsr(0x9fc0);
  for (uint32_t i = 0; i < count; ++i) {
    __m128 result = _mm_setzero_ps();
    for (unsigned b = 0; b < 4; ++b) {
      const float w = wgt[4*i+b];
      uint32_t bits;
      std::memcpy(&bits, &w, 4);
      if (b && !(bits & 0x70000000)) break;
      const float *m = mat + unsigned(idx[4*i+b])*16;
      __m128 v = _mm_mul_ps(_mm_loadu_ps(m), _mm_set1_ps(pos[3*i]));
      v = _mm_add_ps(v, _mm_mul_ps(_mm_loadu_ps(m+4), _mm_set1_ps(pos[3*i+1])));
      v = _mm_add_ps(v, _mm_mul_ps(_mm_loadu_ps(m+8), _mm_set1_ps(pos[3*i+2])));
      v = _mm_add_ps(v, _mm_loadu_ps(m+12));
      const __m128 weighted = _mm_mul_ps(v, _mm_set1_ps(w));
      result = b ? _mm_add_ps(result, weighted) : weighted;
    }
    float four[4];
    _mm_storeu_ps(four, result);
    std::memcpy(dst+3*i, four, 12);
  }
  const uint32_t status = _mm_getcsr() & 0x3f;
  _mm_setcsr(saved);
  const uint32_t arm = ((status & 1) ? 1 : 0) | ((status & 2) ? 0x80 : 0)
      | ((status & 4) ? 2 : 0) | ((status & 8) ? 4 : 0)
      | ((status & 16) ? 8 : 0) | ((status & 32) ? 16 : 0);
  // Rare invalid/denormal/overflow/underflow cases require native ARM fallback.
  // In particular SSE and ARM underflow cumulative-flag rules can differ.
  return arm | ((status & 0x1b) ? 0x80000000u : 0);
}

// Transform::PostMultiply candidate. Inputs are independent snapshots, including
// for guest self/partial aliases. Keep each ARM multiply/add rounding step.
extern "C" uint32_t touchHLE_pc_transform_multiply(float *dst, const float *a,
                                                   const float *b) {
  const uint32_t saved = _mm_getcsr();
  _mm_setcsr(0x9fc0);
  const __m128 a0 = _mm_loadu_ps(a);
  const __m128 a1 = _mm_loadu_ps(a+4);
  const __m128 a2 = _mm_loadu_ps(a+8);
  const __m128 a3 = _mm_loadu_ps(a+12);
  for (unsigned column=0; column<4; ++column) {
    __m128 v = _mm_mul_ps(a0, _mm_set1_ps(b[column*4]));
    v = _mm_add_ps(v, _mm_mul_ps(a1, _mm_set1_ps(b[column*4+1])));
    v = _mm_add_ps(v, _mm_mul_ps(a2, _mm_set1_ps(b[column*4+2])));
    v = _mm_add_ps(v, _mm_mul_ps(a3, _mm_set1_ps(b[column*4+3])));
    _mm_storeu_ps(dst+column*4, v);
  }
  const uint32_t status = _mm_getcsr() & 0x3f;
  _mm_setcsr(saved);
  const uint32_t arm = ((status & 1) ? 1 : 0) | ((status & 2) ? 0x80 : 0)
      | ((status & 4) ? 2 : 0) | ((status & 8) ? 4 : 0)
      | ((status & 16) ? 8 : 0) | ((status & 32) ? 16 : 0);
  return arm | ((status & 0x1b) ? 0x80000000u : 0);
}

// Synchronous read-only strided views; Rust validates every span/active index
// and ordinary input bit pattern first. Output is independent of all inputs.
extern "C" uint32_t touchHLE_pc_skin_positions_strided(float *dst,
    const uint8_t *pos, uint32_t pos_stride, const uint8_t *wgt,
    uint32_t wgt_stride, const int8_t *idx, uint32_t idx_stride,
    const uint8_t *mat, uint32_t count) {
  const uint32_t saved = _mm_getcsr();
  _mm_setcsr(0x9fc0);
  for (uint32_t i = 0; i < count; ++i) {
    float p[3];
    std::memcpy(p, pos + std::size_t(i)*pos_stride, sizeof(p));
    __m128 result = _mm_setzero_ps();
    for (unsigned b = 0; b < 4; ++b) {
      float w;
      std::memcpy(&w, wgt + std::size_t(i)*wgt_stride + b*4, 4);
      uint32_t bits;
      std::memcpy(&bits, &w, 4);
      if (b && !(bits & 0x70000000)) break;
      const uint8_t *m = mat + unsigned(idx[std::size_t(i)*idx_stride+b])*64;
      __m128 m0, m1, m2, m3;
      // Byte input alignment need not satisfy float/SIMD alignment.
      std::memcpy(&m0, m, 16); std::memcpy(&m1, m+16, 16);
      std::memcpy(&m2, m+32, 16); std::memcpy(&m3, m+48, 16);
      __m128 v = _mm_mul_ps(m0, _mm_set1_ps(p[0]));
      v = _mm_add_ps(v, _mm_mul_ps(m1, _mm_set1_ps(p[1])));
      v = _mm_add_ps(v, _mm_mul_ps(m2, _mm_set1_ps(p[2])));
      v = _mm_add_ps(v, m3);
      const __m128 weighted = _mm_mul_ps(v, _mm_set1_ps(w));
      result = b ? _mm_add_ps(result, weighted) : weighted;
    }
    float four[4];
    _mm_storeu_ps(four, result);
    std::memcpy(dst+3*i, four, 12);
  }
  const uint32_t status = _mm_getcsr() & 0x3f;
  _mm_setcsr(saved);
  const uint32_t arm = ((status & 1) ? 1 : 0) | ((status & 2) ? 0x80 : 0)
      | ((status & 4) ? 2 : 0) | ((status & 8) ? 4 : 0)
      | ((status & 16) ? 8 : 0) | ((status & 32) ? 16 : 0);
  return arm | ((status & 0x1b) ? 0x80000000u : 0);
}
