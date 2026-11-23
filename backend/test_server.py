"""Tests for backend/server.py. Run: python -m pytest backend/test_server.py"""
import base64
import os

import pytest
from fastapi.testclient import TestClient

import server

ADMIN = "test-admin-token-0123456789abcdef"


@pytest.fixture()
def client(tmp_path, monkeypatch):
    monkeypatch.setenv("DIVIORA_ADMIN_TOKEN", ADMIN)
    monkeypatch.setenv("DIVIORA_SECRET", base64.b64encode(os.urandom(32)).decode())
    monkeypatch.setenv("DIVIORA_CONFIG_KEY", base64.b64encode(os.urandom(32)).decode())
    monkeypatch.setenv("DIVIORA_DB", str(tmp_path / "t.db"))
    return TestClient(server.create_app())


def admin(c, method, path, **kw):
    return getattr(c, method)(path, headers={"Authorization": f"Bearer {ADMIN}"}, **kw)


def test_one_licence_one_device(client):
    key = admin(client, "post", "/api/v3/admin/licenses", json={"days": 30}).json()["license_key"]
    a = client.post("/api/v3/activate", json={"license_key": key, "device_id": "DEVICE-AAAA-1111"})
    assert a.status_code == 200 and a.json()["session"]
    again = client.post("/api/v3/activate", json={"license_key": key, "device_id": "DEVICE-AAAA-1111"})
    assert again.status_code == 200
    other = client.post("/api/v3/activate", json={"license_key": key, "device_id": "DEVICE-BBBB-2222"})
    assert other.status_code == 403
    admin(client, "post", f"/api/v3/admin/licenses/{key}/reset-device")
    moved = client.post("/api/v3/activate", json={"license_key": key, "device_id": "DEVICE-BBBB-2222"})
    assert moved.status_code == 200


def test_invite_single_use(client):
    tok = admin(client, "post", "/api/v3/admin/invites", json={"days": 7}).json()["invite_token"]
    r = client.post("/api/v3/redeem", json={"invite_token": tok, "device_id": "DEVICE-CCCC-3333"})
    assert r.status_code == 200 and r.json()["license_key"].startswith("DVL-")
    assert client.post("/api/v3/redeem", json={"invite_token": tok, "device_id": "DEVICE-DDDD-4444"}).status_code == 403


def test_config_distribution_and_revocation(client):
    key = admin(client, "post", "/api/v3/admin/licenses", json={"days": 30}).json()["license_key"]
    sess = client.post("/api/v3/activate", json={"license_key": key, "device_id": "DEVICE-EEEE-5555"}).json()["session"]
    h = {"Authorization": f"Bearer {sess}"}
    assert client.get("/api/v3/config", headers=h).json() == {"version": 0, "changed": False}
    v = admin(client, "post", "/api/v3/admin/config", json={"text": "Preamp: -6 dB\n"}).json()["version"]
    got = client.get("/api/v3/config", headers=h).json()
    assert got["changed"] and got["version"] == v and got["text"] == "Preamp: -6 dB\n"
    assert client.get(f"/api/v3/config?since={v}", headers=h).json()["changed"] is False
    admin(client, "post", f"/api/v3/admin/licenses/{key}/revoke")
    assert client.get("/api/v3/config", headers=h).status_code == 403


def test_auth_required(client):
    assert client.post("/api/v3/admin/invites", json={"days": 1}).status_code == 401
    assert client.get("/api/v3/config").status_code == 401
    assert client.get("/api/v3/config", headers={"Authorization": "Bearer forged"}).status_code == 401
    assert client.post("/api/v3/activate", json={"license_key": "NOPE-1234", "device_id": "DEVICE-FFFF-6666"}).status_code == 401


def test_sessions_always_verify(client):
    """Regression: a signature containing the separator byte must still verify (was ~12% failures)."""
    for i in range(200):
        key = admin(client, "post", "/api/v3/admin/licenses", json={"days": 30}).json()["license_key"]
        sess = client.post("/api/v3/activate", json={"license_key": key, "device_id": f"DEVICE-LOOP-{i:04d}"}).json()["session"]
        assert client.get("/api/v3/config", headers={"Authorization": f"Bearer {sess}"}).status_code == 200
