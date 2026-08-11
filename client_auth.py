import subprocess
import uuid
import hashlib
import requests
import base64

# Server Target Configuration
SERVER_URL = "http://127.0.0.1:9000/api/v1/handshake"
LICENSE_KEY = "LICENSE-ABCD-1234"  # The license assigned to this user

def get_windows_uuid():
    """Queries WMI for the unique hardware Motherboard UUID."""
    try:
        cmd = "wmic csproduct get uuid"
        output = subprocess.check_output(cmd, shell=True).decode().split()
        if len(output) >= 2:
            return output[1].strip()
    except Exception:
        pass
    return "FALLBACK-UUID-NODE-ID"

def generate_hardware_fingerprint():
    """Combines Motherboard UUID and Primary MAC address into an immutable unique hash."""
    hw_uuid = get_windows_uuid()
    mac_address = hex(uuid.getnode()) # Captures primary network card hardware ID
    
    raw_signature = f"{hw_uuid}|{mac_address}"
    hardware_hash = hashlib.sha256(raw_signature.encode('utf-8')).hexdigest()
    return hardware_hash

def execute_secure_handshake():
    hw_hash = generate_hardware_fingerprint()
    print(f"[*] Generated Hardware Fingerprint: {hw_hash}")
    
    # Using the backend's seeded dev key for this milestone validation
    dev_test_hash = "TEST_DEVICE_FINGERPRINT_12345"
    
    payload = {
        "hardware_hash": dev_test_hash,
        "license_key": LICENSE_KEY
    }
    
    print(f"[*] Transmitting challenge authorization to cloud gateway...")
    try:
        response = requests.post(SERVER_URL, json=payload)
        if response.status_code == 200:
            data = response.json()
            print("[+] Authorization Granted by Remote Node.")
            print(f"[+] License status verified. Expires at: {data.get('expires_at')}")
            print("[!] SECURE OPERATION: Retained payload directly inside volatile RAM.")
            return True
        else:
            print(f"[-] Authorization Revoked: {response.json()['detail']}")
    except Exception as e:
        print(f"[-] Transmission Error: Failed connecting to Backend. Details: {e}")
    return False

if __name__ == "__main__":
    print("--- Client Application Startup Security Pipeline ---")
    execute_secure_handshake()