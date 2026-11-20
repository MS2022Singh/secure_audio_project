#include "DivioraAPO.h"
#include "ConfigManager.h"
#include <cstring>

// "Switch to pass-through" marker for the engine hand-off (dvr_engine_create returns
// NULL for an empty/disabled config, and NULL already means "nothing pending").
static DvrEngine* const kBypassEngine = reinterpret_cast<DvrEngine*>(static_cast<uintptr_t>(1));

static void FreeEngine(DvrEngine* e) {
    if (e && e != kBypassEngine) dvr_engine_destroy(e);
}

// PKEY_Device_FriendlyName {a45c254e-df1c-4efd-8020-67d146a850e0},14 (defined here to
// avoid needing INITGUID for functiondiscoverykeys_devpkey.h).
static const PROPERTYKEY kPkeyDeviceFriendlyName =
    { { 0xa45c254e, 0xdf1c, 0x4efd, { 0x80, 0x20, 0x67, 0xd1, 0x46, 0xa8, 0x50, 0xe0 } }, 14 };

// KSDATAFORMAT_SUBTYPE_IEEE_FLOAT {00000003-0000-0010-8000-00aa00389b71}
static const GUID kSubtypeIeeeFloat =
    { 0x00000003, 0x0000, 0x0010, { 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71 } };

DivioraAPO::DivioraAPO()
    : m_cRef(1), m_registered(false), m_locked(false), m_formatIsFloat32(false),
      m_sampleRate(48000), m_channels(2), m_bytesPerFrame(8), m_pending(nullptr), m_active(nullptr), m_retired(nullptr) {}

DivioraAPO::~DivioraAPO() {
    if (m_registered) {
        ConfigManager::Instance().Unregister(this);
        std::lock_guard<std::mutex> g(ConfigManager::Instance().Mutex());
        if (m_locked) { m_locked = false; DivioraStats().locked--; }
    }
    FreeAllEngines();
}

// ---------------- IUnknown ----------------
STDMETHODIMP DivioraAPO::QueryInterface(REFIID riid, void** ppv) {
    if (!ppv) return E_POINTER;
    if (riid == __uuidof(IUnknown) || riid == __uuidof(IAudioProcessingObject))
        *ppv = static_cast<IAudioProcessingObject*>(this);
    else if (riid == __uuidof(IAudioProcessingObjectConfiguration))
        *ppv = static_cast<IAudioProcessingObjectConfiguration*>(this);
    else if (riid == __uuidof(IAudioProcessingObjectRT))
        *ppv = static_cast<IAudioProcessingObjectRT*>(this);
    else if (riid == __uuidof(IAudioSystemEffects))
        *ppv = static_cast<IAudioSystemEffects*>(this);
    else if (riid == __uuidof(IAudioSystemEffects2))
        *ppv = static_cast<IAudioSystemEffects2*>(this);
    else { *ppv = nullptr; return E_NOINTERFACE; }
    AddRef();
    return S_OK;
}
STDMETHODIMP_(ULONG) DivioraAPO::AddRef() { return InterlockedIncrement(&m_cRef); }
STDMETHODIMP_(ULONG) DivioraAPO::Release() {
    ULONG r = InterlockedDecrement(&m_cRef);
    if (r == 0) delete this;
    return r;
}

// ---------------- IAudioProcessingObject ----------------
STDMETHODIMP DivioraAPO::Reset() { return S_OK; }

STDMETHODIMP DivioraAPO::GetLatency(HNSTIME* pTime) {
    if (!pTime) return E_POINTER;
    *pTime = 0;
    return S_OK;
}

STDMETHODIMP DivioraAPO::GetRegistrationProperties(APO_REG_PROPERTIES** ppRegProps) {
    if (!ppRegProps) return E_POINTER;
    auto* p = static_cast<APO_REG_PROPERTIES*>(CoTaskMemAlloc(sizeof(APO_REG_PROPERTIES)));
    if (!p) return E_OUTOFMEMORY;
    ZeroMemory(p, sizeof(*p));
    p->clsid = CLSID_DivioraAPO;
    p->Flags = static_cast<APO_FLAG>(APO_FLAG_INPLACE | APO_FLAG_SAMPLESPERFRAME_MUST_MATCH |
                                     APO_FLAG_FRAMESPERSECOND_MUST_MATCH | APO_FLAG_BITSPERSAMPLE_MUST_MATCH);
    wcscpy_s(p->szFriendlyName, L"Diviora Endpoint APO");
    wcscpy_s(p->szCopyrightInfo, L"Diviora");
    p->u32MajorVersion = 1;
    p->u32MinorVersion = 1;
    p->u32MinInputConnections = 1;
    p->u32MaxInputConnections = 1;
    p->u32MinOutputConnections = 1;
    p->u32MaxOutputConnections = 1;
    p->u32MaxInstances = 0xFFFFFFFF; // one SFX instance per stream; never refuse a stream
    p->u32NumAPOInterfaces = 1;
    p->iidAPOInterfaceList[0] = __uuidof(IAudioProcessingObject);
    *ppRegProps = p;
    return S_OK;
}

STDMETHODIMP DivioraAPO::Initialize(UINT32 cbDataSize, BYTE* pbyData) {
    bool discoveryOnly = false;
    if (pbyData && cbDataSize >= sizeof(APOInitSystemEffects)) {
        auto* base = reinterpret_cast<APOInitBaseStruct*>(pbyData);
        auto* init = reinterpret_cast<APOInitSystemEffects*>(pbyData);
        if (base->cbSize == sizeof(APOInitSystemEffects2) && cbDataSize >= sizeof(APOInitSystemEffects2)) {
            discoveryOnly = reinterpret_cast<APOInitSystemEffects2*>(pbyData)->InitializeForDiscoveryOnly != FALSE;
        }
        // Endpoint name, so "Device:" blocks in the config apply to the right output.
        if (init->pAPOEndpointProperties) {
            PROPVARIANT v;
            PropVariantInit(&v);
            if (SUCCEEDED(init->pAPOEndpointProperties->GetValue(kPkeyDeviceFriendlyName, &v)) &&
                v.vt == VT_LPWSTR && v.pwszVal) {
                m_deviceName = v.pwszVal;
            }
            PropVariantClear(&v);
        }
    }
    if (!discoveryOnly && !m_registered) {
        ConfigManager::Instance().Register(this);
        m_registered = true;
    }
    return S_OK;
}

STDMETHODIMP DivioraAPO::IsInputFormatSupported(IAudioMediaType* pOpposite, IAudioMediaType* pRequested,
                                                IAudioMediaType** ppSupported) {
    UNREFERENCED_PARAMETER(pOpposite);
    if (!pRequested || !ppSupported) return E_POINTER;
    // In-place pass-through of whatever the engine negotiates; non-float formats are
    // passed through untouched (see LockForProcess).
    *ppSupported = pRequested;
    pRequested->AddRef();
    return S_OK;
}

STDMETHODIMP DivioraAPO::IsOutputFormatSupported(IAudioMediaType* pOpposite, IAudioMediaType* pRequested,
                                                 IAudioMediaType** ppSupported) {
    return IsInputFormatSupported(pOpposite, pRequested, ppSupported);
}

STDMETHODIMP DivioraAPO::GetInputChannelCount(UINT32* pu32ChannelCount) {
    if (!pu32ChannelCount) return E_POINTER;
    *pu32ChannelCount = m_channels;
    return S_OK;
}

// ---------------- IAudioProcessingObjectConfiguration ----------------
STDMETHODIMP DivioraAPO::LockForProcess(UINT32 numIn, APO_CONNECTION_DESCRIPTOR** ppIn,
                                        UINT32 numOut, APO_CONNECTION_DESCRIPTOR** ppOut) {
    if (numIn != 1 || numOut != 1) return APOERR_NUM_CONNECTIONS_INVALID;
    if (!ppIn || !ppIn[0] || !ppOut || !ppOut[0]) return E_POINTER;
    if (m_locked) return APOERR_APO_LOCKED;

    bool isFloat = false;
    UINT32 rate = 48000, channels = 2, blockAlign = 8;
    if (ppIn[0]->pFormat) {
        const WAVEFORMATEX* wfx = ppIn[0]->pFormat->GetAudioFormat();
        if (wfx) {
            rate = wfx->nSamplesPerSec;
            channels = wfx->nChannels;
            blockAlign = wfx->nBlockAlign;
            if (wfx->wBitsPerSample == 32) {
                if (wfx->wFormatTag == WAVE_FORMAT_IEEE_FLOAT) isFloat = true;
                else if (wfx->wFormatTag == WAVE_FORMAT_EXTENSIBLE && wfx->cbSize >= 22) {
                    const auto* ext = reinterpret_cast<const WAVEFORMATEXTENSIBLE*>(wfx);
                    isFloat = IsEqualGUID(ext->SubFormat, kSubtypeIeeeFloat) != FALSE;
                }
            }
        }
    }

    auto& mgr = ConfigManager::Instance();
    std::lock_guard<std::mutex> g(mgr.Mutex());
    FreeAllEngines();
    m_sampleRate = rate;
    m_channels = channels;
    m_bytesPerFrame = blockAlign;
    m_formatIsFloat32 = isFloat && channels >= 1 && channels <= 8;
    m_locked = true;
    // Not processing yet, so the active engine can be set directly.
    m_active = m_formatIsFloat32 ? Compile(mgr.CurrentText()) : nullptr;

    auto& st = DivioraStats();
    st.locked++;
    st.last_sample_rate = static_cast<LONG>(rate);
    st.last_channels = static_cast<LONG>(channels);
    if (!m_formatIsFloat32) st.non_float_streams++;
    return S_OK;
}

STDMETHODIMP DivioraAPO::UnlockForProcess() {
    std::lock_guard<std::mutex> g(ConfigManager::Instance().Mutex());
    if (!m_locked) return APOERR_ALREADY_UNLOCKED;
    m_locked = false;
    FreeAllEngines();
    DivioraStats().locked--;
    return S_OK;
}

// ---------------- IAudioProcessingObjectRT (audio thread) ----------------
// No allocation, locking, file access or COM calls in here.
STDMETHODIMP_(void) DivioraAPO::APOProcess(UINT32 numIn, APO_CONNECTION_PROPERTY** ppIn,
                                           UINT32 numOut, APO_CONNECTION_PROPERTY** ppOut) {
    auto& st = DivioraStats();
    st.process_calls.fetch_add(1, std::memory_order_relaxed);
    if (numIn == 0 || numOut == 0 || !ppIn || !ppOut || !ppIn[0] || !ppOut[0]) return;

    const UINT32 frames = ppIn[0]->u32ValidFrameCount;
    auto* in = reinterpret_cast<float*>(ppIn[0]->pBuffer);
    auto* out = reinterpret_cast<float*>(ppOut[0]->pBuffer);
    if (!in || !out) return;

    if (ppIn[0]->u32BufferFlags == BUFFER_SILENT) {
        if (out != in) ZeroMemory(out, static_cast<size_t>(frames) * m_bytesPerFrame);
        ppOut[0]->u32ValidFrameCount = frames;
        ppOut[0]->u32BufferFlags = BUFFER_SILENT;
        return;
    }
    if (out != in) {
        // Not expected (APO_FLAG_INPLACE), but keep audio flowing if it happens.
        memcpy(out, in, static_cast<size_t>(frames) * m_bytesPerFrame);
    }

    if (m_formatIsFloat32) {
        // Take a newly compiled engine, if any, unless the previous one is still waiting
        // to be freed by the control thread.
        if (m_retired.load(std::memory_order_acquire) == nullptr) {
            DvrEngine* next = m_pending.exchange(nullptr, std::memory_order_acq_rel);
            if (next) {
                m_retired.store(m_active, std::memory_order_release);
                m_active = next;
            }
        }
        if (m_active && m_active != kBypassEngine) {
            dvr_engine_process(m_active, out, frames, m_channels);
            st.filtered_frames.fetch_add(frames, std::memory_order_relaxed);
        }
    }
    st.processed_frames.fetch_add(frames, std::memory_order_relaxed);
    ppOut[0]->u32ValidFrameCount = frames;
    ppOut[0]->u32BufferFlags = BUFFER_VALID;
}

STDMETHODIMP_(UINT32) DivioraAPO::CalcInputFrames(UINT32 outFrames) { return outFrames; }
STDMETHODIMP_(UINT32) DivioraAPO::CalcOutputFrames(UINT32 inFrames) { return inFrames; }

// ---------------- IAudioSystemEffects2 ----------------
STDMETHODIMP DivioraAPO::GetEffectsList(LPGUID* ppEffectsIds, UINT* pcEffects, HANDLE Event) {
    UNREFERENCED_PARAMETER(Event);
    if (!ppEffectsIds || !pcEffects) return E_POINTER;
    *ppEffectsIds = nullptr;
    *pcEffects = 0;
    return S_OK;
}

// ---------------- Config hand-off (control thread, ConfigManager mutex held) ----------------
DvrEngine* DivioraAPO::Compile(const std::string& text) const {
    if (text.empty()) return kBypassEngine;
    DvrEngine* e = dvr_engine_create(reinterpret_cast<const uint8_t*>(text.data()), text.size(),
                                     m_deviceName.empty() ? nullptr : reinterpret_cast<const uint16_t*>(m_deviceName.c_str()),
                                     m_sampleRate, m_channels, nullptr, nullptr);
    return e ? e : kBypassEngine;
}

void DivioraAPO::OnConfigChanged(const std::string& text) {
    if (!m_locked || !m_formatIsFloat32) return;
    DvrEngine* old = m_pending.exchange(Compile(text), std::memory_order_acq_rel);
    FreeEngine(old); // superseded before the audio thread picked it up
}

void DivioraAPO::ReleaseRetired() {
    FreeEngine(m_retired.exchange(nullptr, std::memory_order_acq_rel));
}

void DivioraAPO::FreeAllEngines() {
    FreeEngine(m_pending.exchange(nullptr));
    FreeEngine(m_retired.exchange(nullptr));
    FreeEngine(m_active);
    m_active = nullptr;
}
