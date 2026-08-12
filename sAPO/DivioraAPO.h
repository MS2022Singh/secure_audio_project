#pragma once
#include <windows.h>
#include <unknwn.h>
#include <audioenginebaseapo.h>
#include <atomic>

// GUID definition for Diviora System Processing Object Matrix
// CLSID: {D1B100A1-8888-4444-9999-000000000001}
static const GUID CLSID_DivioraEndpointAPO = 
{ 0xD1B100A1, 0x8888, 0x4444, { 0x99, 0x99, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01 } };

#pragma pack(push, 1)
struct DivioraDSPSharedState {
    uint32_t sample_rate;
    uint32_t channels;
    float gain_db;
    bool watermark_enabled;
    uint32_t watermark_seed;
    float fir_coefficients[512];
};
#pragma pack(pop)

class DivioraEndpointAPO : public IAudioProcessingObject, public IAudioProcessingObjectRT {
private:
    LONG m_cRef;
    DivioraDSPSharedState m_activeState;
    std::atomic<bool> m_pendingStateUpdate;

public:
    DivioraEndpointAPO();
    ~DivioraEndpointAPO();

    // IUnknown methods
    STDMETHODIMP QueryInterface(REFIID riid, void** ppvObject) override;
    STDMETHODIMP_(ULONG) AddRef() override;
    STDMETHODIMP_(ULONG) Release() override;

    // Real-Time Audio Processing Callback (audiodg.exe graph thread)
    STDMETHODIMP_(void) APOProcess(
        UINT32 u32NumInputConnections,
        APO_CONNECTION_PROPERTY** ppInputConnections,
        UINT32 u32NumOutputConnections,
        APO_CONNECTION_PROPERTY** ppOutputConnections
    ) override;
};
