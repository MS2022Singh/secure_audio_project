#include "DivioraAPO.h"
#include <new>

class DivioraClassFactory : public IClassFactory {
public:
    DivioraClassFactory() : m_cRef(1) {}
    virtual ~DivioraClassFactory() {}

    STDMETHODIMP QueryInterface(REFIID riid, void** ppvObject) override {
        if (!ppvObject) return E_POINTER;
        if (riid == IID_IUnknown || riid == IID_IClassFactory) {
            *ppvObject = static_cast<IClassFactory*>(this);
            AddRef();
            return S_OK;
        }
        *ppvObject = nullptr;
        return E_NOINTERFACE;
    }

    STDMETHODIMP_(ULONG) AddRef() override { return InterlockedIncrement(&m_cRef); }
    STDMETHODIMP_(ULONG) Release() override {
        ULONG ulRef = InterlockedDecrement(&m_cRef);
        if (ulRef == 0) delete this;
        return ulRef;
    }

    STDMETHODIMP CreateInstance(IUnknown* pUnkOuter, REFIID riid, void** ppvObject) override {
        if (pUnkOuter) return CLASS_E_NOAGGREGATION;
        if (!ppvObject) return E_POINTER;

        DivioraAPO* pApo = new (std::nothrow) DivioraAPO();
        if (!pApo) return E_OUTOFMEMORY;

        HRESULT hr = pApo->QueryInterface(riid, ppvObject);
        pApo->Release();
        return hr;
    }

    STDMETHODIMP LockServer(BOOL fLock) override {
        UNREFERENCED_PARAMETER(fLock);
        return S_OK;
    }

private:
    volatile LONG m_cRef;
};

STDAPI DllGetClassObject(REFCLSID rclsid, REFIID riid, LPVOID* ppv) {
    if (!ppv) return E_POINTER;
    if (rclsid != CLSID_DivioraAPO) return CLASS_E_CLASSNOTAVAILABLE;

    DivioraClassFactory* pFactory = new (std::nothrow) DivioraClassFactory();
    if (!pFactory) return E_OUTOFMEMORY;

    HRESULT hr = pFactory->QueryInterface(riid, ppv);
    pFactory->Release();
    return hr;
}

STDAPI DllCanUnloadNow(void) { return S_OK; }