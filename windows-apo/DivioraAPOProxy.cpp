#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <audioenginebaseapo.h>
#include <unknwn.h>
#include <mmdeviceapi.h>
#include <iostream>
#include "DivioraAPO.h"

class CDivioraAPO : public IAudioProcessingObject,
                    public IAudioProcessingObjectRT,
                    public IAudioSystemEffects2 {
private:
    LONG m_cRef;
    IAudioProcessingObject* m_pRealtekChild;

public:
    CDivioraAPO() : m_cRef(1), m_pRealtekChild(nullptr) {}
    virtual ~CDivioraAPO() {
        if (m_pRealtekChild) m_pRealtekChild->Release();
    }

    STDMETHODIMP QueryInterface(REFIID riid, void** ppv) {
        if (!ppv) return E_POINTER;
        *ppv = nullptr;
        if (riid == IID_IUnknown || riid == __uuidof(IAudioProcessingObject)) {
            *ppv = static_cast<IAudioProcessingObject*>(this);
        } else if (riid == __uuidof(IAudioProcessingObjectRT)) {
            *ppv = static_cast<IAudioProcessingObjectRT*>(this);
        } else if (riid == __uuidof(IAudioSystemEffects) || riid == __uuidof(IAudioSystemEffects2)) {
            *ppv = static_cast<IAudioSystemEffects2*>(this);
        } else {
            return E_NOINTERFACE;
        }
        AddRef();
        return S_OK;
    }

    STDMETHODIMP_(ULONG) AddRef() { return InterlockedIncrement(&m_cRef); }
    STDMETHODIMP_(ULONG) Release() {
        ULONG ulRef = InterlockedDecrement(&m_cRef);
        if (ulRef == 0) delete this;
        return ulRef;
    }

    STDMETHODIMP Initialize(UINT32 cbDataSize, BYTE* pbyData) {
        CLSID childClsid = {0xC18E2F7E, 0x933D, 0x4965, {0xB7, 0xD1, 0x1E, 0xEF, 0x22, 0x8D, 0x2A, 0xF3}};
        CoCreateInstance(childClsid, nullptr, CLSCTX_INPROC_SERVER, __uuidof(IAudioProcessingObject), (void**)&m_pRealtekChild);
        if (m_pRealtekChild) {
            m_pRealtekChild->Initialize(cbDataSize, pbyData);
        }
        return S_OK;
    }

    STDMETHODIMP IsInputFormatSupported(IAudioMediaType* pOppositeFormat, IAudioMediaType* pRequestedFormat, IAudioMediaType** ppSupportedFormat) {
        if (m_pRealtekChild) return m_pRealtekChild->IsInputFormatSupported(pOppositeFormat, pRequestedFormat, ppSupportedFormat);
        if (!pRequestedFormat) return E_POINTER;
        if (ppSupportedFormat) *ppSupportedFormat = nullptr;
        return S_OK;
    }

    STDMETHODIMP IsOutputFormatSupported(IAudioMediaType* pOppositeFormat, IAudioMediaType* pRequestedFormat, IAudioMediaType** ppSupportedFormat) {
        if (m_pRealtekChild) {
            auto pChild = dynamic_cast<IAudioProcessingObject*>(m_pRealtekChild);
            if (pChild) return pChild->IsOutputFormatSupported(pOppositeFormat, pRequestedFormat, ppSupportedFormat);
        }
        if (!pRequestedFormat) return E_POINTER;
        if (ppSupportedFormat) *ppSupportedFormat = nullptr;
        return S_OK;
    }

    STDMETHODIMP GetRegistrationProperties(APO_REG_PROPERTIES** ppProps) {
        if (!ppProps) return E_POINTER;
        *ppProps = (APO_REG_PROPERTIES*)CoTaskMemAlloc(sizeof(APO_REG_PROPERTIES));
        if (!*ppProps) return E_OUTOFMEMORY;
        
        (*ppProps)->clsid = CLSID_DivioraAPO;
        (*ppProps)->Flags = APO_FLAG_DEFAULT;
        wcscpy_s((*ppProps)->szFriendlyName, 256, L"Diviora Endpoint APO");
        wcscpy_s((*ppProps)->szCopyrightInfo, 256, L"Diviora DSP");
        (*ppProps)->u32MajorVersion = 1;
        (*ppProps)->u32MinorVersion = 0;
        return S_OK;
    }

    STDMETHODIMP Reset() {
        if (m_pRealtekChild) m_pRealtekChild->Reset();
        return S_OK;
    }

    STDMETHODIMP GetLatency(HNSTIME* pTime) {
        if (m_pRealtekChild) return m_pRealtekChild->GetLatency(pTime);
        if (pTime) *pTime = 0;
        return S_OK;
    }

    STDMETHODIMP GetInputChannelCount(UINT32* pcChannels) {
        if (m_pRealtekChild) return m_pRealtekChild->GetInputChannelCount(pcChannels);
        if (pcChannels) *pcChannels = 2;
        return S_OK;
    }

    // Exact Windows SDK APOProcess Signature Match
    STDMETHODIMP_(void) APOProcess(
        UINT32 u32NumInputConnections, 
        APO_CONNECTION_PROPERTY** ppInputConnections, 
        UINT32 u32NumOutputConnections, 
        APO_CONNECTION_PROPERTY** ppOutputConnections
    ) {
        if (!ppInputConnections || !ppOutputConnections || u32NumInputConnections == 0 || u32NumOutputConnections == 0) return;

        APO_CONNECTION_PROPERTY* pIn = ppInputConnections[0];
        APO_CONNECTION_PROPERTY* pOut = ppOutputConnections[0];

        if (pIn && pOut && pIn->pBuffer && pOut->pBuffer) {
            UINT32 frameCount = pIn->u32ValidFrameCount;
            float* pSrc = (float*)pIn->pBuffer;
            float* pDst = (float*)pOut->pBuffer;
            memcpy(pDst, pSrc, frameCount * 2 * sizeof(float));
        }

        if (m_pRealtekChild) {
            auto pRT = dynamic_cast<IAudioProcessingObjectRT*>(m_pRealtekChild);
            if (pRT) {
                pRT->APOProcess(u32NumInputConnections, ppInputConnections, u32NumOutputConnections, ppOutputConnections);
            }
        }
    }

    STDMETHODIMP_(UINT32) CalcInputFrameCount(UINT32 u32OutputFrameCount) { return u32OutputFrameCount; }
    STDMETHODIMP_(UINT32) CalcOutputFrames(UINT32 u32InputFrameCount) { return u32InputFrameCount; }
    STDMETHODIMP_(UINT32) CalcInputFrames(UINT32 u32OutputFrameCount) { return u32OutputFrameCount; }

    STDMETHODIMP GetEffectsList(LPGUID* ppEffectsIds, UINT* pcEffects, HANDLE hEvent) {
        if (!ppEffectsIds || !pcEffects) return E_POINTER;
        *pcEffects = 1;
        *ppEffectsIds = (LPGUID)CoTaskMemAlloc(sizeof(GUID));
        if (!*ppEffectsIds) return E_OUTOFMEMORY;
        (*ppEffectsIds)[0] = CLSID_DivioraAPO;
        return S_OK;
    }
};

class CDivioraClassFactory : public IClassFactory {
private:
    LONG m_cRef;
public:
    CDivioraClassFactory() : m_cRef(1) {}
    ~CDivioraClassFactory() {}

    STDMETHODIMP QueryInterface(REFIID riid, void** ppv) {
        if (!ppv) return E_POINTER;
        *ppv = nullptr;
        if (riid == IID_IUnknown || riid == IID_IClassFactory) {
            *ppv = static_cast<IClassFactory*>(this);
            AddRef();
            return S_OK;
        }
        return E_NOINTERFACE;
    }

    STDMETHODIMP_(ULONG) AddRef() { return InterlockedIncrement(&m_cRef); }
    STDMETHODIMP_(ULONG) Release() {
        ULONG ulRef = InterlockedDecrement(&m_cRef);
        if (ulRef == 0) delete this;
        return ulRef;
    }

    STDMETHODIMP CreateInstance(IUnknown* pUnkOuter, REFIID riid, void** ppv) {
        if (pUnkOuter) return CLASS_E_NOAGGREGATION;
        CDivioraAPO* pObj = new CDivioraAPO();
        if (!pObj) return E_OUTOFMEMORY;
        HRESULT hr = pObj->QueryInterface(riid, ppv);
        pObj->Release();
        return hr;
    }

    STDMETHODIMP LockServer(BOOL bLock) { return S_OK; }
};

STDAPI DllGetClassObject(REFCLSID rclsid, REFIID riid, LPVOID* ppv) {
    if (rclsid != CLSID_DivioraAPO) return CLASS_E_CLASSNOTAVAILABLE;
    CDivioraClassFactory* pFactory = new CDivioraClassFactory();
    if (!pFactory) return E_OUTOFMEMORY;
    HRESULT hr = pFactory->QueryInterface(riid, ppv);
    pFactory->Release();
    return hr;
}

STDAPI DllCanUnloadNow(void) { return S_OK; }
STDAPI DllRegisterServer(void) { return S_OK; }
STDAPI DllUnregisterServer(void) { return S_OK; }

BOOL WINAPI DllMain(HINSTANCE hInstance, DWORD dwReason, LPVOID lpReserved) { return TRUE; }