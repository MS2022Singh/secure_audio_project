#pragma once
#include <windows.h>
#include <unknwn.h>
#include <mmreg.h>
#include <ks.h>
#include <ksmedia.h>
#include <audioenginebaseapo.h>
#include <audiomediatype.h>
#include <atomic>
#include <cstdint>
#include "shared_protocol.h"

static const GUID CLSID_DivioraEndpointAPO =
{ 0xD1B100A1, 0x8888, 0x4444, { 0x99, 0x99, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01 } };

constexpr UINT32 kDivioraSupportedSampleRate = 48000;
constexpr UINT32 kDivioraSupportedChannels   = 2;
constexpr UINT32 kDivioraMaxFilters          = 64;
constexpr UINT32 kDivioraMaxSeqlockRetries   = 4;

static const GUID CLSID_RealtekChildAPO =
{ 0xC9453E73, 0x8C5C, 0x4463, { 0x99, 0x84, 0xAF, 0x8B, 0xAB, 0x2F, 0x54, 0x47 } };

static const wchar_t kDivioraSharedStateName[] = L"Global\\DivioraSharedState_Default";

struct BiquadState {
    float s1 = 0.0f;
    float s2 = 0.0f;
};

class DivioraEndpointAPO :
    public IAudioProcessingObject,
    public IAudioProcessingObjectConfiguration,
    public IAudioProcessingObjectRT,
    public IAudioSystemEffects
{
private:
    LONG m_cRef;
    bool  m_bIsInitialized;
    bool  m_bIsLocked;
    UINT32 m_lockedSampleRate;
    UINT32 m_lockedChannels;

    HANDLE m_hSharedMapping;
    DivioraSharedState* m_pSharedState;

    static HANDLE s_hDiagMapping;
    static ApoLiveDiagnostics* s_pLiveDiag;
    void EnsureDiagnosticsOpen();

    IAudioProcessingObject* m_pChildApo;
    IAudioProcessingObjectRT* m_pChildApoRT;
    bool m_bChildAvailable;

    DivioraPayloadWire m_cachedPayload;
    uint64_t           m_lastAppliedRevision;
    float              m_gainLinear;
    BiquadState        m_filterStates[8][kDivioraMaxFilters];

    static constexpr float kUnityGainLinear = 1.0f;

    HRESULT ValidateFormat(IAudioMediaType* pFormat) const;
    void    RecomputeGainLinear(float preampLinear);
    void    ResetFilterStates();
    bool    TryReadSharedState(DivioraPayloadWire* pOut);
    void    OpenSharedMapping();
    void    CloseSharedMapping();

public:
    DivioraEndpointAPO();
    ~DivioraEndpointAPO();

    STDMETHODIMP QueryInterface(REFIID riid, void** ppvObject) override;
    STDMETHODIMP_(ULONG) AddRef() override;
    STDMETHODIMP_(ULONG) Release() override;

    STDMETHODIMP Initialize(UINT32 cbDataSize, BYTE* pbyData) override;
    STDMETHODIMP IsInputFormatSupported(IAudioMediaType* pOppositeFormat, IAudioMediaType* pRequestedInputFormat, IAudioMediaType** ppSupportedInputFormat) override;
    STDMETHODIMP IsOutputFormatSupported(IAudioMediaType* pOppositeFormat, IAudioMediaType* pRequestedOutputFormat, IAudioMediaType** ppSupportedOutputFormat) override;
    STDMETHODIMP GetLatency(HNSTIME* pTime) override;
    STDMETHODIMP GetInputChannelCount(UINT32* pu32ChannelCount) override;
    STDMETHODIMP GetRegistrationProperties(APO_REG_PROPERTIES** ppApoProperties) override;
    STDMETHODIMP Reset() override;

    STDMETHODIMP LockForProcess(UINT32 u32NumInputConnections, APO_CONNECTION_DESCRIPTOR** ppInputConnections, UINT32 u32NumOutputConnections, APO_CONNECTION_DESCRIPTOR** ppOutputConnections) override;
    STDMETHODIMP UnlockForProcess() override;

    // IAudioProcessingObjectRT required methods
    STDMETHODIMP_(UINT32) CalcInputFrames(UINT32 u32OutputFrameCount) override { return u32OutputFrameCount; }
    STDMETHODIMP_(UINT32) CalcOutputFrames(UINT32 u32InputFrameCount) override { return u32InputFrameCount; }

    STDMETHODIMP_(void) APOProcess(UINT32 u32NumInputConnections, APO_CONNECTION_PROPERTY** ppInputConnections, UINT32 u32NumOutputConnections, APO_CONNECTION_PROPERTY** ppOutputConnections) override;
};