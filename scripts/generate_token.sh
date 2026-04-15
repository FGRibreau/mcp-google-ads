#!/bin/bash
# Generate OAuth2 refresh token for Google Ads API
# Uses localhost loopback redirect (Google's recommended method for desktop apps)
# The OOB flow (urn:ietf:wg:oauth:2.0:oob) was removed by Google in 2022.

set -euo pipefail

CREDENTIALS_FILE="${1:-$HOME/.mcp-google-ads/credentials.json}"

if [ ! -f "$CREDENTIALS_FILE" ]; then
    echo "ERROR: credentials file not found at $CREDENTIALS_FILE"
    echo "Usage: $0 [path/to/credentials.json]"
    echo ""
    echo "Steps:"
    echo "1. Go to https://console.cloud.google.com"
    echo "2. Enable 'Google Ads API'"
    echo "3. Credentials → Create → OAuth 2.0 Client ID → Desktop App"
    echo "   IMPORTANT: Choose 'Desktop app' type, NOT 'Web application'"
    echo "4. Download the JSON file"
    echo "5. Save it to ~/.mcp-google-ads/credentials.json"
    exit 1
fi

# Detect credential type
CRED_TYPE=$(python3 -c "
import json, sys
d = json.load(open('$CREDENTIALS_FILE'))
if 'installed' in d:
    print('installed')
elif 'web' in d:
    print('web')
elif d.get('type') == 'authorized_user':
    print('authorized_user')
else:
    print('unknown')
")

if [ "$CRED_TYPE" = "unknown" ]; then
    echo "ERROR: Unrecognized credentials format in $CREDENTIALS_FILE"
    echo "Expected a 'Desktop app' OAuth2 client (has 'installed' key)."
    echo "Download from: Google Cloud Console → APIs & Services → Credentials → OAuth 2.0 Client IDs"
    exit 1
fi

if [ "$CRED_TYPE" = "web" ]; then
    echo "WARNING: This credentials file is for a 'Web application' OAuth2 client."
    echo "For the loopback redirect to work, you need a 'Desktop app' client."
    echo ""
    echo "Fix: Go to Google Cloud Console → APIs & Services → Credentials"
    echo "     → Create Credentials → OAuth 2.0 Client ID → Desktop App"
    echo "     → Download the JSON and replace $CREDENTIALS_FILE"
    echo ""
    read -p "Try anyway? (y/N) " CONTINUE
    if [ "$CONTINUE" != "y" ] && [ "$CONTINUE" != "Y" ]; then
        exit 1
    fi
fi

if [ "$CRED_TYPE" = "authorized_user" ]; then
    echo "This credentials file already contains an authorized_user token."
    echo "Copying it as your token file."
    TOKEN_FILE="$HOME/.mcp-google-ads/token.json"
    mkdir -p "$(dirname "$TOKEN_FILE")"
    cp "$CREDENTIALS_FILE" "$TOKEN_FILE"
    chmod 600 "$TOKEN_FILE"
    echo "Done: $TOKEN_FILE"
    exit 0
fi

CLIENT_ID=$(python3 -c "import json; d=json.load(open('$CREDENTIALS_FILE')); print(d.get('installed',d.get('web',{})).get('client_id',''))")
CLIENT_SECRET=$(python3 -c "import json; d=json.load(open('$CREDENTIALS_FILE')); print(d.get('installed',d.get('web',{})).get('client_secret',''))")

if [ -z "$CLIENT_ID" ] || [ -z "$CLIENT_SECRET" ]; then
    echo "ERROR: Could not extract client_id/client_secret from $CREDENTIALS_FILE"
    exit 1
fi

PORT=8085
REDIRECT_URI="http://localhost:${PORT}"
SCOPE="https://www.googleapis.com/auth/adwords"

AUTH_URL="https://accounts.google.com/o/oauth2/v2/auth?client_id=${CLIENT_ID}&redirect_uri=${REDIRECT_URI}&scope=${SCOPE}&response_type=code&access_type=offline&prompt=consent"

echo "=== Google Ads OAuth2 Token Generator ==="
echo ""
echo "1. Opening browser for Google sign-in..."
echo "   (If it doesn't open, copy this URL manually):"
echo ""
echo "   $AUTH_URL"
echo ""

# Try to open browser
if command -v open &>/dev/null; then
    open "$AUTH_URL"
elif command -v xdg-open &>/dev/null; then
    xdg-open "$AUTH_URL"
fi

echo "2. Waiting for OAuth2 callback on http://localhost:${PORT} ..."
echo ""

# Start a minimal HTTP server to capture the auth code
AUTH_CODE=$(python3 -c "
import http.server
import urllib.parse
import sys

class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        query = urllib.parse.urlparse(self.path).query
        params = urllib.parse.parse_qs(query)

        if 'error' in params:
            self.send_response(400)
            self.send_header('Content-Type', 'text/html')
            self.end_headers()
            error = params['error'][0]
            desc = params.get('error_description', [''])[0]
            self.wfile.write(f'<h1>Authorization failed</h1><p>{error}: {desc}</p>'.encode())
            print(f'ERROR:{error}:{desc}', file=sys.stderr)
            raise SystemExit(1)

        code = params.get('code', [''])[0]
        if code:
            self.send_response(200)
            self.send_header('Content-Type', 'text/html')
            self.end_headers()
            self.wfile.write(b'<h1>Authorization successful!</h1><p>You can close this tab and return to the terminal.</p>')
            print(code)
        else:
            self.send_response(400)
            self.send_header('Content-Type', 'text/html')
            self.end_headers()
            self.wfile.write(b'<h1>No authorization code received</h1>')

    def log_message(self, format, *args):
        pass  # Suppress HTTP logs

server = http.server.HTTPServer(('127.0.0.1', ${PORT}), Handler)
server.timeout = 120  # 2 minute timeout
server.handle_request()
")

if [ -z "$AUTH_CODE" ]; then
    echo "ERROR: No authorization code received. Did you authorize in the browser?"
    exit 1
fi

echo "3. Got authorization code, exchanging for tokens..."

# Exchange code for tokens
RESPONSE=$(curl -s -X POST "https://oauth2.googleapis.com/token" \
    -d "code=${AUTH_CODE}" \
    -d "client_id=${CLIENT_ID}" \
    -d "client_secret=${CLIENT_SECRET}" \
    -d "redirect_uri=${REDIRECT_URI}" \
    -d "grant_type=authorization_code")

REFRESH_TOKEN=$(echo "$RESPONSE" | python3 -c "import sys,json; print(json.load(sys.stdin).get('refresh_token',''))" 2>/dev/null)

if [ -z "$REFRESH_TOKEN" ]; then
    echo ""
    echo "ERROR: Failed to get refresh token. Response:"
    echo "$RESPONSE"
    echo ""
    echo "Common causes:"
    echo "  - Wrong OAuth2 client type (need 'Desktop app', not 'Web application')"
    echo "  - http://localhost:${PORT} not in authorized redirect URIs"
    echo "  - Google Ads API not enabled in the project"
    exit 1
fi

TOKEN_FILE="$HOME/.mcp-google-ads/token.json"
mkdir -p "$(dirname "$TOKEN_FILE")"
echo "{\"refresh_token\": \"$REFRESH_TOKEN\"}" > "$TOKEN_FILE"
chmod 600 "$TOKEN_FILE"

echo ""
echo "=== SUCCESS ==="
echo "Refresh token saved to: $TOKEN_FILE"
echo ""
echo "Now set these environment variables (add to your shell profile or Claude Code MCP config):"
echo ""
echo "  export GOOGLE_ADS_CREDENTIALS_PATH=\"$CREDENTIALS_FILE\""
echo "  export GOOGLE_ADS_TOKEN_PATH=\"$TOKEN_FILE\""
echo "  export GOOGLE_ADS_DEVELOPER_TOKEN=\"YOUR_DEVELOPER_TOKEN\""
echo "  export GOOGLE_ADS_CUSTOMER_ID=\"YOUR_TEST_ACCOUNT_ID\""
echo "  # export GOOGLE_ADS_LOGIN_CUSTOMER_ID=\"YOUR_MCC_ID\"  # optional, for MCC"
echo ""
echo "Or configure in Claude Code settings.json:"
echo ""
echo '  "google-ads": {'
echo "    \"command\": \"$(cd "$(dirname "$0")/.." && pwd)/target/release/mcp-google-ads\","
echo '    "env": {'
echo "      \"GOOGLE_ADS_CREDENTIALS_PATH\": \"$CREDENTIALS_FILE\","
echo "      \"GOOGLE_ADS_TOKEN_PATH\": \"$TOKEN_FILE\","
echo '      "GOOGLE_ADS_DEVELOPER_TOKEN": "YOUR_DEVELOPER_TOKEN",'
echo '      "GOOGLE_ADS_CUSTOMER_ID": "YOUR_TEST_ACCOUNT_ID"'
echo '    }'
echo '  }'
