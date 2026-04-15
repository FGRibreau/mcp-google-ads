<div align="center">

# mcp-google-ads

**Pilot Google Ads campaigns from Claude with built-in safety guardrails**

<br/>

<img src="assets/banner.svg" alt="mcp-google-ads architecture" width="700"/>

<br/>
<br/>

[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://rust-lang.org)
[![MCP](https://img.shields.io/badge/MCP-compatible-green.svg)](https://modelcontextprotocol.io)
[![Google Ads API](https://img.shields.io/badge/Google%20Ads%20API-v23-4285F4.svg)](https://developers.google.com/google-ads/api)

</div>

---

## What is this?

An MCP server that gives Claude full read + write access to Google Ads accounts via the REST API. Every write operation goes through a two-step safety layer: draft a change, review the preview, then confirm to execute. Budget caps, bid limits, and audit logging prevent accidental spend.

## Features

- **33 tools** - Campaign management, RSA ads, keywords, extensions, PMax, audiences, bidding, scheduling
- **Two-step safety** - All mutations return a preview; nothing executes until you confirm
- **Budget guardrails** - Configurable daily budget cap, bid increase limits, broad+manual CPC blocker
- **Audit logging** - Every mutation logged to a local JSON file with timestamp and dry-run status
- **Read-only mode** - Expose only read tools for safe observability
- **Zero config files** - Configured entirely via environment variables

---

## Quick Start

### 1. Build

```bash
git clone https://github.com/YOUR_USER/mcp-google-ads.git
cd mcp-google-ads
cargo build --release
```

### 2. Set up credentials

```bash
# Generate OAuth2 refresh token
./scripts/generate_token.sh ~/.mcp-google-ads/credentials.json
```

### 3. Configure Claude Code

Add to `~/.claude/settings.json`:

```json
{
  "mcpServers": {
    "google-ads": {
      "command": "/path/to/mcp-google-ads/target/release/mcp-google-ads",
      "env": {
        "GOOGLE_ADS_DEVELOPER_TOKEN": "your-developer-token",
        "GOOGLE_ADS_CUSTOMER_ID": "123-456-7890",
        "GOOGLE_ADS_CREDENTIALS_PATH": "~/.mcp-google-ads/credentials.json",
        "GOOGLE_ADS_TOKEN_PATH": "~/.mcp-google-ads/token.json"
      }
    }
  }
}
```

---

## Configuration

All configuration is via environment variables. No config files.

| Variable | Default | Description |
|----------|---------|-------------|
| `GOOGLE_ADS_DEVELOPER_TOKEN` | *required* | Google Ads API developer token |
| `GOOGLE_ADS_CUSTOMER_ID` | *required* | Target account ID (e.g. `123-456-7890`) |
| `GOOGLE_ADS_CREDENTIALS_PATH` | `~/.mcp-google-ads/credentials.json` | OAuth2 credentials JSON |
| `GOOGLE_ADS_TOKEN_PATH` | `~/.mcp-google-ads/token.json` | Refresh token JSON |
| `GOOGLE_ADS_LOGIN_CUSTOMER_ID` | | MCC manager account ID (optional) |
| `GOOGLE_ADS_MAX_DAILY_BUDGET` | `50.0` | Maximum daily budget allowed per campaign |
| `GOOGLE_ADS_MAX_BID_INCREASE_PCT` | `100` | Maximum bid increase percentage |
| `GOOGLE_ADS_REQUIRE_DRY_RUN` | `true` | Default dry_run to true on confirm_and_apply |
| `GOOGLE_ADS_AUDIT_LOG` | `~/.mcp-google-ads/audit.log` | Path for mutation audit log |
| `GOOGLE_ADS_BLOCKED_OPS` | | Comma-separated list of blocked operations |
| `GOOGLE_ADS_READ_ONLY` | `false` | Disable all write tools |

---

## Tools

### Read (11 tools)

| Tool | Description |
|------|-------------|
| `health_check` | Check server connectivity and configuration |
| `list_accounts` | List all accessible Google Ads accounts |
| `get_campaign_performance` | Campaign metrics (cost, clicks, conversions, CPA) |
| `get_ad_performance` | Ad-level metrics with headlines and descriptions |
| `get_keyword_performance` | Keyword metrics with quality scores |
| `get_search_terms` | Actual user queries that triggered ads |
| `get_negative_keywords` | List campaign negative keywords |
| `run_gaql` | Execute arbitrary GAQL queries (json/table/csv) |
| `search_geo_targets` | Find location IDs for geo-targeting |
| `get_geo_performance` | Performance breakdown by location |
| `list_recommendations` | Active Google Ads recommendations |

### Write (22 tools)

All write tools return a preview. Call `confirm_and_apply` with `dry_run=false` to execute.

| Tool | Description |
|------|-------------|
| `draft_campaign` | Create campaign (PAUSED) + ad group + keywords |
| `update_campaign` | Modify budget, bidding, targeting |
| `draft_responsive_search_ad` | Create RSA (3-15 headlines, 2-4 descriptions) |
| `draft_keywords` | Add keywords with match types |
| `add_negative_keywords` | Block irrelevant searches |
| `draft_sitelinks` | Sitelink extensions |
| `create_callouts` | Callout extensions |
| `create_structured_snippets` | Structured snippet extensions |
| `create_pmax_campaign` | Performance Max campaign with text assets |
| `create_custom_audience` | Remarketing / customer match audiences |
| `add_audience_targeting` | Target audiences (TARGETING/OBSERVATION) |
| `create_portfolio_bidding_strategy` | Portfolio bidding (CPA, ROAS, impression share) |
| `update_keyword_bid` | Modify keyword CPC bid |
| `upload_image_asset` | Upload base64-encoded image |
| `upload_text_asset` | Create reusable text asset |
| `set_campaign_schedule` | Ad scheduling / dayparting |
| `apply_recommendation` | Apply a Google recommendation |
| `dismiss_recommendation` | Dismiss a recommendation |
| `pause_entity` | Pause campaign/ad group/ad/keyword |
| `enable_entity` | Enable paused entity |
| `remove_entity` | Permanently remove entity (destructive) |
| `confirm_and_apply` | Execute a previewed change (dry_run=true default) |

---

## Safety Guardrails

| Guard | Behavior |
|-------|----------|
| **Two-step confirm** | Every write returns a plan preview; must call `confirm_and_apply` to execute |
| **Dry-run default** | `confirm_and_apply` defaults to `dry_run=true` — must explicitly set `false` |
| **Budget cap** | Rejects campaigns exceeding `GOOGLE_ADS_MAX_DAILY_BUDGET` |
| **Bid limit** | Rejects bid increases exceeding `GOOGLE_ADS_MAX_BID_INCREASE_PCT` |
| **Broad + Manual CPC** | Blocks BROAD match with MANUAL_CPC (burns budget) |
| **PAUSED by default** | New campaigns and ads are created PAUSED |
| **Blocked operations** | Configure `GOOGLE_ADS_BLOCKED_OPS` to disable specific tools |
| **Read-only mode** | Set `GOOGLE_ADS_READ_ONLY=true` to disable all writes |
| **Audit logging** | Every mutation logged with timestamp, operation, changes, dry_run status |
| **Double confirmation** | Destructive ops (remove) and large budget increases require double confirm |

---

## Credentials Setup

### 1. Create a test account

1. Create a **Manager Account (MCC)** at [ads.google.com](https://ads.google.com/home/tools/manager-accounts/)
2. In the MCC, create a **Test Account** (Settings → Sub-account settings)
3. Note both IDs

### 2. Get a developer token

1. In MCC → **Tools & Settings → API Center**
2. Request a developer token (test access is sufficient)

### 3. Create OAuth2 credentials

1. [Google Cloud Console](https://console.cloud.google.com) → Enable **Google Ads API**
2. **Credentials → Create → OAuth 2.0 Client ID → Desktop App**
3. Download JSON to `~/.mcp-google-ads/credentials.json`

### 4. Generate refresh token

```bash
./scripts/generate_token.sh
```

---

## Comparison with Other Google Ads MCP Servers

| | **mcp-google-ads** (this) | [adloop](https://github.com/kLOsk/adloop) | [mikdeangelis](https://github.com/mikdeangelis/mcp-google-ads) | [grantweston](https://github.com/grantweston/google-ads-mcp-complete) | [Official Google](https://github.com/googleads/google-ads-mcp) |
|---|---|---|---|---|---|
| **Language** | Rust | Python | Python | Python | Python |
| **API version** | v23 | v23 | v23 | v21 | v23 |
| **Total tools** | 33 | 26 | 52 | 45+ | 2 |
| **Write tools** | 22 | 8 | 26 | 25 | 0 |
| **Tests** | 183 | partial | 0 | 0 | N/A |

### Features

| Feature | this | adloop | mikdeangelis | grantweston | Official |
|---------|------|--------|-------------|-------------|----------|
| Campaign creation | Search + PMax | Search | Search + PMax | 7 types | - |
| RSA ads | yes | yes | yes | yes | - |
| Keywords (positive + negative) | yes | yes | yes | yes | - |
| Extensions (sitelinks, callouts, snippets) | yes | sitelinks only | yes | yes | - |
| Audiences | yes | - | - | yes | - |
| Bid adjustments | yes | - | - | yes | - |
| Geo targeting (set/remove) | yes | via update | yes | hardcoded US | - |
| Ad scheduling | yes | - | yes | yes | - |
| Asset upload (image/text) | yes | - | yes | yes | - |
| Recommendations (apply/dismiss) | yes | - | yes | yes | - |
| Arbitrary GAQL queries | yes | yes | - | - | yes |
| GA4 integration | - | yes | - | - | - |

### Safety & Reliability

| Guard | this | adloop | mikdeangelis | grantweston | Official |
|-------|------|--------|-------------|-------------|----------|
| Two-step confirm (draft → review → apply) | yes | yes | - | - | N/A |
| Dry-run default | yes | yes | - | - | N/A |
| Budget cap | yes | yes (50EUR) | - | - | N/A |
| Bid increase limit | yes | - | - | - | N/A |
| Broad + Manual CPC blocker | yes | yes | - | - | N/A |
| Campaigns created PAUSED | yes | yes | default | ENABLED | N/A |
| Audit logging (JSON) | yes | yes | - | - | N/A |
| Read-only mode | yes | - | - | - | N/A |
| Blocked operations list | yes | - | - | - | N/A |
| Input validation (char limits, URLs) | yes | yes | Pydantic | partial | N/A |
| Unit test coverage | 183 tests | partial | 0 | 0 | N/A |
| Binary size / startup | 10.8 MB / instant | Python | Python | Python | Python |

---

## Development

```bash
cargo build              # Build debug
cargo build --release     # Build release (10.8 MB binary)
cargo test                # Run 183 tests
cargo clippy -- -D warnings
cargo fmt --check
```

---

## Sponsors

<table>
  <tr>
    <td align="center" width="200">
        <a href="https://getnatalia.com/">
        <img src="assets/sponsors/natalia.svg" height="60" alt="Natalia"/><br/>
        <b>Natalia</b>
        </a><br/>
        <sub>24/7 AI voice and whatsapp agent for customer services</sub>
    </td>
    <td align="center" width="200">
      <a href="https://nobullshitconseil.com/">
        <img src="assets/sponsors/nobullshitconseil.svg" height="60" alt="NoBullshitConseil"/><br/>
        <b>NoBullshitConseil</b>
      </a><br/>
      <sub>360° tech consulting</sub>
    </td>
    <td align="center" width="200">
      <a href="https://www.hook0.com/">
        <img src="assets/sponsors/hook0.png" height="60" alt="Hook0"/><br/>
        <b>Hook0</b>
      </a><br/>
      <sub>Open-Source Webhooks-as-a-Service</sub>
    </td>
    <td align="center" width="200">
      <a href="https://france-nuage.fr/">
        <img src="assets/sponsors/france-nuage.png" height="60" alt="France-Nuage"/><br/>
        <b>France-Nuage</b>
      </a><br/>
      <sub>Sovereign cloud hosting in France</sub>
    </td>
  </tr>
</table>

> **Interested in sponsoring?** [Get in touch](mailto:rust@fgribreau.com)

---

## License

[MIT](LICENSE)
