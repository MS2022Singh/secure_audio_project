#include <windows.h>
#include <mmdeviceapi.h>
#include <functiondiscoverykeys_devpkey.h>
#include <iostream>
#include <string>
#include <vector>

int main() {
    HRESULT hr = CoInitialize(NULL);
    if (FAILED(hr)) return 1;

    IMMDeviceEnumerator* pEnum = NULL;
    hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), NULL, CLSCTX_ALL, __uuidof(IMMDeviceEnumerator), (void**)&pEnum);
    if (FAILED(hr) || !pEnum) {
        CoUninitialize();
        return 1;
    }

    // 1. Identify Default Console Endpoint
    IMMDevice* pDefaultDevice = NULL;
    std::wstring defaultId = L"";
    if (SUCCEEDED(pEnum->GetDefaultAudioEndpoint(eRender, eConsole, &pDefaultDevice)) && pDefaultDevice) {
        LPWSTR pwszDefId = NULL;
        if (SUCCEEDED(pDefaultDevice->GetId(&pwszDefId)) && pwszDefId) {
            defaultId = pwszDefId;
            CoTaskMemFree(pwszDefId);
        }
        pDefaultDevice->Release();
    }

    // 2. Enumerate All Active Render Endpoints
    IMMDeviceCollection* pCollection = NULL;
    hr = pEnum->EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE, &pCollection);
    if (SUCCEEDED(hr) && pCollection) {
        UINT count = 0;
        pCollection->GetCount(&count);

        for (UINT i = 0; i < count; ++i) {
            IMMDevice* pDevice = NULL;
            if (SUCCEEDED(pCollection->Item(i, &pDevice)) && pDevice) {
                LPWSTR pwszId = NULL;
                pDevice->GetId(&pwszId);

                IPropertyStore* pStore = NULL;
                std::wstring friendlyName = L"Audio Device";
                if (SUCCEEDED(pDevice->OpenPropertyStore(STGM_READ, &pStore)) && pStore) {
                    PROPVARIANT varName;
                    PropVariantInit(&varName);
                    if (SUCCEEDED(pStore->GetValue(PKEY_Device_FriendlyName, &varName)) && varName.pwszVal) {
                        friendlyName = varName.pwszVal;
                    }
                    PropVariantClear(&varName);
                    pStore->Release();
                }

                std::wstring fullId = pwszId ? pwszId : L"";
                size_t lastOpen = fullId.rfind(L"{");
                size_t lastClose = fullId.rfind(L"}");
                std::wstring guidStr = L"";
                if (lastOpen != std::wstring::npos && lastClose != std::wstring::npos && lastClose > lastOpen) {
                    guidStr = fullId.substr(lastOpen, lastClose - lastOpen + 1);
                }

                bool isDefault = (!defaultId.empty() && fullId == defaultId);

                // Print machine-readable lines for Rust parser
                std::wcout << L"DEVICE_ENTRY|" << friendlyName << L"|" << guidStr << L"|" << (isDefault ? L"DEFAULT" : L"ACTIVE") << std::endl;

                if (pwszId) CoTaskMemFree(pwszId);
                pDevice->Release();
            }
        }
        pCollection->Release();
    }

    pEnum->Release();
    CoUninitialize();
    return 0;
}