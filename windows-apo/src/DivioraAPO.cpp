#include "DivioraAPO.h"
#include <cmath>
#include <cstring>
#include <combaseapi.h>

static ApoLiveDiagnostics* g_pLiveDiag = nullptr;
static HANDLE g_hDiagMap = NULL;

static void EnsureSharedDiagnostics() {
    if (g_pLiveDiag) return;

    g_hDiagMap = OpenFileMappingW(FILE_MAP_READ | FILE_MAP_WRITE, FALSE, L"Global\\DivioraApoDiagnostics");
    if (!g_hDiagMap) return;

    g_pLiveDiag = (ApoLiveDiagnostics*)MapViewOfFile(
        g_hDiagMap, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, sizeof(ApoLiveDiagnostics)
    );
    if (g_pLiveDiag) {
        InterlockedExchange((volatile LONG*)&g_pLiveDiag->instantiated, 1);
    }
}

DivioraAPO::DivioraAPO()
    : m_cRef(1), m_bIsInitialized(false), m_bIsLocked(false),
      m_lockedSampleRate(48000), m_lockedChannels(2),
      m_hSharedMapping(nullptr), m_pSharedState(nullptr),
      m_lastAppliedRevision(0), m_gainLinear(1.0f)
{
    EnsureSharedDiagnostics();
    ZeroMemory(&m_cachedPayload, sizeof(m_cachedPayload));
    m_cachedPayload.preamp_linear = 1.0f;
    m_cachedPayload.filter_count = 0;
    ResetFilterStates();
}

DivioraAPO::~DivioraAPO() {
    CloseSharedMapping();
}

STDMETHODIMP DivioraAPO::QueryInterface(REFIID riid, void** ppvObject) {
    if (!ppvObject) return E_POINTER;

    if (riid == IID_IUnknown) {
        *ppvObject = static_cast<IUnknown*>(static_cast<IAudioProcessingObject*>(this));
    } else if (riid == __uuidof(IAudioProcessingObject)) {
        *ppvObject = static_cast<IAudioProcessingObject*>(this);
    } else if (riid == __uuidof(IAudioProcessingObjectConfiguration)) {
        *ppvObject = static_cast<IAudioProcessingObjectConfiguration*>(this);
    } else if (riid == __uuidof(IAudioProcessingObjectRT)) {
        *ppvObject = static_cast<IAudioProcessingObjectRT*>(this);
    } else if (riid == __uuidof(IAudioSystemEffects)) {
        *ppvObject = static_cast<IAudioSystemEffects*>(this);
    } else if (riid == IID_IAudioSystemEffects2_Local) {
        *ppvObject = static_cast<IAudioSystemEffects2*>(this);
    } else if (riid == IID_IAudioProcessingObjectPreferredFormats_Local) {
        *ppvObject = static_cast<IAudioProcessingObjectPreferredFormats*>(this);
    } else {
        *ppvObject = nullptr;
        return E_NOINTERFACE;
    }

    AddRef();
    return S_OK;
}

STDMETHODIMP_(ULONG) DivioraAPO::AddRef() { return InterlockedIncrement(&m_cRef); }
STDMETHODIMP_(ULONG) DivioraAPO::Release() {
    ULONG ulRef = InterlockedDecrement(&m_cRef);
    if (ulRef == 0) delete this;
    return ulRef;
}

STDMETHODIMP DivioraAPO::Initialize(UINT32 cbDataSize, BYTE* pbyData) {
    UNREFERENCED_PARAMETER(cbDataSize);
    UNREFERENCED_PARAMETER(pbyData);
    EnsureSharedDiagnostics();
    if (g_pLiveDiag) InterlockedIncrement((volatile LONG*)&g_pLiveDiag->initialize_calls);
    OpenSharedMapping();
    m_bIsInitialized = true;
    return S_OK;
}

HRESULT DivioraAPO::ValidateFormat(IAudioMediaType* pFormat) const {
    if (!pFormat) return E_POINTER;
    const WAVEFORMATEX* wfx = pFormat->GetAudioFormat();
    if (!wfx) return APOERR_FORMAT_NOT_SUPPORTED;

    if (wfx->nSamplesPerSec < 44100 || wfx->nSamplesPerSec > 192000) return APOERR_FORMAT_NOT_SUPPORTED;
    if (wfx->nChannels < 1 || wfx->nChannels > 8) return APOERR_FORMAT_NOT_SUPPORTED;

    return S_OK;
}

STDMETHODIMP DivioraAPO::IsInputFormatSupported(IAudioMediaType* pOpp, IAudioMediaType* pReq, IAudioMediaType** ppSup) {
    UNREFERENCED_PARAMETER(pOpp);
    if (!pReq) return E_POINTER;
    HRESULT hr = ValidateFormat(pReq);
    if (SUCCEEDED(hr)) {
        if (ppSup) { *ppSup = pReq; pReq->AddRef(); }
        return S_OK;
    }
    if (ppSup) *ppSup = nullptr;
    return hr;
}

STDMETHODIMP DivioraAPO::IsOutputFormatSupported(IAudioMediaType* pOpp, IAudioMediaType* pReq, IAudioMediaType** ppSup) {
    return IsInputFormatSupported(pOpp, pReq, ppSup);
}

STDMETHODIMP DivioraAPO::GetLatency(HNSTIME* pTime) {
    if (!pTime) return E_POINTER;
    *pTime = 0;
    return S_OK;
}

STDMETHODIMP DivioraAPO::GetInputChannelCount(UINT32* pu32ChannelCount) {
    if (!pu32ChannelCount) return E_POINTER;
    *pu32ChannelCount = m_lockedChannels;
    return S_OK;
}

STDMETHODIMP DivioraAPO::GetRegistrationProperties(APO_REG_PROPERTIES** ppApoProperties) {
    if (!ppApoProperties) return E_POINTER;
    APO_REG_PROPERTIES* pProps = static_cast<APO_REG_PROPERTIES*>(CoTaskMemAlloc(sizeof(APO_REG_PROPERTIES)));
    if (!pProps) return E_OUTOFMEMORY;
    ZeroMemory(pProps, sizeof(APO_REG_PROPERTIES));
    pProps->clsid = CLSID_DivioraAPO;
    pProps->Flags = static_cast<APO_FLAG>(APO_FLAG_INPLACE | APO_FLAG_SAMPLESPERFRAME_MUST_MATCH | APO_FLAG_FRAMESPERSECOND_MUST_MATCH | APO_FLAG_BITSPERSAMPLE_MUST_MATCH);
    wcscpy_s(pProps->szFriendlyName, L"Diviora Endpoint APO");
    wcscpy_s(pProps->szCopyrightInfo, L"Diviora");
    pProps->u32MajorVersion = 1;
    pProps->u32MinorVersion = 0;
    pProps->u32MinInputConnections = 1;
    pProps->u32MaxInputConnections = 1;
    pProps->u32MinOutputConnections = 1;
    pProps->u32MaxOutputConnections = 1;
    pProps->u32MaxInstances = 8;
    pProps->u32NumAPOInterfaces = 2;
    pProps->iidAPOInterfaceList[0] = __uuidof(IAudioProcessingObjectRT);
    pProps->iidAPOInterfaceList[1] = IID_IAudioSystemEffects2_Local;
    *ppApoProperties = pProps;
    return S_OK;
}

STDMETHODIMP DivioraAPO::GetEffectsList(LPGUID* ppEffectsIds, UINT* pcEffects, HANDLE Event) {
    UNREFERENCED_PARAMETER(Event);
    if (!ppEffectsIds || !pcEffects) return E_POINTER;
    *ppEffectsIds = nullptr;
    *pcEffects = 0;
    return S_OK;
}

STDMETHODIMP DivioraAPO::GetPreferredInputFormat(IAudioMediaType* pOppositeFormat, IAudioMediaType** ppPreferredFormat) {
    if (!ppPreferredFormat) return E_POINTER;
    if (pOppositeFormat) {
        *ppPreferredFormat = pOppositeFormat;
        pOppositeFormat->AddRef();
        return S_OK;
    }
    *ppPreferredFormat = nullptr;
    return S_OK;
}

STDMETHODIMP DivioraAPO::GetPreferredOutputFormat(IAudioMediaType* pOppositeFormat, IAudioMediaType** ppPreferredFormat) {
    return GetPreferredInputFormat(pOppositeFormat, ppPreferredFormat);
}

STDMETHODIMP DivioraAPO::Reset() {
    ResetFilterStates();
    m_gainLinear = 1.0f;
    m_lastAppliedRevision = 0;
    return S_OK;
}

STDMETHODIMP DivioraAPO::LockForProcess(UINT32 numIn, APO_CONNECTION_DESCRIPTOR** ppIn, UINT32 numOut, APO_CONNECTION_DESCRIPTOR** ppOut) {
    if (numIn != 1 || numOut != 1) return APOERR_NUM_CONNECTIONS_INVALID;
    if (!ppIn || !ppIn[0] || !ppOut || !ppOut[0]) return E_POINTER;
    if (ppIn[0]->pFormat) {
        const WAVEFORMATEX* wfx = ppIn[0]->pFormat->GetAudioFormat();
        if (wfx) {
            m_lockedSampleRate = wfx->nSamplesPerSec;
            m_lockedChannels   = wfx->nChannels;
        }
    }
    m_bIsLocked = true;
    EnsureSharedDiagnostics();
    if (g_pLiveDiag) {
        InterlockedExchange((volatile LONG*)&g_pLiveDiag->format_valid, 1);
        InterlockedIncrement((volatile LONG*)&g_pLiveDiag->lock_calls);
    }
    return S_OK;
}

STDMETHODIMP DivioraAPO::UnlockForProcess() {
    if (!m_bIsLocked) return E_FAIL;
    m_bIsLocked = false;
    EnsureSharedDiagnostics();
    if (g_pLiveDiag) InterlockedExchange((volatile LONG*)&g_pLiveDiag->format_valid, 0);
    return S_OK;
}

STDMETHODIMP_(UINT32) DivioraAPO::CalcInputFrames(UINT32 outFrames) { return outFrames; }
STDMETHODIMP_(UINT32) DivioraAPO::CalcOutputFrames(UINT32 inFrames) { return inFrames; }

void DivioraAPO::OpenSharedMapping() {
    if (m_pSharedState) return;
    m_hSharedMapping = OpenFileMappingW(FILE_MAP_READ, FALSE, kDivioraSharedStateName);
    if (!m_hSharedMapping) return;

    LPVOID pView = MapViewOfFile(m_hSharedMapping, FILE_MAP_READ, 0, 0, sizeof(DivioraSharedState));
    if (!pView) {
        CloseHandle(m_hSharedMapping);
        m_hSharedMapping = nullptr;
        return;
    }
    m_pSharedState = reinterpret_cast<DivioraSharedState*>(pView);
}

void DivioraAPO::CloseSharedMapping() {
    if (m_pSharedState) {
        UnmapViewOfFile(m_pSharedState);
        m_pSharedState = nullptr;
    }
    if (m_hSharedMapping) {
        CloseHandle(m_hSharedMapping);
        m_hSharedMapping = nullptr;
    }
}

bool DivioraAPO::TryReadSharedState(DivioraPayloadWire* pOut) {
    if (!m_pSharedState) OpenSharedMapping();
    if (!m_pSharedState || !pOut) return false;

    for (UINT32 attempt = 0; attempt < kDivioraMaxSeqlockRetries; ++attempt) {
        LONG s1 = m_pSharedState->seq_lock;
        if (s1 & 1) continue;

        MemoryBarrier();
        DivioraPayloadWire snapshot = m_pSharedState->payload;
        MemoryBarrier();

        LONG s2 = m_pSharedState->seq_lock;
        if (s1 == s2) {
            *pOut = snapshot;
            return true;
        }
    }
    return false;
}

void DivioraAPO::RecomputeGainLinear(float preampLinear) { m_gainLinear = preampLinear; }

void DivioraAPO::ResetFilterStates() {
    for (int ch = 0; ch < 8; ++ch) {
        for (UINT32 f = 0; f < kDivioraMaxFilters; ++f) m_filterStates[ch][f] = BiquadState{};
    }
}

STDMETHODIMP_(void) DivioraAPO::APOProcess(UINT32 numIn, APO_CONNECTION_PROPERTY** ppIn, UINT32 numOut, APO_CONNECTION_PROPERTY** ppOut) {
    EnsureSharedDiagnostics();
    if (g_pLiveDiag) InterlockedIncrement64(&g_pLiveDiag->process_calls);
    if (numIn == 0 || !ppIn || !ppIn[0] || numOut == 0 || !ppOut || !ppOut[0] || !m_bIsLocked) return;

    UINT32 u32FrameCount = ppIn[0]->u32ValidFrameCount;
    float* pIn = reinterpret_cast<float*>(ppIn[0]->pBuffer);
    float* pOut = reinterpret_cast<float*>(ppOut[0]->pBuffer);
    if (!pIn || !pOut) return;

    const UINT32 channels = (m_lockedChannels > 0 && m_lockedChannels <= 8) ? m_lockedChannels : 2;

    if (ppIn[0]->u32BufferFlags == BUFFER_SILENT) {
        ZeroMemory(pOut, static_cast<size_t>(u32FrameCount) * channels * sizeof(float));
        ppOut[0]->u32ValidFrameCount = u32FrameCount;
        ppOut[0]->u32BufferFlags = BUFFER_SILENT;
        return;
    }

    DivioraPayloadWire fresh;
    if (TryReadSharedState(&fresh)) {
        if (fresh.revision != 0 && fresh.revision != m_lastAppliedRevision) {
            m_cachedPayload = fresh;
            ResetFilterStates();
            RecomputeGainLinear(fresh.preamp_linear);
            m_lastAppliedRevision = fresh.revision;

            if (g_pLiveDiag) {
                InterlockedExchange64(&g_pLiveDiag->loaded_revision, fresh.revision);
            }
        }
    }

    if (g_pLiveDiag) InterlockedAdd64(&g_pLiveDiag->processed_frames, u32FrameCount);

    const float gain = m_gainLinear;
    const uint32_t filterCount = m_cachedPayload.filter_count > kDivioraMaxFilters ? kDivioraMaxFilters : m_cachedPayload.filter_count;

    for (UINT32 frame = 0; frame < u32FrameCount; ++frame) {
        for (UINT32 ch = 0; ch < channels; ++ch) {
            UINT32 idx = frame * channels + ch;
            float x = pIn[idx] * gain;

            for (uint32_t f = 0; f < filterCount; ++f) {
                const BiquadCoeffsWire& c = m_cachedPayload.filters[f];
                BiquadState& st = m_filterStates[ch][f];
                float y = c.b0 * x + st.s1;
                st.s1 = c.b1 * x - c.a1 * y + st.s2;
                st.s2 = c.b2 * x - c.a2 * y;
                x = y;
            }
            pOut[idx] = x;
        }
    }
    ppOut[0]->u32ValidFrameCount = u32FrameCount;
    ppOut[0]->u32BufferFlags = BUFFER_VALID;
}