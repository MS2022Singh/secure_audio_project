# Diviora licence and configuration server

FastAPI service for licence activation (one licence = one device), invite tokens, and
distribution of the developer's configuration to activated devices.

## Run locally (testing)

```bash
cd backend
python -m pip install -r requirements.txt
```

Generate the three secrets once and keep them (losing `DIVIORA_CONFIG_KEY` makes stored
configurations unreadable):

```bash
python -c "import os,base64,secrets;print('DIVIORA_ADMIN_TOKEN='+secrets.token_urlsafe(32));print('DIVIORA_SECRET='+base64.b64encode(os.urandom(32)).decode());print('DIVIORA_CONFIG_KEY='+base64.b64encode(os.urandom(32)).decode())"
```

Set them as environment variables, then start the server:

```bash
python server.py
```

It listens on `127.0.0.1:9000`; set `DIVIORA_HOST` and `DIVIORA_PORT` to change that, and
`DIVIORA_DB` for the database path.

Tests:

```bash
python -m pytest -q test_server.py
```

## Production

- Run it on a server or cloud host behind **HTTPS** (for example Caddy or nginx with a
  certificate). Clients refuse plain HTTP in practice because they talk to Windows' TLS stack.
- Back up `diviora.db` and the three secrets.
- Put the HTTPS URL in the GitHub secret `DIVIORA_SERVER_URL` (user builds) and in
  `developer.json` (developer PC); see `docs/DEVELOPER_GUIDE.md`.

## API (v3)

| Method & path | Auth | Purpose |
|---|---|---|
| `POST /api/v3/activate` `{license_key, device_id}` | – | Bind/verify a licence on a device, returns a session |
| `POST /api/v3/redeem` `{invite_token, device_id}` | – | Turn an invite into a licence bound to the device |
| `GET /api/v3/config?since=N` | session | Latest configuration if newer than N |
| `POST /api/v3/admin/licenses` `{days, note}` | admin | Create a licence key |
| `GET /api/v3/admin/licenses` | admin | List licences |
| `POST /api/v3/admin/licenses/{key}/revoke` | admin | Revoke |
| `POST /api/v3/admin/licenses/{key}/reset-device` | admin | Release the device binding |
| `POST /api/v3/admin/invites` `{days}` | admin | Create a single-use invite |
| `POST /api/v3/admin/config` `{text}` | admin | Publish a new configuration version |
