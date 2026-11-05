# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A Rust MCP (Model Context Protocol) server that gives LLM agents (Claude, etc.) read + write access to the Google Ads API (v25) via REST. It exposes ~63 MCP tools. All mutations go through a two-step draft → preview → confirm flow with several hard safety guards (budget caps, bid limits, dry-run requirement, double-confirm for destructive ops). Configured entirely via env vars, no config files.

## Commands

```bash
cargo build                                          # debug build
cargo build --release                                # release build (binary used by MCP hosts)
cargo test                                            # unit + wiremock integration tests, no credentials needed
cargo test --test <file_stem>                         # run one integration test file, e.g. `cargo test --test apply_recommendation_error_is_actionable_test`
cargo test <substring>                                # run tests whose name matches (works within lib or a test file)
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check                                     # CI formatting gate; use `cargo fmt` to fix
```

`tests/integration_test.rs` requires real Google Ads test credentials (`GOOGLE_ADS_TEST_*` env vars) and is normally skipped locally; every other file under `tests/` is a standalone wiremock-driven suite (spins up a mock HTTP server, no network/credentials required) and runs as part of plain `cargo test`.

To generate local OAuth2 credentials for manual/manual-integration testing: `./scripts/generate_token.sh ~/.mcp-google-ads/credentials.json`.

## Architecture

### The two-step write flow (the core design)

Every write tool follows the same shape, split across two phases that never touch each other's concerns:

1. **Draft phase** (`src/tools/*_write.rs`, `src/tools/entity_lifecycle.rs`) — pure, synchronous, no network I/O. Validates input against safety guards (`src/safety/guards.rs`: budget cap, bid increase %, char limits, broad+manual-CPC block, URL/path validation), builds the raw Google Ads `MutateOperation` JSON payloads, wraps them in a `ChangePlan` (`src/safety/preview.rs`), and stores it in a process-global in-memory map (`PENDING_PLANS`, keyed by a short UUID `plan_id`). Returns a JSON preview, never executes anything.
2. **Apply phase** (`src/tools/confirm.rs::confirm_and_apply`) — looks up the plan by `plan_id` and dispatches it. This is the only place that performs mutating HTTP calls.

`confirm_and_apply` modes and hard guards (the guards on a real apply are checked **before any HTTP traffic**):
- `dry_run` (default `true`) — sends the plan to `googleAds:mutate` with `validateOnly: true`: Google validates it and applies nothing, and a rejection comes back as Google's error. Recommendation plans (no validate-only mode on their RPCs) are only previewed. The plan is kept either way.
- `config.safety.require_dry_run` (env `GOOGLE_ADS_REQUIRE_DRY_RUN`, default `true`) — rejects `dry_run=false` unless the caller passes a one-shot `bypass_require_dry_run=true`.
- `plan.requires_double_confirm` (set by `src/safety/guards.rs::requires_double_confirmation` — true for any `delete`/`remove` op or a >50% budget increase) — rejects unless `confirmed_twice=true`.

On success the plan is removed from `PENDING_PLANS` and the mutation is logged via `src/safety/audit.rs`; on failure the plan is **kept** so the caller can retry (e.g. with `exempt_policy_violations=true` after a policy rejection — see `src/safety/policy_exemption.rs`).

`ChangePlan.dispatch` (`PlanDispatch` enum) routes the plan to one of three RPCs — this exists because Google Ads v25 does not accept recommendation apply/dismiss operations through the generic mutate endpoint:
- `MutateOperations` (default) → `POST /v25/customers/{cid}/googleAds:mutate`
- `ApplyRecommendation` → `POST /v25/customers/{cid}/recommendations:apply`
- `DismissRecommendation` → `POST /v25/customers/{cid}/recommendations:dismiss`

### Layers

- `src/lib.rs` — the `GoogleAdsMcp` struct and the `#[rmcp::tool_router]` impl block containing every `#[tool(...)]`-annotated MCP method plus their `Parameters<...>ToolParams` structs (all `#[serde(deny_unknown_fields)]` — unknown params are rejected, not ignored). Tool methods are thin: resolve the customer ID, call `check_write_allowed()` for writes, delegate to a `src/tools/*` function, serialize the result. Read tools go through the `run_tool()` helper (creates a `GoogleAdsClient`, awaits, serializes errors uniformly); write tools call the draft functions directly (synchronous, no client needed) except where a resource must be resolved via a live GAQL lookup first (e.g. `update_campaign`'s budget resource name, `resolve_ad_composite_id` for ad-group-scoped ad IDs).
- `src/client.rs` — `GoogleAdsClient`: OAuth2 token handling (via `yup-oauth2`), the raw `search`/`mutate`/`apply_recommendations`/`dismiss_recommendations` HTTP calls, and `validate_mutate_operations` — a client-side whitelist of valid top-level `MutateOperation` keys that rejects typos/mis-routed recommendation ops before any HTTP call.
- `src/tools/` — one module per domain (`campaigns_write.rs`, `ads_write.rs`, `keywords_write.rs`, `shared_sets_write.rs`, `entity_lifecycle.rs`, `pmax.rs`, etc.), split into read (no `_write` suffix) and write (`_write` suffix) files. Write-side functions are pure preview builders as described above.
- `src/safety/` — `guards.rs` (validation + limit checks), `preview.rs` (`ChangePlan` + the `PENDING_PLANS` store), `confirm.rs` is actually under `src/tools/confirm.rs` (the apply dispatcher), `audit.rs` (JSON audit log writer), `policy_exemption.rs` (parses Google ad-policy violations from an error response and resubmits with `exemptPolicyViolationKeys` when the caller opts in).
- `src/models/` — small serde enums shared across tool/API boundaries: `AdStatus` (defaults to `Paused` — new campaigns/ad groups/ads are created paused unless the caller explicitly opts into `ENABLED`), `NextActionHint` (tells the agent which MCP tool to call next, e.g. `enable_entity`, so it never needs a UI step), `AdRotationMode`, `GeoTargetType`, demographic enums.
- `src/error.rs` — `McpGoogleAdsError` (thiserror), converted to a JSON error envelope at the MCP boundary (`src/lib.rs::error_response`).
- `src/gaql.rs` — GAQL query helpers used by read tools and by `run_gaql`.
- `src/tools/write_tools_registration.txt` is a **stale scaffolding note** from an earlier iteration (different, non-async tool signatures) — not part of the build, do not treat it as current; the live implementation is the `#[rmcp::tool_router]` block in `src/lib.rs`.

### Safety-by-default conventions worth preserving

- New campaigns/ad groups/ads default to `PAUSED`; flipping to `ENABLED` is a deliberate opt-in (`status: "ENABLED"` param) or a follow-up `enable_entity` call surfaced via `next_action_hint`.
- Destructive tools (`remove_*`, `delete_*`, `detach_*`) are irreversible against the Google Ads account and must be flagged `requires_double_confirm` in their `ChangePlan`.
- `confirm_and_apply` never emits a "successful but you should have dry-run'd" warning — cosmetic warnings with no enforcement were removed as security theatre (see `docs/decisions/0001-v0.3.0-fix-strategy.md`).
- Read tools default to a 30-day window when no explicit date range is given.
- `GOOGLE_ADS_READ_ONLY=true` disables all write tools at the `check_write_allowed()` gate in `src/lib.rs`.
- `GOOGLE_ADS_MCP_TRANSPORT` (`src/main.rs`, `src/config.rs::Transport`) picks the wire transport: `stdio` (default) or `http` (streamable HTTP per the MCP spec). HTTP always binds `0.0.0.0` (container/reverse-proxy friendly) on the port from `GOOGLE_ADS_MCP_HTTP_PORT` (default `8080`) — there's no separate bind-IP setting. In HTTP mode `GoogleAdsMcp::new` is called once per session by `StreamableHttpService`'s factory closure (`LocalSessionManager`) rather than once at process startup, so it must stay cheap and side-effect-free — it currently just stores `Config` and builds the tool router.
- The bind address/port and the inbound `Host` header allowlist are two independent guards — binding `0.0.0.0` does not widen which `Host` values are accepted. `rmcp`'s `StreamableHttpServerConfig` defaults `allowed_hosts` to loopback only (`localhost`/`127.0.0.1`/`::1`, a DNS-rebinding guard) regardless of the bind address; `GOOGLE_ADS_MCP_ALLOWED_HOSTS` (`config.rs::env_allowed_hosts`, comma-separated, `*` = accept any) is the explicit opt-in to widen it, e.g. for a real hostname behind a reverse proxy.

### Testing pattern

Most test files under `tests/` are named after the single behavior they pin down (e.g. `require_dry_run_hard_guards_apply_test.rs`, `partial_failure_surfaces_as_error_test.rs`) and use `tests/common/mod.rs`'s `spawn_mock_google_ads()` to stand up a `wiremock::MockServer` and a `GoogleAdsClient` pointed at it — `test_config()` uses a nonexistent credentials path, which makes the client fall through to an in-process test bearer-token bypass, so no real OAuth flow runs. When adding a new safety guard or dispatch route, add a same-shaped test file asserting the actual HTTP request/response shape (endpoint routed to, body sent) rather than only the preview JSON — this is called out in `docs/decisions/0001-v0.3.0-fix-strategy.md` as the root cause class behind several past bugs (untested routing + untested payload contents).

### Adding a new write tool

1. Add a draft function in the appropriate `src/tools/*_write.rs` (or a new module) that validates via `src/safety/guards.rs`, builds the `MutateOperation` JSON, and returns a `ChangePlan` (via `src/safety/preview.rs::store_plan`).
2. Add a `...ToolParams` struct and a `#[tool(...)]` method in the `#[rmcp::tool_router]` impl in `src/lib.rs`, gated by `check_write_allowed()`.
3. Default any lifecycle `status` field to `PAUSED` unless there's a specific reason not to.
4. Add a wiremock test file under `tests/` asserting the request actually sent.
