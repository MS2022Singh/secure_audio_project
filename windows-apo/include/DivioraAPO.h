// Diviora system-effect APO.
//
// Works the way Equalizer APO does: Windows loads this DLL into audiodg.exe for the
// endpoint whose stream-effect slot ({d04e05a6-...},5) names our CLSID. The APO reads
// the encrypted, ACL-locked configuration written by the Diviora app
// (%ProgramData%\Diviora\apo\config.dvr), reloads it when it changes, and runs the
// shared Rust DSP engine on the audio. No service, no shared memory, no virtual cable.
#pragma once
#include <windows.h>
#include <audioenginebaseapo.h>
#include <mmreg.h>
#include <atomic>
#include <string>
#include "diviora_dsp.h"

// {D1B100A1-8888-4444-9999-000000000001}
static const CLSID CLSID_DivioraAPO =
    { 0xD1B100A1, 0x8888, 0x4444, { 0x99, 0x99, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01 } };

class DivioraAPO :
    public IAudioProcessingObject,
    public IAudioProcessingObjectConfiguration,
    public IAudioProcessingObjectRT,
    public IAudioSystemEffects2
{
public:
    // pUnkOuter: the controlling object when Windows audio aggregates the APO
    // (audiodg.exe always does), or nullptr for standalone use (self-test).
    explicit DivioraAPO(IUnknown* pUnkOuter = nullptr);
    virtual ~DivioraAPO();

    // The inner (non-delegating) IUnknown handed back to an aggregating outer object.
    IUnknown* NonDelegatingUnknown() { return &m_inner; }

    // IUnknown of every interface: delegates to the controlling (outer) unknown.
    STDMETHODIMP QueryInterface(REFIID riid, void** ppvObject) override;
    STDMETHODIMP_(ULONG) AddRef() override;
    STDMETHODIMP_(ULONG) Release() override;

    // IAudioProcessingObject
    STDMETHODIMP Reset() override;
    STDMETHODIMP GetLatency(HNSTIME* pTime) override;
    STDMETHODIMP GetRegistrationProperties(APO_REG_PROPERTIES** ppRegProps) override;
    STDMETHODIMP Initialize(UINT32 cbDataSize, BYTE* pbyData) override;
    STDMETHODIMP IsInputFormatSupported(IAudioMediaType* pOppositeFormat, IAudioMediaType* pRequestedInputFormat, IAudioMediaType** ppSupportedInputFormat) override;
    STDMETHODIMP IsOutputFormatSupported(IAudioMediaType* pOppositeFormat, IAudioMediaType* pRequestedOutputFormat, IAudioMediaType** ppSupportedOutputFormat) override;
    STDMETHODIMP GetInputChannelCount(UINT32* pu32ChannelCount) override;

    // IAudioProcessingObjectConfiguration
    STDMETHODIMP LockForProcess(UINT32 u32NumInputConnections, APO_CONNECTION_DESCRIPTOR** ppInputConnections, UINT32 u32NumOutputConnections, APO_CONNECTION_DESCRIPTOR** ppOutputConnections) override;
    STDMETHODIMP UnlockForProcess() override;

    // IAudioProcessingObjectRT
    STDMETHODIMP_(void) APOProcess(UINT32 u32NumInputConnections, APO_CONNECTION_PROPERTY** ppInputConnections, UINT32 u32NumOutputConnections, APO_CONNECTION_PROPERTY** ppOutputConnections) override;
    STDMETHODIMP_(UINT32) CalcInputFrames(UINT32 u32OutputFrameCount) override;
    STDMETHODIMP_(UINT32) CalcOutputFrames(UINT32 u32InputFrameCount) override;

    // IAudioSystemEffects2
    STDMETHODIMP GetEffectsList(LPGUID* ppEffectsIds, UINT* pcEffects, HANDLE Event) override;

    // Called by ConfigManager with its mutex held (never on the audio thread).
    void OnConfigChanged(const std::string& configText);
    void ReleaseRetired();
    bool IsLockedForProcess() const { return m_locked; }

private:
    // COM aggregation: the inner unknown owns the reference count and answers
    // QueryInterface; the public IUnknown methods forward to m_pUnkOuter.
    struct InnerUnknown : public IUnknown {
        DivioraAPO* self = nullptr;
        STDMETHODIMP QueryInterface(REFIID riid, void** ppv) override { return self->NonDelegatingQueryInterface(riid, ppv); }
        STDMETHODIMP_(ULONG) AddRef() override { return InterlockedIncrement(&self->m_cRef); }
        STDMETHODIMP_(ULONG) Release() override {
            ULONG r = InterlockedDecrement(&self->m_cRef);
            if (r == 0) delete self;
            return r;
        }
    };
    friend struct InnerUnknown;
    HRESULT NonDelegatingQueryInterface(REFIID riid, void** ppv);

    InnerUnknown m_inner;
    IUnknown* m_pUnkOuter;          // outer object, or &m_inner when not aggregated
    volatile LONG m_cRef;
    bool m_registered;              // registered with ConfigManager
    bool m_locked;                  // guarded by ConfigManager mutex
    bool m_formatIsFloat32;
    bool m_loggedFirstProcess;      // audio thread only
    UINT32 m_sampleRate;
    UINT32 m_channels;
    UINT32 m_bytesPerFrame;         // nBlockAlign of the locked format
    std::wstring m_deviceName;      // endpoint friendly name, for Device: blocks

    // Engine hand-off, lock-free:
    //   control thread -> m_pending (atomic exchange)
    //   audio thread owns m_active; puts the replaced engine in m_retired
    //   control thread frees m_retired.
    std::atomic<DvrEngine*> m_pending;
    DvrEngine*              m_active;
    std::atomic<DvrEngine*> m_retired;

    DvrEngine* Compile(const std::string& configText) const;
    void FreeAllEngines();
};
