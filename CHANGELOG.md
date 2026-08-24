# Changelog

All notable changes to `mcp-google-ads` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.13.0] - 2026-08-24

### Added

- `update_responsive_search_ad` edits an existing ad in place instead of
  forcing a remove-and-re-create. Changing a single headline used to cost the
  ad everything it had earned: the asset-level performance labels Google
  attaches to each headline and description, and the ad-level learning that a
  fresh ad starts over from. The server could only draft a new ad, so every
  copy change paid that price.

  The operation carries a field mask built from the fields the caller actually
  supplied. The ad keeps its ID, and a field left out is left alone — asking to
  rewrite the headlines does not clear the descriptions. An update naming no
  field at all is refused rather than applied as a silent no-op, because an
  update that quietly does nothing is indistinguishable from one that worked.

  Every bound is refused rather than truncated, and the rejection names both
  the field and the limit it broke: 3 to 15 headlines of 30 characters, 2 to 4
  descriptions of 90, an absolute http(s) final URL of 2048, and display paths
  of 15 — counted in characters, not bytes. `ad_id` must be a bare numeric id:
  passing a full resource name used to build a nonsensical URL and fail against
  the API with a message that pointed nowhere.

### Changed

- `draft_responsive_search_ad` and `update_responsive_search_ad` now share one
  set of headline and description validators rather than each carrying its own
  copy of the limits, so create and update cannot drift apart on a bound. The
  messages are unchanged. One nearly invisible difference: when an input breaks
  two rules at once, the error that surfaces may now be the other one.

## [0.12.0] - 2026-08-21

### Changed

- `rmcp` 0.16 to 3.1.4. The protocol version this server announces at handshake
  moves from `2025-03-26` to `2025-11-25`. The tool surface is untouched. All
  62 tools still list, and a `tools/call` round-trips exactly as before.

- `rmcp` and `rmcp-macros` are pinned to the same exact version. `rmcp` asks
  for its macro crate through a caret range even though the two ship in
  lockstep, so Cargo was free to pair a 1.4.0 runtime with a 1.8.0 macro crate.
  It did, and the build came apart into sixty copies of `cannot find function
  schema_for_input`, one per tool, every one of them pointing at this crate
  instead of at the dependency that was actually wrong. Pinning both stops
  Cargo from choosing a mismatched pair, and a partial bump now fails during
  resolution with a message that names the real culprit.

- `get_info` is built through rmcp's constructors, since `ServerInfo`,
  `ServerCapabilities` and `Implementation` are now `#[non_exhaustive]`. Those
  constructors read their defaults from inside rmcp, so the explicit
  `server_info` matters: without it the server introduces itself to every MCP
  client as `rmcp`. A test pins the announced name, version, description,
  protocol version, tools capability and instructions.

### Security

- GHSA-89vp-x53w-74fx on `rmcp` is cleared by leaving 0.16 behind. As 0.10.1
  already noted, the flaw was never reachable here, since it sits in the
  Streamable HTTP server transport and only `transport-io` is enabled.

## [0.11.0] - 2026-08-20

### Fixed

- Tool parameters that a tool does not define are now rejected instead of
  ignored. serde skips unknown fields by default, so a caller passing `days: 2`
  to a tool that only understands `date_range_start`/`date_range_end` got a
  successful answer computed over the *default* window, with nothing to signal
  the mistake — the reply looked exactly like the one that was asked for.
  That silently produced a wrong reading of a live campaign: two windows
  believed to be 14 and 2 days were both 30, and the comparison drawn between
  them was therefore meaningless. All 46 tool parameter structs now carry
  `deny_unknown_fields`, and the error names the offending key alongside the
  ones the tool accepts.

### Added

- `get_search_terms` takes a `limit` (1 to 10000, default 200). The row cap was
  hardcoded at 200, which is both unaskable-for and unreadable: the report is
  unbounded by nature — a broad campaign produces thousands of distinct queries
  — and 200 rows of it run to well over 100 KB. An out-of-range value is
  refused client-side rather than clamped, so the caller is never quietly
  served something other than what they asked for.

## [0.10.1] - 2026-08-20

### Security

- `rustls-webpki` 0.103.12 to 0.103.14 (GHSA-82j2-j2ch-gfr8) and `quinn-proto`
  0.11.14 to 0.11.17 (GHSA-4w2j-m93h-cj5j), both transitive. Neither advisory
  was reachable here: `quinn` is resolved in the lockfile but never compiled
  into the binary, and the `rustls-webpki` panic needs CRL parsing, which
  rustls does not perform unless a revocation check is configured. Bumped
  anyway, since both are patch releases and the shipped binaries otherwise
  carry the flagged versions.

  The third open advisory, GHSA-89vp-x53w-74fx on `rmcp`, is left alone. It
  affects the Streamable HTTP server transport; this server enables only
  `transport-io` and serves over stdio, so the vulnerable code is not compiled.
  Its fix lands in rmcp 1.4.0, a major jump from 0.16 that deserves its own
  change rather than riding along with a security patch.

## [0.10.0] - 2026-08-19

### Added

- `apply_recommendation` accepts the `apply_parameters` oneof. Google's
  `ApplyRecommendationOperation` carries 22 type-specific parameter variants,
  and several recommendation types cannot be applied without the matching one —
  the operation has to say *what* to apply: which assets to attach for
  `SITELINK_ASSET`, which amount for `CAMPAIGN_BUDGET`. The server only ever
  sent a bare `resourceName`, so those types were unreachable through the MCP.
  Pass a single-key object naming one variant, for example
  `{"campaignBudget": {"newBudgetAmountMicros": "15000000"}}`. Omitting it keeps
  the previous behaviour: Google applies the values it recommended.
  The payload is checked client-side against the documented key list — a oneof
  takes exactly one key, and an undocumented key is refused before any HTTP
  traffic rather than spent on a round trip that fails opaquely.

### Fixed

- Partial failures now surface what actually went wrong. `recommendations:apply`
  and `googleAds:mutate` report per-operation errors as HTTP 200 with a
  `partialFailureError` body, and everything actionable in it — the request id,
  the failing field path, the error code — lives in `details`, which the error's
  `Display` dropped. Callers saw only the top-level sentence.
  This mattered most in the case that produced it: when Google answers with an
  error code the negotiated API version cannot name, it degrades the code to
  `UNKNOWN` and the message to "The error code is not in this version.", which
  on its own is a dead end. That case is now spelled out as a version mismatch,
  with the API version that answered and the request id to quote to support.

## [0.9.0] - 2026-08-19

### Added

- Negative keyword lists (Google Ads "shared sets") are now addressable. The
  server previously exposed only campaign-level negatives, so the only way to
  apply one exclusion set across N campaigns was to write the same keywords N
  times — 40 words across 5 campaigns meant 200 criteria to keep in sync by
  hand, and every new campaign started unprotected. Eight tools close that gap:
  `list_negative_keyword_lists` and `get_negative_keyword_list` (read),
  `create_negative_keyword_list`, `add_to_negative_keyword_list`,
  `remove_from_negative_keyword_list`, `attach_negative_keyword_list`,
  `detach_negative_keyword_list` and `delete_negative_keyword_list` (write).
  The three underlying `MutateOperation` keys — `sharedSetOperation`,
  `sharedCriterionOperation`, `campaignSharedSetOperation` — were already in
  the client whitelist; nothing but the tool surface was missing.
  `create_negative_keyword_list` emits the set, its keywords and its campaign
  links as ONE atomic mutate using the temporary resource ID `-1` (the same
  technique `draft_campaign` uses for budget→campaign→ad group). Splitting them
  would allow a list attached to campaigns while holding none of its keywords —
  campaigns that read as protected and are not.
  Everything that strips live exclusions (`remove_from_…`, `detach_…`,
  `delete_…`) is flagged `requires_double_confirm`.
  Input keywords are de-duplicated case-insensitively before submission,
  because a single repeat would fail the whole atomic batch; the count dropped
  is reported as `duplicates_dropped` so the edit is never silent. Shared set
  and campaign IDs are validated as bare integers — they are interpolated into
  both GAQL and resource names.
  `get_negative_keywords` keeps returning campaign-level negatives only; its
  description now says so and points at the new tools, since a campaign's
  effective exclusions are the union of both mechanisms.

- Demographic targeting is now reachable from the server: `exclude_demographics`,
  `remove_demographic_criterion` and `get_demographics`. Household income band,
  age bracket and gender exclusions previously had no tool at all, so every
  campaign built through this server shipped serving to all incomes and all ages
  — two mandatory items of the clinic playbook silently skipped, discoverable
  only by noticing their absence later. The gap was being closed by hand-rolled
  REST calls against the same credentials, which is exactly the workaround a
  server exists to remove.
  These are `AdGroupCriterion` rows and are **ad-group scoped** — Google has no
  campaign-level demographic exclusion — so `exclude_demographics` takes a list
  of ad group IDs and writes one negative criterion per (ad group × tier).
  Criteria are created **by type**, never by ID: Google resolves the fixed
  criterion ID itself. The ID table on the enums exists only for the removal
  path, where `adGroupCriteria/{ad_group}~{id}` has to be built by hand.
  Guardrails, each of which prevents a failure that is invisible until it costs
  money: excluding every tier of a dimension is rejected (it would stop the ad
  group serving to anyone, which Google accepts without complaint); ad groups and
  tiers are de-duplicated, because a repeat collides on the shared resource name
  and fails the whole batch; and a batch over 500 criteria is a hard error rather
  than a truncation, since a partially applied exclusion reads as done.
  `remove_demographic_criterion` is `requires_double_confirm` — dropping a
  negative row silently re-opens the ad group to a tier the advertiser
  deliberately excluded. It also handles the collision case: a tier already
  present as an explicit *positive* row must be removed before the negative one
  can be created, as both share a resource name.
  The enums carry explicit `#[serde(rename)]` on every variant rather than
  `rename_all = "SCREAMING_SNAKE_CASE"`. Serde does not insert a separator before
  a digit, so the derived form of `IncomeRange0_50` is `INCOME_RANGE0_50` — a
  spelling Google rejects. A table test pins the exact JSON spelling of all
  seventeen variants against literals so the two representations cannot drift.
  Note `Gender` is deliberately asymmetric: `GENDER_FEMALE` on the JSON surface,
  bare `FEMALE` on the wire.

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

- `confirm_and_apply` gains `exempt_policy_violations` (default `false`).
  Google rejects many creates with `POLICY_ERROR` and an `isExemptible: true`
  violation; such an operation is accepted when resubmitted carrying
  `exemptPolicyViolationKeys`, which is exactly what the Google Ads web UI does
  automatically. Without it, whole categories of legitimate keywords were
  simply unaddable through this server: every medical/health term trips
  `HEALTH_IN_PERSONALIZED_ADS` ("sensitive health information"), and
  contraception terms additionally trip `BIRTH_CONTROL`. With the flag set, a
  rejected mutate is retried **once** with an exemption key attached to each
  offending operation, and the response reports what was exempted under
  `policy_exemptions_requested` — an exemption is an assertion of eligibility
  that Google still reviews, so it is never silent and never automatic.
  Verified against the live v23 API: only `adGroupCriterionOperation` accepts
  `exemptPolicyViolationKeys`; `campaignCriterionOperation` has no such field,
  and `adGroupAdOperation.policyValidationParameter` rejects responsive search
  ads with `UNSUPPORTED_AD_TYPE_FOR_EXEMPT_POLICY_VIOLATION_KEYS`. Violations on
  operations that cannot be exempted are reported as such instead of being
  retried pointlessly.

- `safety::policy_exemption`: parses `policyViolationDetails` out of a
  `GoogleAdsFailure`, maps each violation to its `mutate_operations` index
  (treating an absent index as 0, since protobuf JSON omits zero values), and
  attaches deduplicated exemption keys to the operations that support them.

### Changed

- The REST surface moves to Google Ads **v25**. It is the newest version the
  API answers on: probing `customers:search` read-only across v22..v26 returns
  rows for v22-v25 and "Method not found" for v26. Verified afterwards by
  driving the built server over stdio against a live account with no base-URL
  override.

  The `MutateOperation` whitelist follows the oneof rather than the previous
  list. The feed-based extension operations (`feedOperation`,
  `campaignFeedOperation`, `extensionFeedItemOperation`, the
  `*ExtensionSettingOperation` family, …) are gone: they went away with the
  sunset of feed-based extensions, and `validateOnly` answers "Unknown name"
  for every one of them on v23 as well as v25 — they were already dead entries,
  not a v25 regression. Extensions here go through `assetOperation` and
  `campaignAssetOperation`, neither of which is affected.
  `recommendationSubscriptionOperation` is added; the API accepts it.

### Fixed

- `update_campaign` could not change a bidding strategy at all. The field mask
  named the parent message (`targetRoas`, `maximizeConversions`, …), which the
  API rejects with `FIELD_HAS_SUBFIELDS`, so every target ROAS and target CPA
  update failed with a bare 400. Each strategy now masks its leaf
  (`targetRoas.targetRoas`, `maximizeConversions.targetCpaMicros`,
  `manualCpc.enhancedCpcEnabled`, …), and `TARGET_SPEND` / `MAXIMIZE_CLICKS`
  gained the arm they never had. Maximize Conversions and Maximize Conversion
  Value always send their target field — `0` being how the API expresses "no
  target" — so a leaf always exists to mask. Found independently by two
  contributors against live accounts.

- `create_pmax_campaign` now sets `campaign_budget.explicitly_shared = false`.
  Budgets default to shared server-side, and Performance Max rejects a shared
  budget with `BIDDING_STRATEGY_TYPE_INCOMPATIBLE_WITH_SHARED_BUDGET`.
  `draft_campaign` got this fix in 5670d2d; the PMax path was missed.

- `create_pmax_campaign` now sets
  `campaign.contains_eu_political_advertising = DOES_NOT_CONTAIN_EU_POLITICAL_ADVERTISING`.
  The field is required on every campaign create, and omitting it failed the
  entire mutate with `fieldError=REQUIRED`. Together with the budget fix, these
  two meant `create_pmax_campaign` could not create any Performance Max
  campaign — every call failed at `confirm_and_apply`.

- Error responses now include the underlying `GoogleAdsFailure` instead of
  discarding it. Google puts a generic `"Request contains an invalid argument."`
  at the top level and the actual cause — error code, field path, offending
  text, policy details — in `error.details`, which this server parsed into
  `McpGoogleAdsError::GoogleAds.details` and then never rendered. Every
  rejection therefore looked identical, and a policy block was indistinguishable
  from a malformed field. Responses now carry a compact `failure_details`
  summary (capped at 25 errors, with a `truncated` count beyond that), and
  policy rejections carry `policy_violations` naming each policy and the exact
  text that tripped it.

- Error hints now also match against the `GoogleAdsFailure` details, not just
  the top-level message. Google's generic `"Request contains an invalid
  argument."` never contained the error code the hints key on, so hints could
  effectively only fire for GAQL query errors. Added a hint for the structured
  snippet header, which Google rejects with a bare
  `stringFormatError: INVALID_FORMAT` that never names the valid values.

- `create_structured_snippets` no longer rejects valid non-English headers. The
  header whitelist was hardcoded to the 13 English values, so it refused
  `Serviços` — the header the Portuguese accounts in this MCC actually run on —
  and every other localized form, making the tool unusable outside English
  accounts. The accepted set is language-specific and is *not* a literal
  translation (verified live: `Neighborhoods` is valid but `Bairros` is not;
  `Serviços` is valid but the unaccented `Servicos` is not), so a client-side
  list can never be authoritative. The list is now guidance: an unrecognized
  header is passed to Google, which validates it, and the preview carries a
  `header_note` so a typo is still caught before applying. The known-good set
  gained the twelve verified pt-BR headers. An empty header is still rejected.

- Removed two debug writes in `client.mutate` that dumped every request body and
  error body to hardcoded `/tmp/mcp-google-ads-last-{request,error}.json`. They
  silently no-opped on Windows and, on Unix, wrote full mutate payloads to a
  world-readable path on every call.

### Removed

- `create_custom_audience`. It emitted a `customAudienceOperation` inside
  `googleAds:mutate`, a key that is in no version of the oneof — `validateOnly`
  answers "Unknown name" on v23 and v25 alike, custom audiences having their
  own service off the mutate path. The client's whitelist rejected it before
  any HTTP call, so the tool failed on every invocation and no call ever
  reached Google. A test now scans the tool sources and asserts every emitted
  operation key is one the client will send, so a tool wired to a non-existent
  operation fails in CI rather than in an agent's hands.

### Changed (BREAKING)

- `tools::confirm::ConfirmApplyInput` gains an `exempt_policy_violations: bool`
  field. Rust library callers constructing it literally should add
  `..Default::default()`. MCP-tool callers are unaffected — the new parameter is
  optional and defaults to `false`, preserving existing behaviour.

## [0.8.1] - 2026-08-19

### Fixed

- `run_gaql` with `format=table` or `format=csv` rendered blank cells for every
  field whose path contains a multi-word segment. GAQL SELECT clauses are written
  in snake_case (`metrics.cost_micros`, `ad_group_criterion.keyword.text`) while
  the API answers in camelCase (`{"metrics": {"costMicros": …}}`), and the field
  resolver only ever tried the literal snake_case key. Single-word segments
  (`campaign.name`, `metrics.clicks`) resolved, everything else came back empty,
  so the output looked structurally valid while silently dropping keyword text,
  match types, final URLs, quality-score components, resource names and
  `cost_micros`.

  The failure mode was worse than a missing column. A `change_event` query
  rendered as a table printed blank rows, which reads as "no recent changes on
  the account" — the opposite of the truth. That report is what tells a caller
  whether a change has already been applied, so a silent blank could lead to the
  same mutation being applied twice.

  Each path segment is now looked up literally first, then in its camelCase form,
  so snake_case payloads keep resolving and the fallback only engages when the
  literal key is absent. `format=json` was never affected.

  The existing tests missed this because their fixtures were written in
  snake_case, a shape the API never returns. The new tests use payloads copied
  from real API responses.

## [0.8.0] - 2026-08-09

### Added

- `update_keyword_final_url`: change the landing page
  (`ad_group_criterion.final_urls`) of an existing keyword in place, via an
  `adGroupCriterionOperation.update` with an `updateMask` scoped to `finalUrls`.
  Unlike a `remove_keywords` + `draft_keywords`, an in-place update preserves the
  keyword's quality-score history — the reason it is a distinct tool. This closes
  the gap where the only way to re-route a keyword was to destroy and re-create
  it, resetting the landing-page quality signal. Final URL validated as an
  absolute `http(s)` URL bounded to 2048 characters. Returns a preview confirmed
  via `confirm_and_apply`. Brings the tool count to 50.

## [0.7.0] - 2026-07-26

### Added

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
