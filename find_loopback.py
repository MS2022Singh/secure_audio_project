import sounddevice as sd

def scan_audio_devices():
    print("--- Windows WASAPI Loopback Device Mapping Tool ---")
    devices = sd.query_devices()
    wasapi_loopback_idx = None

    for idx, device in enumerate(devices):
        name = device['name']
        host_api = sd.query_hostapis(device['hostapi'])['name']
        
        # Look specifically for WASAPI loopback input sub-devices
        if "WASAPI" in host_api and device['max_input_channels'] > 0:
            print(f"[FOUND] Index {idx}: {name} (API: {host_api}) | Input Channels: {device['max_input_channels']}")
            # Capture the default speakers loopback if found
            if "Speakers" in name or "Default" in name or "Loopback" in name:
                if wasapi_loopback_idx is None:
                    wasapi_loopback_idx = idx
    
    if wasapi_loopback_idx is not None:
        print(f"\n[+] Recommended Loopback Target Index: {wasapi_loopback_idx}")
    else:
        print("\n[-] Error: No WASAPI loopback input device detected. Ensure Windows WASAPI host api is supported.")

if __name__ == "__main__":
    scan_audio_devices()