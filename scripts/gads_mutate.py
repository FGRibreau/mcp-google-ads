#!/usr/bin/env python3
"""Run a googleAds:mutate request and print Google's full error detail.

The MCP server collapses API failures to "Request contains an invalid argument",
which hides the field-level reason. This sends the same payload and prints the
whole error tree, so a rejected mutate can actually be diagnosed.

Usage:
    scripts/gads_mutate.py operations.json          # apply
    scripts/gads_mutate.py operations.json --check  # validate_only, changes nothing

operations.json is the JSON array that goes in "mutateOperations".
Reads .env from the repo root (same file scripts/mcp-env.sh uses).
"""
import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
API = "https://googleads.googleapis.com/v23"


def load_env():
    env = {}
    path = os.environ.get("MCP_GOOGLE_ADS_ENV", os.path.join(REPO, ".env"))
    with open(path) as fh:
        for line in fh:
            line = line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            k, _, v = line.partition("=")
            env[k.strip()] = v.strip().strip('"').strip("'")
    return env


def access_token(env):
    with open(os.path.expanduser(env["GOOGLE_ADS_CREDENTIALS_PATH"])) as fh:
        cred = json.load(fh)["installed"]
    with open(os.path.expanduser(env["GOOGLE_ADS_TOKEN_PATH"])) as fh:
        refresh = json.load(fh)["refresh_token"]
    body = urllib.parse.urlencode({
        "client_id": cred["client_id"],
        "client_secret": cred["client_secret"],
        "refresh_token": refresh,
        "grant_type": "refresh_token",
    }).encode()
    req = urllib.request.Request("https://oauth2.googleapis.com/token", data=body)
    with urllib.request.urlopen(req) as resp:
        return json.load(resp)["access_token"]


def headers(env, token):
    h = {
        "Authorization": f"Bearer {token}",
        "developer-token": env["GOOGLE_ADS_DEVELOPER_TOKEN"],
        "Content-Type": "application/json",
    }
    login = env.get("GOOGLE_ADS_LOGIN_CUSTOMER_ID", "").replace("-", "")
    if login:
        h["login-customer-id"] = login
    return h


def mutate(env, token, operations, validate_only=False, customer_id=None):
    cust = (customer_id or env["GOOGLE_ADS_CUSTOMER_ID"]).replace("-", "")
    payload = {"mutateOperations": operations, "validateOnly": validate_only}
    req = urllib.request.Request(
        f"{API}/customers/{cust}/googleAds:mutate",
        data=json.dumps(payload).encode(),
        headers=headers(env, token),
    )
    try:
        with urllib.request.urlopen(req) as resp:
            return json.load(resp), None
    except urllib.error.HTTPError as e:
        return None, json.load(e)


def describe(err):
    """Flatten Google's nested error tree into readable lines."""
    lines = []
    for detail in err.get("error", {}).get("details", []):
        for item in detail.get("errors", []):
            code = item.get("errorCode", {})
            code_str = ", ".join(f"{k}={v}" for k, v in code.items())
            loc = item.get("location", {})
            path = ".".join(
                str(f.get("fieldName", f.get("index", "")))
                for f in loc.get("fieldPathElements", [])
            )
            lines.append(f"  [{code_str}] {item.get('message', '')}")
            if path:
                lines.append(f"      at: {path}")
    if not lines:
        lines.append(f"  {err.get('error', {}).get('message', json.dumps(err))}")
    return "\n".join(lines)


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    validate_only = "--check" in sys.argv
    if not args:
        print(__doc__)
        return 2

    with open(args[0]) as fh:
        operations = json.load(fh)

    env = load_env()
    token = access_token(env)
    cid = os.environ.get("GADS_CUSTOMER_ID")
    result, err = mutate(env, token, operations, validate_only, cid)

    if err:
        print(f"FAILED ({'validate' if validate_only else 'apply'}):", file=sys.stderr)
        print(describe(err), file=sys.stderr)
        return 1

    print(f"OK ({'validated' if validate_only else 'applied'}) "
          f"{len(operations)} operation(s)")
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
