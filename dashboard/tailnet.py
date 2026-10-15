#!/usr/bin/env python3
"""Prints this Mac's tailnet DNS name and Tailscale login, space-separated, for make
install. Prints nothing when Tailscale is missing or not running."""
import json
import subprocess

try:
    out = subprocess.run(["tailscale", "status", "--json"], capture_output=True, text=True, timeout=10).stdout
    status = json.loads(out)
    if status.get("BackendState") == "Running":
        me = status["Self"]
        print(me["DNSName"].rstrip("."), status["User"][str(me["UserID"])]["LoginName"])
except Exception:
    pass
