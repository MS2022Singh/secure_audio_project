#include <windows.h>
#include <unknwn.h>
#include <new>
#include "ClassFactory.h"

// Forward declaration from ClassFactory.cpp / DivioraAPO
extern const CLSID CLSID_DivioraEndpointAPO;

_Use_decl_annotations_
STDAPI DllGetClassObject(REFCLSID rclsid, REFIID riid, void** ppv) {
    if (!ppv) return E_POINTER;
    *ppv = nullptr;

    const CLSID CLSID_ExpectedDiviora = {0xD1B100A1, 0x8888, 0x4444, {0x99, 0x99, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01}};

    if (!IsEqualCLSID(rclsid, CLSID_ExpectedDiviora) && rclsid != CLSID_DivioraEndpointAPO) {
        return CLASS_E_CLASSNOTAVAILABLE;
    }

    CClassFactory* pFactory = new (std::nothrow) CClassFactory();
    if (!pFactory) return E_OUTOFMEMORY;

    HRESULT hr = pFactory->QueryInterface(riid, ppv);
    pFactory->Release();
    return hr;
}

_Use_decl_annotations_
STDAPI DllCanUnloadNow() {
    return S_FALSE;
}

BOOL APIENTRY DllMain(HMODULE hModule, DWORD ul_reason_for_call, LPVOID lpReserved) {
    switch (ul_reason_for_call) {
    case DLL_PROCESS_ATTACH:
    case DLL_THREAD_ATTACH:
    case DLL_THREAD_DETACH:
    case DLL_PROCESS_DETACH:
        break;
    }
    return TRUE;
}