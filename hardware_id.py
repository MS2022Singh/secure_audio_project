import subprocess
import hashlib
import sys

def get_wmi_property(command, property_name):
    """Executes a low-level system shell query to extract immutable hardware identifiers."""
    try:
        # Use a localized shell query to pull hardware components directly from the motherboard firmware
        result = subprocess.run(
            ["powershell", "-Command", command],
            capture_output=True,
            text=True,
            check=True
        )
        # Parse the output line safely and clean whitespace text arrays
        lines = [line.strip() for line in result.stdout.split('\n') if line.strip()]
        if len(lines) > 1:
            return lines[1]  # The first line is the header, second line is the raw data value
        return "UNKNOWN"
    except Exception:
        return "FAILED_QUERY"

def generate_secure_hardware_hash():
    """Queries multiple hardware layers and compiles a secure 64-character SHA-256 fingerprint."""
    print("[*] Probing underlying physical hardware layers...")
    
    # 1. Fetch unique physical motherboard serial
    motherboard = get_wmi_property("wmic baseboard get serialnumber", "SerialNumber")
    
    # 2. Fetch raw processor architecture ID asset string
    cpu_id = get_wmi_property("wmic cpu get processorid", "ProcessorId")
    
    # 3. Fetch permanent system BIOS UUID token
    bios_uuid = get_wmi_property("wmic csproduct get uuid", "UUID")
    
    # Fallback checking: Prevent generation if system queries are entirely blocked
    if motherboard == "UNKNOWN" and cpu_id == "UNKNOWN" and bios_uuid == "UNKNOWN":
        print("[-] Critical Error: System failed to securely resolve local device traits.")
        sys.exit(1)
        
    # Concatenate fields cleanly to create the raw machine signature seed string
    raw_hardware_string = f"MB:{motherboard}|CPU:{cpu_id}|BIOS:{bios_uuid}"
    
    # Run the raw sequence through a secure SHA-256 hashing envelope
    sha256 = hashlib.sha256()
    sha256.update(raw_hardware_string.encode('utf-8'))
    hardware_fingerprint = sha256.hexdigest()
    
    print(f"[+] Device Traits Gathered Successfully.")
    return hardware_fingerprint

if __name__ == "__main__":
    print("--- Secure Hardware Binding Diagnostic Initialization ---")
    device_token = generate_secure_hardware_hash()
    print(f"[STATUS] Final Hashed Device ID Token: {device_token}")