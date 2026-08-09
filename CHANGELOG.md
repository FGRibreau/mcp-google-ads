# Changelog

All notable changes to `mcp-google-ads` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.7.0] - Unreleased

### Added

- `link_asset_to_asset_group`: link an existing asset to a Performance Max
  asset group via `assetGroupAssetOperation.create`. `upload_image_asset` only
  ever created the asset — nothing attached it, so uploaded images sat unused
  in the account's asset library. A PMax asset group without a
  `MARKETING_IMAGE`, `SQUARE_MARKETING_IMAGE` and `LOGO` is "Not eligible" and
  never serves, so this is the step that makes an asset group deliverable. The
  field type is validated client-side against the 11 types PMax accepts, since
  the API error for a bad one is opaque.
- `add_asset_group_signal`: add search themes and/or audience signals to a
  Performance Max asset group via `assetGroupSignalOperation.create`. PMax does
  not take audiences as campaign criteria — `add_audience_targeting` writes a
  `campaignCriterion`, which the API rejects for a PMax campaign. Accepts
  search themes, audience IDs, or both, and enforces the 80-character search
  theme limit before the request goes out.
- `scripts/gads_mutate.py`: send a `googleAds:mutate` payload and print
  Google's full error tree, with `--check` for `validateOnly`. The server
  collapses API failures to "Request contains an invalid argument", which hides
  the field-level reason that actually explains a rejected mutate.

### Fixed

- `create_pmax_campaign` now sets `campaign_budget.explicitly_shared = false`.
  Budgets default to shared server-side, and Performance Max rejects a shared
  budget with `BIDDING_STRATEGY_TYPE_INCOMPATIBLE_WITH_SHARED_BUDGET`.
  `draft_campaign` got this fix in 5670d2d; the PMax path was missed.
- `create_pmax_campaign` now sets
  `campaign.contains_eu_political_advertising = DOES_NOT_CONTAIN_EU_POLITICAL_ADVERTISING`.
  The field is required on every campaign create, and omitting it failed the
  entire mutate with `fieldError=REQUIRED`.

  Together these two meant `create_pmax_campaign` could not create any
  Performance Max campaign — every call failed at `confirm_and_apply`.

- `set_campaign_geo_target_type`: set a campaign's
  `campaign.geo_target_type_setting` — `positive_geo_target_type` and/or
  `negative_geo_target_type` — via a `campaignOperation.update` with a matching
  field mask. The lever `update_campaign` did not expose. Setting
  `positive_geo_target_type = PRESENCE` restricts serving to people physically
  in (or regularly in) the targeted locations, instead of the API default
  `PRESENCE_OR_INTEREST` which also serves people who merely show interest in
  them (and burns budget on out-of-area traffic). At least one of the two
  fields must be provided or the call is rejected, mirroring `update_campaign`'s
  "no changes" guard. Returns a preview to confirm via `confirm_and_apply`.
- `models::GeoTargetType` enum (`PRESENCE_OR_INTEREST`, `PRESENCE`) with serde
  rename `"SCREAMING_SNAKE_CASE"`, matching the Google Ads REST API
  `GeoTargetTypeSetting` enum values. The same two values are valid for both the
  positive and negative geo target type fields.
- `draft_keywords` and `draft_campaign` now accept an optional per-keyword
  `final_url`. When set, it writes `ad_group_criterion.final_urls` on the
  keyword's `adGroupCriterionOperation.create`, routing clicks on that keyword
  to a specific landing page instead of inheriting the ad's final URL. Omit the
  field to inherit as before. Final URLs are validated as absolute `http(s)`
  URLs bounded to 2048 characters (`safety::guards::validate_final_url`).

### Changed (BREAKING)

- `tools::campaigns_write::KeywordInput` and
  `tools::keywords_write::KeywordWithMatchType` each gain a
  `final_url: Option<String>` field. Rust library callers constructing these
  structs must add `final_url: None` (or `Some(url)`). MCP-tool callers are
  unaffected — the new field is optional in the tool schema.

## [0.6.2] - 2026-07-15

### Fixed

- `get_campaign_performance`, `get_ad_performance` and `get_keyword_performance`
  now default to the last 30 days (`segments.date DURING LAST_30_DAYS`) when no
  date range is given, matching their documented behaviour. They previously
  emitted no `segments.date` predicate at all and returned **lifetime totals**,
  which silently inflated cost/clicks/conversions (observed ~2x the real 30-day
  figures) and could mislead CPA/budget decisions. `get_search_terms` and
  `get_geo_performance` already defaulted to 30 days and are unchanged.

## [0.6.1] - 2026-07-06

### Fixed

- `draft_campaign`: the campaign budget is now created with
  `explicitlyShared: false`. The Google Ads API defaults `explicitly_shared`
  to `true`, and campaign-level Smart Bidding strategies (e.g.
  `MAXIMIZE_CONVERSIONS`) are rejected on shared budgets with
  `BIDDING_STRATEGY_TYPE_INCOMPATIBLE_WITH_SHARED_BUDGET`.
- `draft_campaign`: `networkSettings` is now derived from `channel_type`
  instead of always sending search-network settings. `DISPLAY` campaigns were
  rejected with `OPERATION_NOT_PERMITTED_FOR_CONTEXT` on
  `network_settings.target_google_search`. SEARCH targets Google Search only
  (search partners off by default), DISPLAY targets the Display Network, and
  other channel types omit `networkSettings` so the API infers valid networks.
- `googleAds:mutate` requests are now sent with `partialFailure: false`
  (atomic). Previously a failed multi-operation plan committed the operations
  that succeeded — e.g. a rejected campaign create still left its freshly
  created budget behind as an orphan, once per retry.

## [0.6.0] - 2026-06-24

### Added

- `exclude_geo_target`: exclude a geographic location from a campaign by adding
  a negative `campaignCriterion` location criterion — the inverse of the
  positive geo targeting `update_campaign` adds. Accepts the bare numeric geo
  target constant ID (`2276`) or the full resource name
  (`geoTargetConstants/2276`); discover IDs via `search_geo_targets`. Returns a
  preview to confirm via `confirm_and_apply`. Previously a campaign location
  could only be *added* (positively targeted), never excluded.
- `remove_geo_target`: remove a positively-targeted location from a campaign
  (`campaignCriterionOperation.remove`). This is the necessary counterpart to
  `exclude_geo_target`: for `LOCATION` criteria the criterion ID equals the geo
  target constant ID, so a campaign that already targets a location positively
  cannot also receive a negative criterion for the same ID — it would collide on
  `campaignCriteria/{campaign}~{id}`. Removing the existing positive criterion is
  the correct operation when trimming a country out of a multi-country campaign.
  Destructive — requires `confirmed_twice`.
- `set_conversion_action_primary_status`: mark a conversion action primary
  (`primaryForGoal = true` — counts in the Conversions column and feeds Smart
  Bidding) or secondary (`primaryForGoal = false` — observation only, excluded
  from the bidding signal). Emits a `conversionActionOperation.update` on
  `primaryForGoal` with the matching `updateMask`. The lever for demoting a
  value-0 signup event so it stops diluting a Maximize Conversions / Target CPA
  goal without losing its historical reporting.

### Changed

- `get_conversion_actions` now also selects `conversion_action.primary_for_goal`
  so callers can see (and verify) whether each action is primary or secondary.

## [0.5.2] - 2026-06-22

### Fixed

- Release workflow now publishes binaries. Two issues prevented any tagged
  release from producing assets: (1) a tag push creates only the git tag, not a
  GitHub Release, so `taiki-e/upload-rust-binary-action` failed with "release
  not found" — a `create-release` job now creates the release before the upload
  matrix runs; (2) the `aarch64-unknown-linux-gnu` target failed to build
  (`E0463: can't find crate for core`, std component not installed) and is
  dropped in favour of `aarch64-unknown-linux-musl`, which builds and is a
  portable static binary.

## [0.5.1] - 2026-06-22

### Fixed

- Release builds for every target. `reqwest` was declared with the `rustls-tls`
  feature but kept its default features, which include `default-tls`
  (native-tls → `openssl-sys`). `openssl-sys` is a C dependency that cannot
  cross-compile, so the release workflow failed on every non-host target since
  v0.4.1. Setting `default-features = false` leaves only `rustls-tls` (pure
  Rust); `openssl-sys` and `native-tls` are removed from the dependency tree.

## [0.5.0] - 2026-06-22

### Added

- `create_conversion_action`: draft a conversion action for server-side click
  uploads (`type = UPLOAD_CLICKS`, gclid-based offline conversion import). Built
  for creating "Signup"/"Activation" conversions whose gclid is uploaded
  server-side. Like the other write tools it returns a preview to confirm via
  `confirm_and_apply`; after apply, the numeric ID (for a
  `*_CONVERSION_ACTION_ID` env var) is the trailing segment of the returned
  `conversionAction` resource name. Parameters: `name`, optional `category`
  (default `SIGNUP`), `counting_type` (default `ONE_PER_CLICK`), and
  `click_through_lookback_window_days` (1-90, default 30). No value settings are
  attached, so valueless uploads are accepted.

## [0.4.1] - 2026-06-14

### Fixed

- `update_campaign` now resolves the campaign's real budget resource before a
  `daily_budget` update. Previously it reused the campaign ID as the budget ID
  (`campaignBudgets/{campaign_id}`), which targets a non-existent budget and
  fails with `RESOURCE_NOT_FOUND` — campaign budgets have their own distinct
  IDs. The resource name is resolved via
  `SELECT campaign.campaign_budget FROM campaign WHERE campaign.id = {id}`.
- `confirm_and_apply` now reports a `partialFailureError` (HTTP 200 with an
  embedded Google Ads error, e.g. code 3) as a failure instead of a false
  `"APPLIED"`. The audit log records `FAILED` and the plan is retained for
  retry. Affects both the mutate and apply-recommendation dispatch paths.
  Previously the partial failure was attached as metadata while the operation
  was reported as successful, masking the failure from callers.

### Added

- `tools::campaigns_write::resolve_campaign_budget_resource` — resolves a
  campaign's budget resource name via the API.
- `error::McpGoogleAdsError::PartialFailure` — carries the Google Ads
  `partialFailureError` payload so the underlying reason is visible to callers.

### Changed (BREAKING)

- `tools::campaigns_write::UpdateCampaignParams` gains a
  `budget_resource_name: Option<&str>` field, required when `daily_budget` is
  set. Existing callers must pass `None` (or a resolved resource name).

## [0.4.0] - 2026-06-12

### Added

- `models::AdRotationMode` enum (`OPTIMIZE`, `ROTATE_FOREVER`) with serde
  rename `"SCREAMING_SNAKE_CASE"`, matching the Google Ads REST API
  `AdGroup.ad_rotation_mode` enum values.
- `update_ad_group` accepts an optional `ad_rotation_mode` parameter and
  writes `adRotationMode` (with the matching update-mask entry) on the
  drafted `adGroupOperation`. `ROTATE_FOREVER` is what the Google Ads UI
  calls "Rotate indefinitely". Previously ad rotation could only be
  changed through the UI.

### Changed (BREAKING)

- `tools::ad_groups_write::update_ad_group` signature gains a trailing
  `ad_rotation_mode: Option<AdRotationMode>` argument. Existing callers
  must pass `None` to keep the previous behaviour. The "at least one
  field" validation message now lists `ad_rotation_mode`.

## [0.3.0] - 2026-05-26

The rationale behind the decisions in this release is recorded in
[`docs/decisions/0001-v0.3.0-fix-strategy.md`](docs/decisions/0001-v0.3.0-fix-strategy.md).

### Changed (BREAKING)

- `tools::ads_write::DraftRsaParams` now carries a `status: Option<AdStatus>`
  field. Callers constructing this struct must add `status: None` (or
  `status: Some(AdStatus::Paused)` for the previous behaviour).
- `tools::campaigns_write::DraftCampaignParams` now carries a
  `status: Option<AdStatus>` field. Same migration as above.
- `tools::ad_groups_write::create_ad_group` signature gains a trailing
  `status: Option<AdStatus>` argument. Existing v0.2.x callers that
  defaulted to `ENABLED` should pass `Some(AdStatus::Enabled)` to keep
  identical behaviour — the new default is `PAUSED` for consistency with
  the rest of the write tools.
- `tools::confirm::confirm_and_apply` now takes a `ConfirmApplyInput`
  struct instead of `(plan_id, dry_run)` positional args. The struct
  exposes the new `bypass_require_dry_run` and `confirmed_twice` opt-out
  flags for the strengthened safety guards.
- `safety.require_dry_run = true` is now a **hard guard**: calling
  `confirm_and_apply` with `dry_run = false` returns
  `Err(McpGoogleAdsError::DryRunRequired)` before any HTTP traffic.
  Previously it only emitted a cosmetic post-apply warning while applying
  anyway. Set `bypass_require_dry_run = true` for a one-shot override.
- `requires_double_confirm = true` is now also a hard guard. Plans with
  this flag require `confirmed_twice = true` or they return
  `Err(McpGoogleAdsError::DoubleConfirmRequired)`. Previously the flag
  was dead code.
- The cosmetic `warnings` field emitted after a successful apply has been
  removed. Successful applies are silent on this front; the new hard
  guards prevent the scenario that warning was trying to describe.

### Added

- `models::AdStatus` enum (`ENABLED`, `PAUSED`, `REMOVED`) with serde
  rename `"UPPERCASE"`, matching the Google Ads REST API enum values.
  Default is `PAUSED`.
- `models::NextActionHint` struct propagated through tool responses to
  tell agents how to continue a workflow via MCP — zero UI action
  required. Builder helpers: `enable_ad`, `enable_campaign`,
  `enable_ad_group`.
- `safety::preview::PlanDispatch` discriminator routing recommendation
  plans through the dedicated `recommendations:apply` /
  `recommendations:dismiss` RPCs instead of `googleAds:mutate`.
- `client::GoogleAdsClient::apply_recommendations` and
  `dismiss_recommendations` methods POSTing to the dedicated v23 RPCs.
- `client::VALID_MUTATE_OPERATION_KEYS` whitelist enforcing v23
  `MutateOperation.operation` oneof keys client-side. Unknown keys are
  rejected with a clear error message before any HTTP traffic via
  `GoogleAdsClient::validate_mutate_operations`.
- `client::GoogleAdsClient::with_base_url` constructor + the
  `GOOGLE_ADS_API_BASE_URL` env var, used by integration tests to point
  the client at a [`wiremock`](https://crates.io/crates/wiremock) mock.
- `status_after_apply` and `next_action_hint` fields surfaced in:
  - draft RSA preview + confirm_and_apply response
  - draft campaign preview + confirm_and_apply response
  - create ad group preview + confirm_and_apply response
  - create PMax campaign preview + confirm_and_apply response
- `status: Option<AdStatus>` MCP tool param exposed on
  `draft_responsive_search_ad`, `draft_campaign`, `create_ad_group`.
- `bypass_require_dry_run` and `confirmed_twice` MCP tool params on
  `confirm_and_apply`.
- 13 new wiremock-driven integration tests under `tests/` covering RSA
  status default + opt-in, recommendations routing, mutate whitelist,
  dry-run guard, double-confirm guard, PMax `start_paused` payload
  propagation, and ad group default consistency.

### Fixed

- **Bug 1** — `draft_responsive_search_ad` now exposes the `PAUSED`
  default in the response (`status_after_apply: "PAUSED"`) together
  with a `next_action_hint` describing how to flip the ad to `ENABLED`
  via the MCP `enable_entity` tool. Previously the default was hidden
  and no follow-up hint was provided.
- **Bug 2** — `apply_recommendation` and `dismiss_recommendation` now
  route through the dedicated `recommendations:apply` /
  `recommendations:dismiss` v23 RPCs. The v0.2.x code wrapped them in
  `MutateOperation` and hit `googleAds:mutate`, which returned 400
  because v23 has no `*RecommendationOperation` key on
  `MutateOperation.operation`.
- **Bug 3** — Cosmetic post-apply warning that lied about safety
  (`"Safety config has require_dry_run=true. Consider running with
  dry_run=true first."` emitted AFTER mutating) has been replaced by a
  hard guard returning `Err(DryRunRequired)` BEFORE any HTTP call.
- **Twin in `pmax.rs`** — `create_pmax_campaign(start_paused = false)`
  now actually sets `status: "ENABLED"` in the payload. Previously the
  param was accepted but ignored; the campaign always shipped `PAUSED`.
- **Twin in `campaigns_write.rs`** — campaign + ad-group creates now
  honour the `status` param consistently.
- **Twin in `ad_groups_write.rs`** — default status is now `PAUSED`
  (matches the rest of the write tools); v0.2.x defaulted to `ENABLED`
  as the only outlier.
- Dead `requires_double_confirm` branch is now wired as a real guard.

## [0.2.1] - 2026-05-20

Initial public release.
