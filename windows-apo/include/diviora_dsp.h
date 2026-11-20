// C ABI of the Rust DSP core (diviora-dsp/src/ffi.rs), linked as diviora_dsp.lib.
#pragma once
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct DvrEngine DvrEngine;

// Compile Equalizer APO text for one stream format. Returns NULL for "pass-through"
// (no stages or error). device_name: optional NUL-terminated UTF-16 endpoint name.
DvrEngine* dvr_engine_create(const uint8_t* text, size_t text_len, const uint16_t* device_name,
                             uint32_t sample_rate, uint32_t channels,
                             uint32_t* out_stage_count, uint32_t* out_warning_count);

// Real-time safe, in place. No-op if engine is NULL or channels mismatch.
void dvr_engine_process(DvrEngine* engine, float* buffer, uint32_t frames, uint32_t channels);

// Never call on the real-time thread.
void dvr_engine_destroy(DvrEngine* engine);

#ifdef __cplusplus
}
#endif
