#ifndef DIVIORA_SHARED_PROTOCOL_H
#define DIVIORA_SHARED_PROTOCOL_H

#include <windows.h>
#include <stdint.h>
#include <stddef.h>

#pragma pack(push, 1)
struct DivioraFrameHeader {
    uint32_t magic;
    uint16_t protocol_version;
    uint16_t message_type;
    uint32_t payload_length;
};

struct BiquadCoeffsWire {
    float b0;
    float b1;
    float b2;
    float a1;
    float a2;
};

struct DivioraPayloadWire {
    uint64_t revision;
    uint32_t sample_rate;
    uint32_t channels;
    float preamp_linear;
    uint32_t filter_count;
    BiquadCoeffsWire filters[64];
};

struct DivioraAckHeader {
    uint32_t status_code;
    uint64_t ack_revision;
};
#pragma pack(pop)

struct alignas(8) DivioraSharedState {
    uint32_t magic;
    uint32_t struct_version;
    volatile LONG seq_lock;
    DivioraPayloadWire payload;
};

struct alignas(8) ApoLiveDiagnostics {
    uint32_t magic;
    uint32_t struct_version;
    uint32_t instantiated;
    uint32_t format_valid;
    volatile LONG seq_lock;
    volatile LONG lock_calls;
    volatile LONG64 initialize_calls;
    volatile LONG64 process_calls;
    volatile LONG64 processed_frames;
    volatile LONG64 loaded_revision;
    uint32_t sample_rate;
    uint32_t channels;
    uint32_t is_hardcoded_baseline;
    uint32_t reserved;
};

#endif