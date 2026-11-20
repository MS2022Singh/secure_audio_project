#pragma once
#include <windows.h>
#include <audioenginebaseapo.h>
#include <audioengineextensionapo.h>
#include <mmreg.h>
#include <cstdint>
#include <new>

// Canonical Diviora Wire Protocol (Matches DivioraControlService.cpp & Rust commands.rs)
static const wchar_t* kDivioraSharedStateName = L"Global\\DivioraSharedState_Default";
static const uint32_t kDivioraMaxFilters = 64;
static const uint32_t kDivioraMaxSeqlockRetries = 10;

#pragma pack(push, 1)
struct BiquadCoeffsWire {
    float b0 = 1.0f;
    float b1 = 0.0f;
    float b2 = 0.0f;
    float a1 = 0.0f;
    float a2 = 0.0f;
};

struct DivioraPayloadWire {
    uint64_t revision;
    uint32_t sample_rate;
    uint32_t channels;
    float preamp_linear;
    uint32_t filter_count;
    BiquadCoeffsWire filters[kDivioraMaxFilters];
};

struct DivioraSharedState {
    volatile LONG seq_lock;
    DivioraPayloadWire payload;
};

struct ApoLiveDiagnostics {
    uint32_t magic;
    uint32_t version;
    volatile LONG instantiated;
    volatile LONG format_valid;
    volatile LONG lock_calls;
    volatile LONG initialize_calls;
    volatile LONG unLock_calls;
    volatile LONG reserved;
    volatile int64_t process_calls;
    volatile int64_t processed_frames;
    volatile int64_t loaded_revision;
};
#pragma pack(pop)

// {D1B100A1-8888-4444-9999-000000000001}
static const CLSID CLSID_DivioraAPO =
    { 0xD1B100A1, 0x8888, 0x4444, { 0x99, 0x99, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01 } };

static const IID IID_IAudioSystemEffects2_Local =
    { 0xBAFE99D2, 0x7436, 0x44CE, { 0x9E, 0x0E, 0x4D, 0x89, 0xAF, 0xBF, 0xFF, 0x56 } };

static const IID IID_IAudioProcessingObjectPreferredFormats_Local =
    { 0x82C530C8, 0x0B2F, 0x43DE, { 0x87, 0x58, 0x15, 0x0E, 0x6B, 0x65, 0xED, 0x5C } };

#ifndef __IAudioSystemEffects2_INTERFACE_DEFINED__
MIDL_INTERFACE("BAFE99D2-7436-44CE-9E0E-4D89AFBFFF56")
IAudioSystemEffects2 : public IAudioSystemEffects
{
public:
    virtual HRESULT STDMETHODCALLTYPE GetEffectsList(
        LPGUID *ppEffectsIds,
        UINT *pcEffects,
        HANDLE Event) = 0;
};
#endif

#ifndef __IAudioProcessingObjectPreferredFormats_INTERFACE_DEFINED__
MIDL_INTERFACE("82C530C8-0B2F-43DE-8758-150E6B65ED5C")
IAudioProcessingObjectPreferredFormats : public IUnknown
{
public:
    virtual HRESULT STDMETHODCALLTYPE GetPreferredInputFormat(
        IAudioMediaType *pOppositeFormat,
        IAudioMediaType **ppPreferredFormat) = 0;

    virtual HRESULT STDMETHODCALLTYPE GetPreferredOutputFormat(
        IAudioMediaType *pOppositeFormat,
        IAudioMediaType **ppPreferredFormat) = 0;
};
#endif

struct BiquadState {
    float s1 = 0.0f;
    float s2 = 0.0f;
};

class DivioraAPO :
    public IAudioProcessingObject,
    public IAudioProcessingObjectConfiguration,
    public IAudioProcessingObjectRT,
    public IAudioSystemEffects2,
    public IAudioProcessingObjectPreferredFormats
{
public:
    DivioraAPO();
    virtual ~DivioraAPO();

    // IUnknown
    STDMETHODIMP QueryInterface(REFIID riid, void** ppvObject) override;
    STDMETHODIMP_(ULONG) AddRef() override;
    STDMETHODIMP_(ULONG) Release() override;

    // IAudioProcessingObject
    STDMETHODIMP Initialize(UINT32 cbDataSize, BYTE* pbyData) override;
    STDMETHODIMP IsInputFormatSupported(IAudioMediaType* pOppositeFormat, IAudioMediaType* pRequestedInputFormat, IAudioMediaType** ppSupportedInputFormat) override;
    STDMETHODIMP IsOutputFormatSupported(IAudioMediaType* pOppositeFormat, IAudioMediaType* pRequestedOutputFormat, IAudioMediaType** ppSupportedOutputFormat) override;
    STDMETHODIMP GetRegistrationProperties(APO_REG_PROPERTIES** ppRegProperties) override;
    STDMETHODIMP Reset() override;
    STDMETHODIMP GetLatency(HNSTIME* pTime) override;
    STDMETHODIMP GetInputChannelCount(UINT32* pu32ChannelCount) override;

    // IAudioProcessingObjectConfiguration
    STDMETHODIMP LockForProcess(UINT32 u32NumInputConnections, APO_CONNECTION_DESCRIPTOR** ppInputConnectionDescriptors, UINT32 u32NumOutputConnections, APO_CONNECTION_DESCRIPTOR** ppOutputConnectionDescriptors) override;
    STDMETHODIMP UnlockForProcess() override;

    // IAudioProcessingObjectRT
    STDMETHODIMP_(void) APOProcess(UINT32 u32NumInputConnections, APO_CONNECTION_PROPERTY** ppInputConnectionProperty, UINT32 u32NumOutputConnections, APO_CONNECTION_PROPERTY** ppOutputConnectionProperty) override;
    STDMETHODIMP_(UINT32) CalcInputFrames(UINT32 u32OutputFrameCount) override;
    STDMETHODIMP_(UINT32) CalcOutputFrames(UINT32 u32InputFrameCount) override;

    // IAudioSystemEffects2
    STDMETHODIMP GetEffectsList(LPGUID* ppEffectsIds, UINT* pcEffects, HANDLE Event) override;

    // IAudioProcessingObjectPreferredFormats
    STDMETHODIMP GetPreferredInputFormat(IAudioMediaType* pOppositeFormat, IAudioMediaType** ppPreferredFormat) override;
    STDMETHODIMP GetPreferredOutputFormat(IAudioMediaType* pOppositeFormat, IAudioMediaType** ppPreferredFormat) override;

private:
    volatile LONG m_cRef;
    bool m_bIsInitialized;
    bool m_bIsLocked;
    UINT32 m_lockedSampleRate;
    UINT32 m_lockedChannels;

    HANDLE m_hSharedMapping;
    DivioraSharedState* m_pSharedState;

    DivioraPayloadWire m_cachedPayload;
    uint64_t m_lastAppliedRevision;
    float m_gainLinear;
    BiquadState m_filterStates[8][kDivioraMaxFilters];

    void OpenSharedMapping();
    void CloseSharedMapping();
    bool TryReadSharedState(DivioraPayloadWire* pOut);
    void RecomputeGainLinear(float preampLinear);
    void ResetFilterStates();
    HRESULT ValidateFormat(IAudioMediaType* pFormat) const;
};