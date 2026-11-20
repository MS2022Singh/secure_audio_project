#pragma once
#include <windows.h>
#include <unknwn.h>

class CClassFactory : public IClassFactory {
private:
    LONG m_cRef;
public:
    CClassFactory();
    virtual ~CClassFactory();

    // IUnknown
    STDMETHODIMP QueryInterface(REFIID riid, void** ppv);
    STDMETHODIMP_(ULONG) AddRef();
    STDMETHODIMP_(ULONG) Release();

    // IClassFactory
    STDMETHODIMP CreateInstance(IUnknown* pUnkOuter, REFIID riid, void** ppv);
    STDMETHODIMP LockServer(BOOL bLock);
};