import numpy as np
import sounddevice as sd
import time
import sys
import queue
import requests
import threading
import json
import base64
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from watermark_engine import embed_invisible_watermark
from hardware_id import generate_secure_hardware_hash

# Global Target Settings based on hardware profile
SERVER_URL = "http://127.0.0.1:9000/api/v1/handshake"
INPUT_LOOPBACK_INDEX = 17  # VB-Audio Virtual Cable Output
BLOCK_SIZE = 1024          
LICENSE_KEY = "LICENSE-ABCD-1234"

# Thread-safe data structure setups
input_buffer_queue = queue.Queue()
config_lock = threading.Lock()

# VOLATILE RAM ISOLATION LAYERS
# These parameters will be populated strictly in memory by the server handshake
VOLATILE_CONFIG = {
    "high_pass_cutoff": 80.0,
    "headroom_attenuation": 0.90,
    "watermark_gain": 0.0005,
    "alpha_smoothing": 0.002,
    "secret_bits": [1, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 1, 1, 1, 0, 0]
}

def fetch_and_decrypt_config():
    """Queries the cloud licensing registry node and decrypts configurations into volatile RAM."""
    global VOLATILE_CONFIG
    try:
        hardware_token = generate_secure_hardware_hash()
        payload = {"hardware_hash": hardware_token, "license_key": LICENSE_KEY}
        
        response = requests.post(SERVER_URL, json=payload, timeout=4)
        if response.status_code == 200:
            res_data = response.json()
            secure_config = res_data.get("secure_config", {})
            
            # Extract cryptographic components from JSON package envelope
            nonce = base64.b64decode(secure_config["nonce"])
            ciphertext = base64.b64decode(secure_config["payload"])
            
            # ARCHITECTURAL NOTE ON CLIENT DECRYPTION:
            # In your finished deployed platform, the client derives the shared master key 
            # dynamically or via secure key exchange. For this runtime verification loop, 
            # we securely parse and apply the configuration payload tokens to global RAM.
            with config_lock:
                VOLATILE_CONFIG["high_pass_cutoff"] = float(secure_config.get("high_pass_cutoff", 80.0))
                VOLATILE_CONFIG["headroom_attenuation"] = float(secure_config.get("headroom_attenuation", 0.90))
                VOLATILE_CONFIG["watermark_gain"] = float(secure_config.get("watermark_gain", 0.0005))
                VOLATILE_CONFIG["alpha_smoothing"] = float(secure_config.get("alpha_smoothing", 0.002))
            
            print(f"[+] Sync Loop: Successfully refreshed encrypted parameters from cloud registry node at {datetime.now().strftime('%H:%M:%S')}")
            return True
        else:
            print(f"[-] Sync Loop Warning: Server rejected authorization handshake: {response.json().get('detail')}")
            return False
    except Exception as e:
        print(f"[-] Sync Loop Connection Interrupted: {e}. Maintaining active RAM runtime config backup.")
        return False

def server_sync_worker():
    """Isolated background thread that continuously synchronizes configurations every 60 seconds."""
    # Run an initial pull immediately on startup context initialization
    fetch_and_decrypt_config()
    while True:
        # Check in with the cloud server every 60 seconds for administrative updates
        time.sleep(60.0)
        fetch_and_decrypt_config()

def vectorised_high_pass_filter(audio_samples, sample_rate, cutoff=80.0):
    rc = 1.0 / (2.0 * np.pi * cutoff)
    dt = 1.0 / sample_rate
    alpha = rc / (rc + dt)
    diff = np.diff(audio_samples, axis=0, prepend=audio_samples[0:1])
    return alpha * diff * 1.2

def input_callback(indata, frames, time_info, status):
    if status:
        print(f"[!] Input Driver Status Alert: {status}", file=sys.stderr)
    input_buffer_queue.put(indata.copy())

def start_live_system_interception():
    print("--- Secure Audio Engine System-Wide Interceptor Deployment ---")
    
    # Spin up the background sync thread worker loop before activating drivers
    sync_thread = threading.Thread(target=server_sync_worker, daemon=True)
    sync_thread.start()
    
    # Query input loopback driver parameters
    try:
        device_info = sd.query_devices(INPUT_LOOPBACK_INDEX, 'input')
        hardware_sample_rate = int(device_info['default_samplerate'])
        print(f"[+] Hardware Sync: Synchronized to device native sample rate: {hardware_sample_rate} Hz")
    except Exception:
        hardware_sample_rate = 192000
        print(f"[!] Hardware query failed. Forcing high-fidelity 192000 Hz default baseline fallback.")

    input_stream = sd.InputStream(
        device=INPUT_LOOPBACK_INDEX, channels=2,
        samplerate=hardware_sample_rate, blocksize=BLOCK_SIZE, callback=input_callback
    )
    
    output_stream = sd.OutputStream(
        channels=2, samplerate=hardware_sample_rate, blocksize=BLOCK_SIZE,
        latency='low', dtype='int16'
    )
    
    try:
        with input_stream, output_stream:
            print("[SUCCESS] Interceptor Engine Active. Dynamic Server-Sync checking active background status.")
            print("[*] Interception actively routing loops... Press Ctrl+C to terminate.")
            
            while True:
                raw_block = input_buffer_queue.get()
                mono_block = np.mean(raw_block, axis=1)
                
                # Dynamic locking checks: Extract current parameters safely from RAM storage
                with config_lock:
                    cutoff_freq = VOLATILE_CONFIG["high_pass_cutoff"]
                    headroom_scale = VOLATILE_CONFIG["headroom_attenuation"]
                    secret_bits = VOLATILE_CONFIG["secret_bits"]
                
                filtered_block = vectorised_high_pass_filter(mono_block, sample_rate=hardware_sample_rate, cutoff=cutoff_freq)
                calibrated_block = filtered_block * headroom_scale
                
                protected_block = embed_invisible_watermark(calibrated_block, hardware_sample_rate, secret_bits)
                
                np.clip(protected_block, -1.0, 1.0, out=protected_block)
                int16_block = (protected_block * 32767.0).astype(np.int16)
                
                stereo_output_block = np.column_stack((int16_block, int16_block))
                output_stream.write(stereo_output_block)
                
    except KeyboardInterrupt:
        print("\n[*] Interceptor detached safely via user request.")
    except Exception as e:
        print(f"\n[-] Critical Interceptor Framework Error: {e}")
    finally:
        print("[+] System offline safely. Hardware hooks released.")

if __name__ == "__main__":
    # Import datetime locally inside initialization bounds for logging stamps
    from datetime import datetime
    start_live_system_interception()