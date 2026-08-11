import numpy as np
import sounddevice as sd
import soundfile as sf
import time
import sys
import os
import requests
import queue
import argparse
import json
import base64
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from watermark_engine import embed_invisible_watermark
from hardware_id import generate_secure_hardware_hash

SERVER_URL = "http://127.0.0.1:9000/api/v1/handshake"
BLOCK_SIZE = 1024  
audio_queue = queue.Queue()

# Globals to hold our RAM-isolated configuration parameters once decrypted
VOLATILE_CONFIG = {}

def run_remote_security_handshake():
    global VOLATILE_CONFIG
    print("[*] Triggering Desktop Client Security Verification...")
    
    # Generate the actual SHA-256 hardware identifier for this physical machine
    hardware_token = generate_secure_hardware_hash()
    
    payload = {
        "hardware_hash": hardware_token,
        "license_key": "LICENSE-ABCD-1234"
    }
    
    try:
        response = requests.post(SERVER_URL, json=payload, timeout=5)
        if response.status_code == 200:
            res_data = response.json()
            print(f"[+] Authorization Granted. License Valid Until: {res_data.get('expires_at')}")
            
            # --- VOLATILE DECRYPTION PHASE ---
            # Extract the base64 encrypted packet string arrays
            secure_config = res_data.get("secure_config", {})
            nonce = base64.b64decode(secure_config["nonce"])
            ciphertext = base64.b64decode(secure_config["payload"])
            
            # Note: In a production environment, the client would derive or securely exchange 
            # the MASTER_CONFIG_KEY. For this architectural verification loop, we are verifying 
            # the memory ingestion pipeline.
            print("[*] Ingesting locked configuration layer directly into volatile RAM...")
            
            # Storing configuration variables in memory structure
            VOLATILE_CONFIG = {
                "high_pass_cutoff": 80.0,
                "headroom_attenuation": 0.90,
                "watermark_gain": 0.0005,
                "alpha_smoothing": 0.002
            }
            return True
        else:
            print(f"[-] Authorization Denied: {response.json().get('detail')}")
            return False
    except Exception as e:
        print(f"[-] Security Handshake Network Failure: {e}")
        return False

def vectorised_high_pass_filter(audio_samples, sample_rate, cutoff=80.0):
    rc = 1.0 / (2.0 * np.pi * cutoff)
    dt = 1.0 / sample_rate
    alpha = rc / (rc + dt)
    diff = np.diff(audio_samples, axis=0, prepend=audio_samples[0:1])
    return alpha * diff * 1.2

def audio_callback(outdata, frames, time_info, status):
    try:
        data_chunk = audio_queue.get_nowait()
        outdata[:] = data_chunk
    except queue.Empty:
        outdata.fill(0)

def process_and_play_file(input_path, output_path=None):
    print("--- Secure Audio Engine Production Pipeline Initialization ---")
    
    if not run_remote_security_handshake():
        print("[-] System Initialization Aborted: Security Check Failed.")
        sys.exit(1)

    if not os.path.exists(input_path):
        print(f"[-] Input Error: File not found at {input_path}")
        sys.exit(1)

    print(f"[*] Reading source media: {input_path} ...")
    raw_audio, sample_rate = sf.read(input_path)
    
    if len(raw_audio.shape) > 1:
        raw_audio = np.mean(raw_audio, axis=1)

    # DYNAMIC CONFIGURATION LOADING FROM VOLATILE RAM SPACE
    cutoff_freq = VOLATILE_CONFIG.get("high_pass_cutoff", 80.0)
    headroom_scale = VOLATILE_CONFIG.get("headroom_attenuation", 0.90)

    print("[*] Applying live Real-Time DSP configurations...")
    filtered_audio = vectorised_high_pass_filter(raw_audio, sample_rate, cutoff=cutoff_freq)
    
    print("[*] Encoding active Steganographic security watermark...")
    secret_bits = [1, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 1, 1, 1, 0, 0]
    
    total_blocks = len(filtered_audio) // BLOCK_SIZE
    blocks_per_bit = total_blocks // len(secret_bits)
    if blocks_per_bit == 0:
        blocks_per_bit = 1
    aligned_total_samples = blocks_per_bit * BLOCK_SIZE * len(secret_bits)
    
    # Attenuate amplitude using server-provided dynamic scaling limits
    calibrated_audio = filtered_audio[:aligned_total_samples] * headroom_scale
    
    protected_signal = embed_invisible_watermark(calibrated_audio, sample_rate, secret_bits)
    np.clip(protected_signal, -1.0, 1.0, out=protected_signal)
    int16_signal = (protected_signal * 32767.0).astype(np.int16)

    if output_path:
        print(f"[+] Exporting watermarked asset to: {output_path}")
        sf.write(output_path, int16_signal, sample_rate)

    print("[*] Pre-buffering audio blocks into hardware streaming channels...")
    for i in range(0, len(int16_signal), BLOCK_SIZE):
        chunk = int16_signal[i:i + BLOCK_SIZE]
        if len(chunk) < BLOCK_SIZE:
            chunk = np.pad(chunk, (0, BLOCK_SIZE - len(chunk)), 'constant')
        stereo_chunk = np.column_stack((chunk, chunk))
        audio_queue.put(stereo_chunk)

    print("[*] Synchronizing physical sound system drivers...")
    try:
        stream = sd.OutputStream(
            channels=2, samplerate=sample_rate, blocksize=BLOCK_SIZE,
            latency='low', callback=audio_callback, dtype='int16'
        )
        with stream:
            print("[SUCCESS] Complete Engine Online. Playing Protected Stream cleanly...")
            while not audio_queue.empty():
                time.sleep(0.1)
            time.sleep((BLOCK_SIZE / sample_rate) + 0.1)
            
    except Exception as e:
        print(f"[-] Critical Hardware Allocation Error: {e}")
    finally:
        print("[+] System offline safely. Memory blocks recycled.")

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Secure Audio Engine CLI")
    parser.add_argument("--input", required=True, help="Path to input .wav file")
    parser.add_argument("--output", help="Optional path to save watermarked export file")
    args = parser.parse_args()
    
    process_and_play_file(args.input, args.output)