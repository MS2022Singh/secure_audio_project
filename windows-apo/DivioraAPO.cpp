#include "DivioraAPO.h"
#include <cmath>
#include <cstring>
#include <combaseapi.h>  
#include <wchar.h>       

HANDLE DivioraEndpointAPO::s_hDiagMapping = nullptr;
ApoLiveDiagnostics* DivioraEndpointAPO::s_pLiveDiag = nullptr;

void DivioraEndpointAPO::EnsureDiagnosticsOpen() {
    if (s_pLiveDiag) return;

    s_hDiagMapping = OpenFileMappingW(FILE_MAP_ALL_ACCESS, FALSE, L"Global\\DivioraApoDiagnostics");
    if (!s_hDiagMapping) {
        s_hDiagMapping = CreateFileMappingW(
            INVALID_HANDLE_VALUE, NULL, PAGE_READWRITE, 0,
            sizeof(ApoLiveDiagnostics), L"Global\\DivioraApoDiagnostics");
    }
    if (!s_hDiagMapping) return;

    s_pLiveDiag = (ApoLiveDiagnostics*)MapViewOfFile(
        s_hDiagMapping, FILE_MAP_ALL_ACCESS, 0, 0, sizeof(ApoLiveDiagnostics));
    if (!s_pLiveDiag) {
        CloseHandle(s_hDiagMapping);
        s_hDiagMapping = nullptr;
        return;
    }

    if (s_pLiveDiag->magic != 0x44494147u) {
        s_pLiveDiag->magic = 0x44494147u; 
        s_pLiveDiag->struct_version = 1;
        s_pLiveDiag->sample_rate = kDivioraSupportedSampleRate;
        s_pLiveDiag->channels = kDivioraSupportedChannels;
    }
}

DivioraEndpointAPO::DivioraEndpointAPO()
    : m_cRef(1),
      m_bIsInitialized(false),
      m_bIsLocked(false),
      m_lockedSampleRate(0),
      m_lockedChannels(0),
      m_hSharedMapping(nullptr),
      m_pSharedState(nullptr),
      m_pChildApo(nullptr),
      m_pChildApoRT(nullptr),
      m_bChildAvailable(false),
      m_lastAppliedRevision(0),
      m_gainLinear(kUnityGainLinear)
{
    ZeroMemory(&m_cachedPayload, sizeof(m_cachedPayload));
    m_cachedPayload.preamp_linear = kUnityGainLinear;
    m_cachedPayload.filter_count = 0;
    ResetFilterStates();
}

DivioraEndpointAPO::~DivioraEndpointAPO() {
    if (m_pChildApoRT) { m_pChildApoRT->Release(); m_pChildApoRT = nullptr; }
    if (m_pChildApo) { m_pChildApo->Release(); m_pChildApo = nullptr; }
    CloseSharedMapping();
}

STDMETHODIMP DivioraEndpointAPO::QueryInterface(REFIID riid, void** ppvObject) {
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
    } else {
        *ppvObject = nullptr;
        return E_NOINTERFACE;
    }

    AddRef();
    return S_OK;
}

STDMETHODIMP_(ULONG) DivioraEndpointAPO::AddRef() {
    return InterlockedIncrement(&m_cRef);
}

STDMETHODIMP_(ULONG) DivioraEndpointAPO::Release() {
    ULONG ulRef = InterlockedDecrement(&m_cRef);
    if (ulRef == 0) {
        delete this;
    }
    return ulRef;
}

STDMETHODIMP DivioraEndpointAPO::Initialize(UINT32 cbDataSize, BYTE* pbyData) {
    UNREFERENCED_PARAMETER(cbDataSize);
    UNREFERENCED_PARAMETER(pbyData);

    if (m_bIsInitialized) {
        return APOERR_ALREADY_INITIALIZED;
    }

    OpenSharedMapping();
    EnsureDiagnosticsOpen();
    if (s_pLiveDiag) {
        InterlockedExchange((volatile LONG*)&s_pLiveDiag->instantiated, 1);
        InterlockedIncrement64(&s_pLiveDiag->initialize_calls);
    }

    HRESULT childHr = CoCreateInstance(
        CLSID_RealtekChildAPO, nullptr, CLSCTX_INPROC_SERVER,
        __uuidof(IAudioProcessingObject), (void**)&m_pChildApo);

    if (SUCCEEDED(childHr) && m_pChildApo) {
        HRESULT rtHr = m_pChildApo->QueryInterface(__uuidof(IAudioProcessingObjectRT), (void**)&m_pChildApoRT);
        if (SUCCEEDED(rtHr) && m_pChildApoRT) {
            m_pChildApo->Initialize(cbDataSize, pbyData);
            m_bChildAvailable = true;
        } else {
            m_pChildApo->Release();
            m_pChildApo = nullptr;
            m_bChildAvailable = false;
        }
    } else {
        m_pChildApo = nullptr;
        m_pChildApoRT = nullptr;
        m_bChildAvailable = false;
    }

    m_bIsInitialized = true;
    return S_OK;
}

HRESULT DivioraEndpointAPO::ValidateFormat(IAudioMediaType* pFormat) const {
    if (!pFormat) return E_POINTER;

    const WAVEFORMATEX* wfx = pFormat->GetAudioFormat();
    if (!wfx) return APOERR_FORMAT_NOT_SUPPORTED;

    if (wfx->nSamplesPerSec != kDivioraSupportedSampleRate ||
        wfx->nChannels != kDivioraSupportedChannels) {
        return APOERR_FORMAT_NOT_SUPPORTED;
    }

    bool isFloat32 =
        (wfx->wFormatTag == WAVE_FORMAT_IEEE_FLOAT && wfx->wBitsPerSample == 32);
    if (!isFloat32 && wfx->wFormatTag == WAVE_FORMAT_EXTENSIBLE) {
        const WAVEFORMATEXTENSIBLE* wfext = reinterpret_cast<const WAVEFORMATEXTENSIBLE*>(wfx);
        isFloat32 = (wfext->SubFormat == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT &&
                     wfx->wBitsPerSample == 32);
    }

    if (!isFloat32) {
        return APOERR_FORMAT_NOT_SUPPORTED;
    }

    return S_OK;
}

STDMETHODIMP DivioraEndpointAPO::IsInputFormatSupported(
    IAudioMediaType* pOppositeFormat,
    IAudioMediaType* pRequestedInputFormat,
    IAudioMediaType** ppSupportedInputFormat
) {
    UNREFERENCED_PARAMETER(pOppositeFormat);
    if (!pRequestedInputFormat) return E_POINTER;

    HRESULT hr = ValidateFormat(pRequestedInputFormat);
    if (FAILED(hr)) {
        return hr; 
    }

    if (ppSupportedInputFormat) {
        *ppSupportedInputFormat = pRequestedInputFormat;
        pRequestedInputFormat->AddRef();
    }
    return S_OK;
}

STDMETHODIMP DivioraEndpointAPO::IsOutputFormatSupported(
    IAudioMediaType* pOppositeFormat,
    IAudioMediaType* pRequestedOutputFormat,
    IAudioMediaType** ppSupportedOutputFormat
) {
    UNREFERENCED_PARAMETER(pOppositeFormat);
    if (!pRequestedOutputFormat) return E_POINTER;

    HRESULT hr = ValidateFormat(pRequestedOutputFormat);
    if (FAILED(hr)) {
        return hr;
    }

    if (ppSupportedOutputFormat) {
        *ppSupportedOutputFormat = pRequestedOutputFormat;
        pRequestedOutputFormat->AddRef();
    }
    return S_OK;
}

STDMETHODIMP DivioraEndpointAPO::GetLatency(HNSTIME* pTime) {
    if (!pTime) return E_POINTER;
    *pTime = 0; 
    return S_OK;
}

STDMETHODIMP DivioraEndpointAPO::GetInputChannelCount(UINT32* pu32ChannelCount) {
    if (!pu32ChannelCount) return E_POINTER;
    *pu32ChannelCount = kDivioraSupportedChannels;
    return S_OK;
}

STDMETHODIMP DivioraEndpointAPO::GetRegistrationProperties(APO_REG_PROPERTIES** ppApoProperties) {
    if (!ppApoProperties) return E_POINTER;

    APO_REG_PROPERTIES* pProps =
        static_cast<APO_REG_PROPERTIES*>(CoTaskMemAlloc(sizeof(APO_REG_PROPERTIES)));
    if (!pProps) return E_OUTOFMEMORY;

    ZeroMemory(pProps, sizeof(APO_REG_PROPERTIES));
    pProps->clsid = CLSID_DivioraEndpointAPO;
    pProps->Flags = static_cast<APO_FLAG>(0); 
    wcscpy_s(pProps->szFriendlyName, L"Diviora Endpoint APO");
    wcscpy_s(pProps->szCopyrightInfo, L"Diviora");
    pProps->u32MajorVersion = 1;
    pProps->u32MinorVersion = 0;
    pProps->u32MinInputConnections = 1;
    pProps->u32MaxInputConnections = 1;
    pProps->u32MinOutputConnections = 1;
    pProps->u32MaxOutputConnections = 1;
    pProps->u32MaxInstances = 8;
    pProps->u32NumAPOInterfaces = 1;
    pProps->iidAPOInterfaceList[0] = __uuidof(IAudioProcessingObjectRT);

    *ppApoProperties = pProps;
    return S_OK;
}

STDMETHODIMP DivioraEndpointAPO::Reset() {
    ResetFilterStates();
    m_gainLinear = kUnityGainLinear;
    m_lastAppliedRevision = 0;
    return S_OK;
}

STDMETHODIMP DivioraEndpointAPO::LockForProcess(
    UINT32 u32NumInputConnections,
    APO_CONNECTION_DESCRIPTOR** ppInputConnections,
    UINT32 u32NumOutputConnections,
    APO_CONNECTION_DESCRIPTOR** ppOutputConnections
) {
    if (!m_bIsInitialized) {
        return E_FAIL;
    }
    if (m_bIsLocked) {
        return APOERR_APO_LOCKED;
    }
    if (u32NumInputConnections != 1 || u32NumOutputConnections != 1) {
        return APOERR_NUM_CONNECTIONS_INVALID;
    }
    if (!ppInputConnections || !ppInputConnections[0] ||
        !ppOutputConnections || !ppOutputConnections[0]) {
        return E_POINTER;
    }

    m_lockedSampleRate = kDivioraSupportedSampleRate;
    m_lockedChannels = kDivioraSupportedChannels;

    ResetFilterStates();
    RecomputeGainLinear(kUnityGainLinear);
    m_lastAppliedRevision = 0;

    m_bIsLocked = true;
    if (s_pLiveDiag) {
        InterlockedExchange((volatile LONG*)&s_pLiveDiag->format_valid, 1);
        InterlockedIncrement((volatile LONG*)&s_pLiveDiag->lock_calls);
    }
    return S_OK;
}

STDMETHODIMP DivioraEndpointAPO::UnlockForProcess() {
    if (!m_bIsLocked) {
        return E_FAIL;
    }
    m_bIsLocked = false;
    return S_OK;
}

void DivioraEndpointAPO::OpenSharedMapping() {
    m_hSharedMapping = OpenFileMappingW(FILE_MAP_READ, FALSE, kDivioraSharedStateName);
    if (!m_hSharedMapping) {
        m_pSharedState = nullptr;
        return;
    }

    LPVOID pView = MapViewOfFile(m_hSharedMapping, FILE_MAP_READ, 0, 0, sizeof(DivioraSharedState));
    if (!pView) {
        CloseHandle(m_hSharedMapping);
        m_hSharedMapping = nullptr;
        m_pSharedState = nullptr;
        return;
    }

    m_pSharedState = reinterpret_cast<DivioraSharedState*>(pView);

    if (m_pSharedState->magic != 0x53544154u ||
        m_pSharedState->struct_version != 1) {
        UnmapViewOfFile(pView);
        CloseHandle(m_hSharedMapping);
        m_hSharedMapping = nullptr;
        m_pSharedState = nullptr;
    }
}

void DivioraEndpointAPO::CloseSharedMapping() {
    if (m_pSharedState) {
        UnmapViewOfFile(m_pSharedState);
        m_pSharedState = nullptr;
    }
    if (m_hSharedMapping) {
        CloseHandle(m_hSharedMapping);
        m_hSharedMapping = nullptr;
    }
}

bool DivioraEndpointAPO::TryReadSharedState(DivioraPayloadWire* pOut) {
    if (!m_pSharedState || !pOut) return false;

    for (UINT32 attempt = 0; attempt < kDivioraMaxSeqlockRetries; ++attempt) {
        LONG s1 = m_pSharedState->seq_lock;
        if (s1 & 1) {
            continue;
        }

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

void DivioraEndpointAPO::RecomputeGainLinear(float preampLinear) {
    m_gainLinear = preampLinear;
}

void DivioraEndpointAPO::ResetFilterStates() {
    for (int ch = 0; ch < 2; ++ch) {
        for (UINT32 f = 0; f < kDivioraMaxFilters; ++f) {
            m_filterStates[ch][f] = BiquadState{};
        }
    }
}

STDMETHODIMP_(void) DivioraEndpointAPO::APOProcess(
    UINT32 u32NumInputConnections,
    APO_CONNECTION_PROPERTY** ppInputConnections,
    UINT32 u32NumOutputConnections,
    APO_CONNECTION_PROPERTY** ppOutputConnections
) {
    if (u32NumInputConnections == 0 || !ppInputConnections || !ppInputConnections[0]) return;
    if (u32NumOutputConnections == 0 || !ppOutputConnections || !ppOutputConnections[0]) return;
    if (!m_bIsLocked) return;

    if (s_pLiveDiag) {
        InterlockedIncrement64(&s_pLiveDiag->process_calls);
    }

    UINT32 u32FrameCount = ppInputConnections[0]->u32ValidFrameCount;

    switch (ppInputConnections[0]->u32BufferFlags) {
        case BUFFER_INVALID:
            return;

        case BUFFER_SILENT: {
            float* pOutputSamples = reinterpret_cast<float*>(ppOutputConnections[0]->pBuffer);
            ZeroMemory(pOutputSamples, static_cast<size_t>(u32FrameCount) * m_lockedChannels * sizeof(float));
            ppOutputConnections[0]->u32ValidFrameCount = u32FrameCount;
            ppOutputConnections[0]->u32BufferFlags = BUFFER_SILENT;
            return;
        }

        case BUFFER_VALID:
        default:
            break; 
    }

    DivioraPayloadWire fresh;
    if (TryReadSharedState(&fresh)) {
        if (fresh.revision != 0 && fresh.revision != m_lastAppliedRevision) {
            m_cachedPayload = fresh;
            ResetFilterStates();
            RecomputeGainLinear(fresh.preamp_linear);
            m_lastAppliedRevision = fresh.revision;
            if (s_pLiveDiag) {
                InterlockedExchange64(&s_pLiveDiag->loaded_revision, fresh.revision);
            }
        }
    }

    float* pInputSamples = reinterpret_cast<float*>(ppInputConnections[0]->pBuffer);
    float* pOutputSamples = reinterpret_cast<float*>(ppOutputConnections[0]->pBuffer);

    const float gain = m_gainLinear;
    const uint32_t filterCount = m_cachedPayload.filter_count > kDivioraMaxFilters
        ? kDivioraMaxFilters
        : m_cachedPayload.filter_count;

    for (UINT32 frame = 0; frame < u32FrameCount; ++frame) {
        for (UINT32 ch = 0; ch < m_lockedChannels; ++ch) {
            UINT32 idx = frame * m_lockedChannels + ch;
            float x = pInputSamples[idx] * gain;

            for (uint32_t f = 0; f < filterCount; ++f) {
                const BiquadCoeffsWire& c = m_cachedPayload.filters[f];
                BiquadState& st = m_filterStates[ch][f];

                float y = c.b0 * x + st.s1;
                st.s1 = c.b1 * x - c.a1 * y + st.s2;
                st.s2 = c.b2 * x - c.a2 * y;
                x = y;
            }

            pOutputSamples[idx] = x;
        }
    }

    if (s_pLiveDiag) {
        InterlockedAdd64(&s_pLiveDiag->processed_frames, u32FrameCount);
    }

    ppOutputConnections[0]->u32ValidFrameCount = u32FrameCount;
    ppOutputConnections[0]->u32BufferFlags = BUFFER_VALID; 

    if (m_bChildAvailable && m_pChildApoRT) {
        m_pChildApoRT->APOProcess(u32NumInputConnections, ppOutputConnections,
                                   u32NumOutputConnections, ppOutputConnections);
    }
}

// Factory helper used by ClassFactory.cpp
HRESULT CreateDivioraEndpointAPO(REFIID riid, void** ppv) {
    DivioraEndpointAPO* pApo = new (std::nothrow) DivioraEndpointAPO();
    if (!pApo) return E_OUTOFMEMORY;

    HRESULT hr = pApo->QueryInterface(riid, ppv);
    pApo->Release();
    return hr;
}