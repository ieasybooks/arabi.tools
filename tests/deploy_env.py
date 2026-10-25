"""Verify JSON survives both real Compose env-file parsing paths; never uses real keys."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import sys

sys.dont_write_bytecode = True

root = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("write_env", root / "deploy/write-env.py")
writer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(writer)

accounts = [{"type": "service_account", "client_email": "test@example.test", "private_key": 'line 1\nline "2"\n$DOLLAR \\ backslash and apostrophe\'\n', "label": "عربي"}]
values = {"WIT_TOKENS_AR": "test-token", "TUNNEL_TOKEN": "test-tunnel", "DRIVE_SERVICE_ACCOUNTS": json.dumps(accounts, indent=2)}
with tempfile.TemporaryDirectory(prefix="ocr-compose-") as directory:
    directory = Path(directory)
    (directory / ".env").write_text(writer.render(values))
    (directory / "compose.yaml").write_text('''services:
  direct:
    image: example.invalid/test
    env_file: .env
  interpolated:
    image: example.invalid/test
    environment:
      DRIVE_SERVICE_ACCOUNTS: ${DRIVE_SERVICE_ACCOUNTS}
''')
    env = {key: value for key, value in os.environ.items() if key not in values}
    result = subprocess.run(["docker", "compose", "--env-file", ".env", "config", "--format", "json"], cwd=directory, env=env, check=True, capture_output=True, text=True)
    services = json.loads(result.stdout)["services"]
    for name in ["direct", "interpolated"]:
        assert json.loads(services[name]["environment"]["DRIVE_SERVICE_ACCOUNTS"]) == accounts
print("Compose preserved every JSON key value through env_file and --env-file interpolation.")
