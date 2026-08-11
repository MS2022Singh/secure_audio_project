import os
import sqlite3
import json
import base64
from datetime import datetime
from fastapi import FastAPI, HTTPException, status
from pydantic import BaseModel
import uvicorn
from cryptography.hazmat.primitives.ciphers.aead import AESGCM

app = FastAPI(title="Secure Audio Engine - Cloud Licensing API", version="2.0.0")
DB_FILE = "licensing_registry.db"

# Master Key for Configuration Encryption (Maintained in volatile server RAM memory)
MASTER_CONFIG_KEY = AESGCM.generate_key(bit_length=256)

def init_db():
    conn = sqlite3.connect(DB_FILE)
    cursor = conn.cursor()
    # Drops old table to apply new schema structure safely during development
    cursor.execute("DROP TABLE IF EXISTS devices")
    cursor.execute("""
        CREATE TABLE IF NOT EXISTS devices (
            hardware_hash TEXT PRIMARY KEY,
            license_key TEXT NOT NULL,
            is_active INTEGER DEFAULT 1,
            registered_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            expires_at TIMESTAMP NOT NULL
        )
    """)
    
    # Seed a Test License that binds permanently to the first real machine that checks in
    cursor.execute(
        "INSERT OR IGNORE INTO devices (hardware_hash, license_key, is_active, expires_at) VALUES (?, ?, ?, ?)",
        ("TEST_DEVICE_FINGERPRINT_12345", "LICENSE-ABCD-1234", 1, "2030-12-31 23:59:59")
    )
    
    # Seed an Expired Device profile to verify security policy validation paths
    cursor.execute(
        "INSERT OR IGNORE INTO devices (hardware_hash, license_key, is_active, expires_at) VALUES (?, ?, ?, ?)",
        ("EXPIRED_DEVICE_FINGERPRINT", "LICENSE-EXPIRED-9999", 1, "2025-01-01 00:00:00")
    )
    
    conn.commit()
    conn.close()

init_db()

class HandshakeRequest(BaseModel):
    hardware_hash: str
    license_key: str

@app.get("/")
def read_root():
    return {"status": "online", "engine": "Secure Audio DSP Backend Layer"}

@app.post("/api/v1/handshake")
def device_handshake(payload: HandshakeRequest):
    conn = sqlite3.connect(DB_FILE)
    cursor = conn.cursor()
    
    # 1. Look up any existing hardware registration records associated with this specific license key
    cursor.execute(
        "SELECT hardware_hash, is_active, expires_at FROM devices WHERE license_key = ?",
        (payload.license_key,)
    )
    records = cursor.fetchall()
    
    if not records:
        conn.close()
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED, 
            detail="Access Denied: Unrecognized license configuration key."
        )
        
    # Check if this exact hardware footprint is already registered to the license key
    matching_record = next((r for r in records if r[0] == payload.hardware_hash), None)
    
    # 2. "ONE DEVICE, ONE ID" BINDING ENFORCEMENT ENGINE
    if not matching_record:
        # Check if the license key is currently sitting on an unassigned default testing placeholder token
        placeholder_record = next((r for r in records if r[0] == "TEST_DEVICE_FINGERPRINT_12345"), None)
        
        if placeholder_record:
            # DYNAMIC AUTO-BINDING: Permanently update the placeholder tracking record to the client's actual signature
            is_active, expires_at_str = placeholder_record[1], placeholder_record[2]
            cursor.execute(
                "UPDATE devices SET hardware_hash = ? WHERE license_key = ? AND hardware_hash = ?",
                (payload.hardware_hash, payload.license_key, "TEST_DEVICE_FINGERPRINT_12345")
            )
            conn.commit()
            print(f"[+]: Secure Binding Established. License {payload.license_key} locked to Device {payload.hardware_hash}.")
        else:
            # SECURITY BREACH: License key is already mapped to a different physical hardware hash signature
            conn.close()
            print(f"[CRITICAL SECURITY ALERT]: Hardware fingerprint mismatch detected for license {payload.license_key}!")
            raise HTTPException(
                status_code=status.HTTP_403_FORBIDDEN,
                detail="Access Denied: Hardware binding violation. One Device, One ID restriction active."
            )
    else:
        # A valid historical registration entry match was located within the SQLite matrix
        is_active, expires_at_str = matching_record[1], matching_record[2]
        print(f"[+]: Device verified successfully against registered license keys.")

    conn.close()
    
    # 3. REVOCATION & EXPIRATION POLICY CHECK
    if is_active == 0:
        raise HTTPException(status_code=403, detail="License has been manually revoked.")

    expires_at = datetime.strptime(expires_at_str, "%Y-%m-%d %H:%M:%S")
    if datetime.now() > expires_at:
        raise HTTPException(status_code=403, detail=f"Your access token expired on {expires_at_str}.")

    # 4. VOLATILE MEMORY ISO-ENCAPSULATED APO CONFIGURATION PAYLOAD
    dsp_config_payload = {
        "eq_bands": [0.5, -1.2, 3.4, 0.0, -0.5],
        "high_pass_cutoff": 80.0,
        "headroom_attenuation": 0.90,
        "watermark_gain": 0.0005,
        "alpha_smoothing": 0.002,
        "stego_watermark_threshold": -45,
        "watermark_salt": "SECURE_CRYPTO_SALT_9988"
    }
    
    # Encrypt raw configurations inside an isolated ephemeral memory container
    aesgcm = AESGCM(MASTER_CONFIG_KEY)
    nonce = os.urandom(12) 
    serialized_data = json.dumps(dsp_config_payload).encode('utf-8')
    encrypted_bytes = aesgcm.encrypt(nonce, serialized_data, None)
    
    secure_transmission = {
        "nonce": base64.b64encode(nonce).decode('utf-8'),
        "payload": base64.b64encode(encrypted_bytes).decode('utf-8')
    }
    
    print(f"[SUCCESS]: Handshake complete. Secure encrypted data stream dispatched.")
    return {
        "status": "authorized",
        "expires_at": expires_at_str,
        "secure_config": secure_transmission
    }

if __name__ == "__main__":
    # Fixed execution target assignment parameters to run via app instance directly
    uvicorn.run(app, host="127.0.0.1", port=9000, reload=False)