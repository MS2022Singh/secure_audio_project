#include <windows.h>
#include <mmdeviceapi.h>
#include <audioclient.h>
#include <iostream>
#include <vector>
#include <atomic>
#include <thread>
#include <cstdint>
#include "DivioraAPO.h"

#pragma comment(lib, "ole32.lib")
#pragma comment(lib, "advapi32.lib")

static std::atomic<bool> g_bRunning(true);
static HANDLE g_hSharedStateMap = NULL;
static volatile DivioraSharedState* g_pSharedState = nullptr;

static HANDLE g_hDiagMap = NULL;
static volatile ApoLiveDiagnostics* g_pLiveDiag = nullptr;

struct BiquadStateLocal {
    float s1 = 0.0f;
    float s2 = 0.0f;
};

static BiquadStateLocal g_filterStates[8][kDivioraMaxFilters];
static DivioraPayloadWire g_cachedPayload;
static uint64_t g_lastAppliedRevision = 0;
static float g_gainLinear = 1.0f;

void ResetLocalFilters() {
    for (int ch = 0; ch < 8; ++ch) {
        for (uint32_t f = 0; f < kDivioraMaxFilters; ++f) {
            g_filterStates[ch][f] = BiquadStateLocal{};
        }
    }
}

bool CheckAndUpdateSharedState() {
    if (!g_pSharedState) return false;

    LONG s1 = g_pSharedState->seq_lock;
    if (s1 & 1) return false;

    MemoryBarrier();
    uint64_t currentRev = g_pSharedState->payload.revision;
    MemoryBarrier();

    LONG s2 = g_pSharedState->seq_lock;
    if (s1 != s2 || currentRev == 0) return false;

    if (currentRev != g_lastAppliedRevision) {
        DivioraPayloadWire snap = const_cast<const DivioraPayloadWire&>(g_pSharedState->payload);
        g_cachedPayload = snap;
        ResetLocalFilters();
        g_gainLinear = snap.preamp_linear;
        g_lastAppliedRevision = snap.revision;

        if (g_pLiveDiag) {
            InterlockedExchange64((volatile LONG64*)&g_pLiveDiag->loaded_revision, (LONG64)snap.revision);
        }
        return true;
    }
    return false;
}

void ProcessAudioBuffer(float* pIn, float* pOut, UINT32 frameCount, UINT32 channels) {
    if (!pIn || !pOut || frameCount == 0 || channels == 0) return;

    CheckAndUpdateSharedState();

    if (g_pLiveDiag) {
        InterlockedIncrement64((volatile LONG64*)&g_pLiveDiag->process_calls);
        InterlockedAdd64((volatile LONG64*)&g_pLiveDiag->processed_frames, frameCount);
    }

    const float gain = g_gainLinear;
    const uint32_t filterCount = (g_cachedPayload.filter_count > kDivioraMaxFilters) ? kDivioraMaxFilters : g_cachedPayload.filter_count;
    const UINT32 activeChannels = (channels <= 8) ? channels : 2;

    for (UINT32 frame = 0; frame < frameCount; ++frame) {
        for (UINT32 ch = 0; ch < activeChannels; ++ch) {
            UINT32 idx = frame * channels + ch;
            float x = pIn[idx] * gain;

            for (uint32_t f = 0; f < filterCount; ++f) {
                const BiquadCoeffsWire& c = g_cachedPayload.filters[f];
                BiquadStateLocal& st = g_filterStates[ch][f];
                float y = c.b0 * x + st.s1;
                st.s1 = c.b1 * x - c.a1 * y + st.s2;
                st.s2 = c.b2 * x - c.a2 * y;
                x = y;
            }
            pOut[idx] = x;
        }
    }
}

void AudioBridgeWorker() {
    HRESULT hr = CoInitializeEx(NULL, COINIT_MULTITHREADED);
    if (FAILED(hr)) return;

    IMMDeviceEnumerator* pEnumerator = nullptr;
    hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), NULL, CLSCTX_ALL, __uuidof(IMMDeviceEnumerator), (void**)&pEnumerator);
    if (FAILED(hr) || !pEnumerator) {
        CoUninitialize();
        return;
    }

    // 1. Get Capture Device: VB-Audio Cable Input
    IMMDevice* pCaptureDevice = nullptr;
    LPCWSTR captureId = L"{0.0.0.00000000}.{3d565cc1-abb9-4506-ae0e-f3e66eabc844}";
    hr = pEnumerator->GetDevice(captureId, &pCaptureDevice);
    if (FAILED(hr) || !pCaptureDevice) {
        pEnumerator->GetDefaultAudioEndpoint(eCapture, eConsole, &pCaptureDevice);
    }

    // 2. Get Render Device: Realtek Speakers
    IMMDevice* pRenderDevice = nullptr;
    LPCWSTR renderId = L"{0.0.0.00000000}.{74e3dfaf-ea45-49ef-a4c7-eaef0bbbdc51}";
    hr = pEnumerator->GetDevice(renderId, &pRenderDevice);
    if (FAILED(hr) || !pRenderDevice) {
        pEnumerator->GetDefaultAudioEndpoint(eRender, eConsole, &pRenderDevice);
    }

    if (!pCaptureDevice || !pRenderDevice) {
        pEnumerator->Release();
        CoUninitialize();
        return;
    }

    IAudioClient* pCaptureAudioClient = nullptr;
    hr = pCaptureDevice->Activate(__uuidof(IAudioClient), CLSCTX_ALL, NULL, (void**)&pCaptureAudioClient);
    
    WAVEFORMATEX* pwfx = nullptr;
    hr = pCaptureAudioClient->GetMixFormat(&pwfx);
    
    REFERENCE_TIME hnsBufferDuration = 100000; // 10ms
    hr = pCaptureAudioClient->Initialize(AUDCLNT_SHAREMODE_SHARED, 0, hnsBufferDuration, 0, pwfx, NULL);

    IAudioCaptureClient* pCaptureClient = nullptr;
    hr = pCaptureAudioClient->GetService(__uuidof(IAudioCaptureClient), (void**)&pCaptureClient);

    IAudioClient* pRenderAudioClient = nullptr;
    hr = pRenderDevice->Activate(__uuidof(IAudioClient), CLSCTX_ALL, NULL, (void**)&pRenderAudioClient);
    hr = pRenderAudioClient->Initialize(AUDCLNT_SHAREMODE_SHARED, 0, hnsBufferDuration, 0, pwfx, NULL);

    IAudioRenderClient* pRenderClient = nullptr;
    hr = pRenderAudioClient->GetService(__uuidof(IAudioRenderClient), (void**)&pRenderClient);

    hr = pCaptureAudioClient->Start();
    hr = pRenderAudioClient->Start();

    if (g_pLiveDiag) {
        InterlockedExchange((volatile LONG*)&g_pLiveDiag->instantiated, 1);
        InterlockedExchange((volatile LONG*)&g_pLiveDiag->format_valid, 1);
    }

    UINT32 channels = pwfx->nChannels;
    std::vector<float> processedBuffer;

    while (g_bRunning) {
        Sleep(2);

        UINT32 packetLength = 0;
        hr = pCaptureClient->GetNextPacketSize(&packetLength);
        if (FAILED(hr) || packetLength == 0) continue;

        while (packetLength > 0) {
            BYTE* pData = nullptr;
            UINT32 numFramesAvailable = 0;
            DWORD flags = 0;

            hr = pCaptureClient->GetBuffer(&pData, &numFramesAvailable, &flags, NULL, NULL);
            if (FAILED(hr)) break;

            if (!(flags & AUDCLNT_BUFFERFLAGS_SILENT) && numFramesAvailable > 0) {
                size_t totalSamples = static_cast<size_t>(numFramesAvailable) * channels;
                if (processedBuffer.size() < totalSamples) {
                    processedBuffer.resize(totalSamples);
                }

                float* pFloatIn = reinterpret_cast<float*>(pData);
                ProcessAudioBuffer(pFloatIn, processedBuffer.data(), numFramesAvailable, channels);

                BYTE* pRenderData = nullptr;
                hr = pRenderClient->GetBuffer(numFramesAvailable, &pRenderData);
                if (SUCCEEDED(hr) && pRenderData) {
                    memcpy(pRenderData, processedBuffer.data(), totalSamples * sizeof(float));
                    pRenderClient->ReleaseBuffer(numFramesAvailable, 0);
                }
            }

            pCaptureClient->ReleaseBuffer(numFramesAvailable);
            hr = pCaptureClient->GetNextPacketSize(&packetLength);
            if (FAILED(hr)) break;
        }
    }

    pCaptureAudioClient->Stop();
    pRenderAudioClient->Stop();
    if (pCaptureClient) pCaptureClient->Release();
    if (pRenderClient) pRenderClient->Release();
    if (pCaptureAudioClient) pCaptureAudioClient->Release();
    if (pRenderAudioClient) pRenderAudioClient->Release();
    if (pwfx) CoTaskMemFree(pwfx);
    if (pCaptureDevice) pCaptureDevice->Release();
    if (pRenderDevice) pRenderDevice->Release();
    if (pEnumerator) pEnumerator->Release();
    CoUninitialize();
}

void SetupSharedMemory() {
    SECURITY_DESCRIPTOR sd;
    InitializeSecurityDescriptor(&sd, SECURITY_DESCRIPTOR_REVISION);
    SetSecurityDescriptorDacl(&sd, TRUE, NULL, FALSE);
    SECURITY_ATTRIBUTES sa;
    sa.nLength = sizeof(sa);
    sa.lpSecurityDescriptor = &sd;
    sa.bInheritHandle = FALSE;

    g_hSharedStateMap = CreateFileMappingW(
        INVALID_HANDLE_VALUE, &sa, PAGE_READWRITE, 0, sizeof(DivioraSharedState), kDivioraSharedStateName
    );
    if (g_hSharedStateMap) {
        g_pSharedState = (volatile DivioraSharedState*)MapViewOfFile(g_hSharedStateMap, FILE_MAP_ALL_ACCESS, 0, 0, sizeof(DivioraSharedState));
        if (g_pSharedState) {
            ZeroMemory((void*)g_pSharedState, sizeof(DivioraSharedState));
            g_pSharedState->payload.preamp_linear = 1.0f;
        }
    }

    g_hDiagMap = CreateFileMappingW(
        INVALID_HANDLE_VALUE, &sa, PAGE_READWRITE, 0, sizeof(ApoLiveDiagnostics), L"Global\\DivioraApoDiagnostics"
    );
    if (g_hDiagMap) {
        g_pLiveDiag = (volatile ApoLiveDiagnostics*)MapViewOfFile(g_hDiagMap, FILE_MAP_ALL_ACCESS, 0, 0, sizeof(ApoLiveDiagnostics));
        if (g_pLiveDiag) {
            ZeroMemory((void*)g_pLiveDiag, sizeof(ApoLiveDiagnostics));
            g_pLiveDiag->magic = 0x4449564F;
            g_pLiveDiag->version = 1;
        }
    }
}

int main() {
    std::cout << "=== Diviora In-Line Audio Bridge Service ===" << std::endl;

    SetupSharedMemory();
    ResetLocalFilters();

    std::thread bridgeThread(AudioBridgeWorker);

    while (g_bRunning) {
        CheckAndUpdateSharedState();
        Sleep(50);
    }

    if (bridgeThread.joinable()) bridgeThread.join();

    if (g_pLiveDiag) UnmapViewOfFile((void*)g_pLiveDiag);
    if (g_hDiagMap) CloseHandle(g_hDiagMap);
    if (g_pSharedState) UnmapViewOfFile((void*)g_pSharedState);
    if (g_hSharedStateMap) CloseHandle(g_hSharedStateMap);

    return 0;
}