#include "DivioraAPO.h"

DivioraEndpointAPO::DivioraEndpointAPO() : m_cRef(1), m_pendingStateUpdate(false) {
    m_activeState.sample_rate = 48000;
    m_activeState.channels = 2;
    m_activeState.gain_db = 0.0f;
    m_activeState.watermark_enabled = false;
}

DivioraEndpointAPO::~DivioraEndpointAPO() {}

STDMETHODIMP DivioraEndpointAPO::QueryInterface(REFIID riid, void** ppvObject) {
    if (!ppvObject) return E_POINTER;
    if (riid == IID_IUnknown || riid == __uuidof(IAudioProcessingObject)) {
        *ppvObject = static_cast<IAudioProcessingObject*>(this);
    } else if (riid == __uuidof(IAudioProcessingObjectRT)) {
        *ppvObject = static_cast<IAudioProcessingObjectRT*>(this);
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

// Zero-Lock Real-Time Processing Thread Callback inside audiodg.exe
STDMETHODIMP_(void) DivioraEndpointAPO::APOProcess(
    UINT32 u32NumInputConnections,
    APO_CONNECTION_PROPERTY** ppInputConnections,
    UINT32 u32NumOutputConnections,
    APO_CONNECTION_PROPERTY** ppOutputConnections
) {
    if (u32NumInputConnections == 0 || !ppInputConnections || !ppInputConnections[0]) return;
    if (u32NumOutputConnections == 0 || !ppOutputConnections || !ppOutputConnections[0]) return;

    float* pInputSamples = reinterpret_cast<float*>(ppInputConnections[0]->pBuffer);
    float* pOutputSamples = reinterpret_cast<float*>(ppOutputConnections[0]->pBuffer);
    UINT32 u32FrameCount = ppInputConnections[0]->u32ValidFrameCount;

    // Passthrough with real-time zero-copy processing buffer loop
    for (UINT32 i = 0; i < u32FrameCount * m_activeState.channels; ++i) {
        pOutputSamples[i] = pInputSamples[i];
    }
}
