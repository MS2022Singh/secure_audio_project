"""Diviora licensing and configuration-distribution server.

* One licence ID = one device: the first activation binds a licence key to a device ID;
  other devices are refused until the developer resets the binding.
* Invite tokens (issued by the developer) create a time-limited licence on redemption.
* The developer publishes the master configuration (Equalizer APO text + watermark
  lines); activated clients poll for new versions and apply them automatically.
* Configurations are stored encrypted (AES-256-GCM) and only served to valid sessions.

Environment (all required in production, see backend/README.md):
  DIVIORA_ADMIN_TOKEN   developer API token (long random string)
  DIVIORA_SECRET        base64 32 bytes: signs client session tokens
  DIVIORA_CONFIG_KEY    base64 32 bytes: encrypts stored configurations
  DIVIORA_DB            SQLite path (default: diviora.db)
Run behind HTTPS (e.g. a reverse proxy); never expose plain HTTP.
"""
import base64
import hashlib
import hmac
import json
import os
import secrets
import sqlite3
import time
from contextlib import contextmanager
from typing import Optional

from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from fastapi import Depends, FastAPI, Header, HTTPException
from pydantic import BaseModel, Field

SESSION_SECONDS = 7 * 24 * 3600


def _env_key(name: str) -> bytes:
    raw = os.environ.get(name)
    if not raw:
        raise RuntimeError(f"Environment variable {name} is required (base64 of 32 random bytes).")
    key = base64.b64decode(raw)
    if len(key) != 32:
        raise RuntimeError(f"{name} must decode to exactly 32 bytes.")
    return key


class Settings:
    def __init__(self):
        self.admin_token = os.environ.get("DIVIORA_ADMIN_TOKEN", "")
        if len(self.admin_token) < 24:
            raise RuntimeError("DIVIORA_ADMIN_TOKEN is required and must be at least 24 characters.")
        self.secret = _env_key("DIVIORA_SECRET")
        self.config_key = _env_key("DIVIORA_CONFIG_KEY")
        self.db_path = os.environ.get("DIVIORA_DB", "diviora.db")


def create_app(settings: Optional[Settings] = None) -> FastAPI:
    s = settings or Settings()
    app = FastAPI(title="Diviora licensing & config API", version="3.0.0")

    @contextmanager
    def db():
        conn = sqlite3.connect(s.db_path)
        conn.row_factory = sqlite3.Row
        try:
            yield conn
            conn.commit()
        finally:
            conn.close()

    with db() as c:
        c.executescript(
            """
            CREATE TABLE IF NOT EXISTS licenses (
                license_key TEXT PRIMARY KEY,
                device_id   TEXT,
                created_at  INTEGER NOT NULL,
                expires_at  INTEGER NOT NULL,
                revoked     INTEGER NOT NULL DEFAULT 0,
                note        TEXT
            );
            CREATE TABLE IF NOT EXISTS invites (
                token       TEXT PRIMARY KEY,
                days        INTEGER NOT NULL,
                created_at  INTEGER NOT NULL,
                redeemed_by TEXT
            );
            CREATE TABLE IF NOT EXISTS configs (
                version     INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at  INTEGER NOT NULL,
                sha256      TEXT NOT NULL,
                nonce       BLOB NOT NULL,
                ciphertext  BLOB NOT NULL
            );
            """
        )

    # ------------------------------------------------------------------ helpers
    def new_key(prefix: str) -> str:
        alphabet = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789"
        body = "".join(secrets.choice(alphabet) for _ in range(20))
        return f"{prefix}-" + "-".join(body[i:i + 5] for i in range(0, 20, 5))

    def sign_session(license_key: str, device_id: str, exp: int) -> str:
        msg = f"{license_key}|{device_id}|{exp}".encode()
        sig = hmac.new(s.secret, msg, hashlib.sha256).digest()
        return base64.urlsafe_b64encode(msg + b"|" + sig).decode()

    def verify_session(token: str) -> tuple:
        try:
            raw = base64.urlsafe_b64decode(token.encode())
            msg, sig = raw.rsplit(b"|", 1)
            license_key, device_id, exp = msg.decode().split("|")
        except Exception:
            raise HTTPException(401, "Invalid session.")
        good = hmac.new(s.secret, msg, hashlib.sha256).digest()
        if not hmac.compare_digest(sig, good) or int(exp) < time.time():
            raise HTTPException(401, "Session expired or invalid. Activate again.")
        with db() as c:
            row = c.execute("SELECT * FROM licenses WHERE license_key=?", (license_key,)).fetchone()
        if not row or row["revoked"] or row["device_id"] != device_id or row["expires_at"] < time.time():
            raise HTTPException(403, "Licence no longer valid for this device.")
        return license_key, device_id

    def require_admin(authorization: str = Header(default="")):
        if not hmac.compare_digest(authorization.removeprefix("Bearer ").strip(), s.admin_token):
            raise HTTPException(401, "Developer token required.")

    def require_session(authorization: str = Header(default="")):
        return verify_session(authorization.removeprefix("Bearer ").strip())

    def session_response(row) -> dict:
        exp = min(int(time.time()) + SESSION_SECONDS, int(row["expires_at"]))
        return {
            "status": "authorized",
            "license_key": row["license_key"],
            "licence_expires_at": int(row["expires_at"]),
            "session": sign_session(row["license_key"], row["device_id"], exp),
            "session_expires_at": exp,
        }

    # ------------------------------------------------------------------ models
    class Activate(BaseModel):
        license_key: str = Field(min_length=4, max_length=64)
        device_id: str = Field(min_length=8, max_length=128)

    class Redeem(BaseModel):
        invite_token: str = Field(min_length=4, max_length=64)
        device_id: str = Field(min_length=8, max_length=128)

    class NewLicense(BaseModel):
        days: int = Field(ge=1, le=36500)
        note: str = ""

    class NewInvite(BaseModel):
        days: int = Field(ge=1, le=3650)

    class NewConfig(BaseModel):
        text: str = Field(max_length=4 * 1024 * 1024)

    # ------------------------------------------------------------------ client API
    @app.get("/")
    def health():
        return {"status": "online", "service": "diviora"}

    @app.post("/api/v3/activate")
    def activate(p: Activate):
        key = p.license_key.strip().upper()
        with db() as c:
            row = c.execute("SELECT * FROM licenses WHERE license_key=?", (key,)).fetchone()
            if not row:
                raise HTTPException(401, "Unknown licence key.")
            if row["revoked"]:
                raise HTTPException(403, "This licence has been revoked.")
            if row["expires_at"] < time.time():
                raise HTTPException(403, "This licence has expired.")
            if row["device_id"] and row["device_id"] != p.device_id:
                raise HTTPException(403, "This licence is already bound to another device (one ID = one device).")
            if not row["device_id"]:
                c.execute("UPDATE licenses SET device_id=? WHERE license_key=?", (p.device_id, key))
                row = c.execute("SELECT * FROM licenses WHERE license_key=?", (key,)).fetchone()
        return session_response(row)

    @app.post("/api/v3/redeem")
    def redeem(p: Redeem):
        token = p.invite_token.strip().upper()
        with db() as c:
            inv = c.execute("SELECT * FROM invites WHERE token=?", (token,)).fetchone()
            if not inv:
                raise HTTPException(401, "Unknown invite token.")
            if inv["redeemed_by"]:
                raise HTTPException(403, "This invite token has already been used.")
            key = new_key("DVL")
            now = int(time.time())
            c.execute("INSERT INTO licenses VALUES (?,?,?,?,0,?)",
                      (key, p.device_id, now, now + inv["days"] * 86400, f"invite {token}"))
            c.execute("UPDATE invites SET redeemed_by=? WHERE token=?", (key, token))
            row = c.execute("SELECT * FROM licenses WHERE license_key=?", (key,)).fetchone()
        return session_response(row)

    @app.get("/api/v3/config")
    def get_config(since: int = 0, who=Depends(require_session)):
        with db() as c:
            row = c.execute("SELECT * FROM configs ORDER BY version DESC LIMIT 1").fetchone()
        if not row:
            return {"version": 0, "changed": False}
        if row["version"] <= since:
            return {"version": row["version"], "changed": False}
        text = AESGCM(s.config_key).decrypt(row["nonce"], row["ciphertext"], str(row["version"]).encode()).decode()
        return {"version": row["version"], "changed": True, "sha256": row["sha256"], "text": text}

    # ------------------------------------------------------------------ developer API
    @app.post("/api/v3/admin/licenses", dependencies=[Depends(require_admin)])
    def create_license(p: NewLicense):
        key, now = new_key("DVL"), int(time.time())
        with db() as c:
            c.execute("INSERT INTO licenses VALUES (?,?,?,?,0,?)", (key, None, now, now + p.days * 86400, p.note))
        return {"license_key": key, "expires_at": now + p.days * 86400}

    @app.get("/api/v3/admin/licenses", dependencies=[Depends(require_admin)])
    def list_licenses():
        with db() as c:
            return [dict(r) for r in c.execute("SELECT * FROM licenses ORDER BY created_at DESC")]

    @app.post("/api/v3/admin/licenses/{key}/revoke", dependencies=[Depends(require_admin)])
    def revoke(key: str):
        with db() as c:
            if c.execute("UPDATE licenses SET revoked=1 WHERE license_key=?", (key,)).rowcount == 0:
                raise HTTPException(404, "No such licence.")
        return {"revoked": key}

    @app.post("/api/v3/admin/licenses/{key}/reset-device", dependencies=[Depends(require_admin)])
    def reset_device(key: str):
        with db() as c:
            if c.execute("UPDATE licenses SET device_id=NULL WHERE license_key=?", (key,)).rowcount == 0:
                raise HTTPException(404, "No such licence.")
        return {"reset": key}

    @app.post("/api/v3/admin/invites", dependencies=[Depends(require_admin)])
    def create_invite(p: NewInvite):
        token = new_key("DVI")
        with db() as c:
            c.execute("INSERT INTO invites VALUES (?,?,?,NULL)", (token, p.days, int(time.time())))
        return {"invite_token": token, "days": p.days}

    @app.post("/api/v3/admin/config", dependencies=[Depends(require_admin)])
    def publish_config(p: NewConfig):
        digest = hashlib.sha256(p.text.encode()).hexdigest()
        with db() as c:
            cur = c.execute("INSERT INTO configs (created_at, sha256, nonce, ciphertext) VALUES (?,?,?,?)",
                            (int(time.time()), digest, b"", b""))
            version = cur.lastrowid
            nonce = os.urandom(12)
            ct = AESGCM(s.config_key).encrypt(nonce, p.text.encode(), str(version).encode())
            c.execute("UPDATE configs SET nonce=?, ciphertext=? WHERE version=?", (nonce, ct, version))
        return {"version": version, "sha256": digest}

    return app


if __name__ == "__main__":
    import uvicorn
    uvicorn.run(create_app(), host=os.environ.get("DIVIORA_HOST", "127.0.0.1"),
                port=int(os.environ.get("DIVIORA_PORT", "9000")))
