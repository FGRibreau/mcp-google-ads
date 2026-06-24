pub mod auth;
pub mod client;
pub mod config;
pub mod error;
pub mod gaql;
pub mod models;
pub mod safety;
pub mod tools;

use client::GoogleAdsClient;
use config::Config;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct GoogleAdsMcp {
    config: Config,
    tool_router: ToolRouter<Self>,
}

// ── Parameter structs ───────────────────────────────────────────────────

/// Parameters for tools that accept an optional customer ID and optional date range.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DateRangeParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// Start date YYYY-MM-DD
    pub date_range_start: Option<String>,
    /// End date YYYY-MM-DD
    pub date_range_end: Option<String>,
}

/// Parameters for tools that accept only an optional customer ID.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CustomerIdParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
}

/// Parameters for run_gaql tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RunGaqlParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// GAQL query string (e.g. SELECT campaign.id, campaign.name FROM campaign)
    pub query: String,
    /// Output format: json (default), table, or csv
    pub format: Option<String>,
}

/// Parameters for search_geo_targets tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SearchGeoParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// Location name to search for (e.g. 'Paris', 'France', 'New York')
    pub query: String,
}

// ── Write tool parameter structs ────────────────────────────────────────

/// Parameters for drafting a new campaign.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DraftCampaignToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// Name for the new campaign.
    pub campaign_name: String,
    /// Daily budget in dollars (e.g. 50.0).
    pub daily_budget: f64,
    /// Bidding strategy (e.g. MAXIMIZE_CONVERSIONS, TARGET_CPA, MANUAL_CPC).
    pub bidding_strategy: String,
    /// Target CPA in dollars, if using TARGET_CPA or MAXIMIZE_CONVERSIONS.
    pub target_cpa: Option<f64>,
    /// Target ROAS, if using TARGET_ROAS or MAXIMIZE_CONVERSION_VALUE.
    pub target_roas: Option<f64>,
    /// Channel type (default "SEARCH").
    pub channel_type: Option<String>,
    /// Name for the default ad group.
    pub ad_group_name: Option<String>,
    /// Optional keywords to add to the ad group.
    pub keywords: Option<Vec<tools::campaigns_write::KeywordInput>>,
    /// Geographic target IDs for campaign targeting.
    pub geo_target_ids: Vec<String>,
    /// Language IDs for campaign targeting.
    pub language_ids: Vec<String>,
    /// Lifecycle status for the new campaign + ad group. Defaults to `PAUSED`
    /// for safety; pass `ENABLED` to start serving immediately.
    pub status: Option<models::AdStatus>,
}

/// Parameters for updating an existing campaign.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct UpdateCampaignToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// The campaign ID to update.
    pub campaign_id: String,
    /// New bidding strategy (e.g. MAXIMIZE_CONVERSIONS, TARGET_CPA, MANUAL_CPC).
    pub bidding_strategy: Option<String>,
    /// Target CPA in dollars.
    pub target_cpa: Option<f64>,
    /// Target ROAS.
    pub target_roas: Option<f64>,
    /// New daily budget in dollars.
    pub daily_budget: Option<f64>,
    /// Geographic target IDs to add.
    pub geo_target_ids: Option<Vec<String>>,
    /// Language IDs to add.
    pub language_ids: Option<Vec<String>>,
}

/// Parameters for drafting a Responsive Search Ad.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DraftRsaToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// The ad group ID to create the ad in.
    pub ad_group_id: String,
    /// Headlines (3-15, max 30 chars each).
    pub headlines: Vec<String>,
    /// Descriptions (2-4, max 90 chars each).
    pub descriptions: Vec<String>,
    /// Final URL for the ad.
    pub final_url: String,
    /// Display URL path 1.
    pub path1: Option<String>,
    /// Display URL path 2.
    pub path2: Option<String>,
    /// Lifecycle status for the new ad. One of `ENABLED`, `PAUSED`, `REMOVED`.
    /// Defaults to `PAUSED` for safety — the response will include a
    /// `next_action_hint` describing how to call `enable_entity` via MCP.
    pub status: Option<models::AdStatus>,
}

/// Parameters for drafting keyword additions.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DraftKeywordsToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// The ad group ID to add keywords to.
    pub ad_group_id: String,
    /// Keywords with match types to add.
    pub keywords: Vec<tools::campaigns_write::KeywordInput>,
}

/// Parameters for adding negative keywords.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct AddNegativeKeywordsToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// The campaign ID to add negative keywords to.
    pub campaign_id: String,
    /// Keywords to add as negatives.
    pub keywords: Vec<String>,
    /// Match type for all keywords (default "EXACT"). One of: EXACT, PHRASE, BROAD.
    pub match_type: Option<String>,
}

/// Parameters for excluding a geographic location from a campaign.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ExcludeGeoTargetToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// The campaign ID to exclude the location from.
    pub campaign_id: String,
    /// Geo target constant ID to exclude — bare numeric ("2276") or full
    /// resource name ("geoTargetConstants/2276"). Find IDs via search_geo_targets.
    pub geo_target_id: String,
}

/// Parameters for removing a positive geographic target from a campaign.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RemoveGeoTargetToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// The campaign ID to remove the location target from.
    pub campaign_id: String,
    /// Geo target constant ID currently targeted — bare numeric ("2276") or full
    /// resource name ("geoTargetConstants/2276").
    pub geo_target_id: String,
}

/// Parameters for drafting sitelink extensions.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DraftSitelinksToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// The campaign ID to add sitelinks to.
    pub campaign_id: String,
    /// Sitelink definitions.
    pub sitelinks: Vec<tools::extensions_write::SitelinkInput>,
}

/// Parameters for creating callout extensions.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CreateCalloutsToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// The campaign ID to add callouts to.
    pub campaign_id: String,
    /// Callout texts (max 25 chars each).
    pub callouts: Vec<String>,
}

/// Parameters for creating structured snippet extensions.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CreateSnippetsToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// The campaign ID to add snippets to.
    pub campaign_id: String,
    /// Snippet header (e.g. Brands, Types, Amenities).
    pub header: String,
    /// Snippet values.
    pub values: Vec<String>,
}

/// Parameters for creating a conversion action for server-side click uploads.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CreateConversionActionToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// Name of the conversion action (e.g. "Activation").
    pub name: String,
    /// Conversion category (default "SIGNUP"). E.g. SIGNUP, LEAD, DEFAULT.
    pub category: Option<String>,
    /// Counting type (default "ONE_PER_CLICK"). One of ONE_PER_CLICK, MANY_PER_CLICK.
    pub counting_type: Option<String>,
    /// Click-through attribution window in days, 1-90 (default 30).
    pub click_through_lookback_window_days: Option<i64>,
}

/// Parameters for setting a conversion action's primary/secondary status.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SetConversionActionPrimaryToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// The conversion action ID (numeric — the trailing segment of the
    /// conversionAction resource name).
    pub conversion_action_id: String,
    /// true = primary (counts in the Conversions column and Smart Bidding);
    /// false = secondary (observation only, excluded from the bidding signal).
    pub primary: bool,
}

/// Parameters for pausing, enabling, or removing an entity.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct EntityActionParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// Entity type: campaign, ad_group, ad, or keyword.
    pub entity_type: String,
    /// The entity ID.
    pub entity_id: String,
}

/// Parameters for confirming and applying a change plan.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ConfirmApplyParams {
    /// The plan ID returned from a draft/preview operation.
    pub plan_id: String,
    /// If true (default), returns a preview without executing. Set to false to apply changes.
    pub dry_run: Option<bool>,
    /// One-shot opt out of the `require_dry_run` safety guard for THIS apply.
    /// Defaults to `false`. Does NOT mutate the global config. Use this only
    /// when you explicitly know you want to apply without a prior dry-run.
    pub bypass_require_dry_run: Option<bool>,
    /// Acknowledgement for plans flagged `requires_double_confirm`. Without
    /// `confirmed_twice=true`, those plans return an error and do nothing.
    pub confirmed_twice: Option<bool>,
}

/// Parameters for creating a new ad group.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CreateAdGroupToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// Campaign ID to create the ad group in.
    pub campaign_id: String,
    /// Name for the new ad group.
    pub ad_group_name: String,
    /// Optional CPC bid in micros (1 dollar = 1_000_000 micros).
    pub cpc_bid_micros: Option<i64>,
    /// Lifecycle status. Defaults to `PAUSED` for safety (was `ENABLED` in
    /// v0.2.x — corrected for consistency with the other write tools).
    pub status: Option<models::AdStatus>,
}

/// Parameters for updating an ad group.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct UpdateAdGroupToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// The ad group ID to update.
    pub ad_group_id: String,
    /// New name for the ad group.
    pub name: Option<String>,
    /// New CPC bid in micros.
    pub cpc_bid_micros: Option<i64>,
    /// Ad rotation mode: OPTIMIZE (serve best-performing ad) or
    /// ROTATE_FOREVER ("Rotate indefinitely" in the Google Ads UI).
    pub ad_rotation_mode: Option<models::AdRotationMode>,
}

/// Parameters for keyword discovery.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DiscoverKeywordsToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// Seed keywords to generate ideas from.
    pub seed_keywords: Vec<String>,
}

/// Parameters for keyword forecasts.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct KeywordForecastsToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// Keywords to get forecasts for.
    pub keyword_texts: Vec<String>,
}

// ── Phase 5 parameter structs ────────────────────────────────────────

/// Parameters for creating a Performance Max campaign.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CreatePmaxCampaignToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// Campaign name.
    pub campaign_name: String,
    /// Daily budget in dollars.
    pub daily_budget: f64,
    /// Bidding strategy (e.g. MAXIMIZE_CONVERSIONS, MAXIMIZE_CONVERSION_VALUE).
    pub bidding_strategy: String,
    /// Final URLs for the asset group.
    pub final_urls: Vec<String>,
    /// Headlines (3-15, max 30 chars each).
    pub headlines: Vec<String>,
    /// Long headlines (1-5, max 90 chars each).
    pub long_headlines: Vec<String>,
    /// Descriptions (2-5, max 90 chars each).
    pub descriptions: Vec<String>,
    /// Business name (max 25 chars).
    pub business_name: String,
    /// Geographic target IDs.
    pub geo_target_ids: Vec<String>,
    /// If true, campaign starts PAUSED (default true).
    pub start_paused: Option<bool>,
}

/// Parameters for creating a custom audience.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CreateCustomAudienceToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// Name for the audience.
    pub audience_name: String,
    /// Audience type: WEBSITE_VISITORS or CUSTOMER_MATCH.
    pub audience_type: String,
    /// URL patterns or rules for the audience.
    pub urls_or_rules: Vec<String>,
}

/// Parameters for adding audience targeting to a campaign.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct AddAudienceTargetingToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// Campaign ID to add targeting to.
    pub campaign_id: String,
    /// Audience/user list ID.
    pub audience_id: String,
    /// Targeting mode: TARGETING or OBSERVATION.
    pub targeting_mode: String,
}

/// Parameters for creating a portfolio bidding strategy.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CreatePortfolioBiddingToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// Strategy name.
    pub name: String,
    /// Strategy type: TARGET_CPA, TARGET_ROAS, or TARGET_IMPRESSION_SHARE.
    pub strategy_type: String,
    /// Target CPA in dollars (required for TARGET_CPA).
    pub target_cpa: Option<f64>,
    /// Target ROAS (required for TARGET_ROAS).
    pub target_roas: Option<f64>,
}

/// Parameters for updating a keyword bid.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct UpdateKeywordBidToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// Ad group ID containing the keyword.
    pub ad_group_id: String,
    /// Criterion ID of the keyword.
    pub criterion_id: String,
    /// Current bid in dollars (for safety check).
    pub current_bid: f64,
    /// New bid in dollars.
    pub new_bid: f64,
}

/// Parameters for uploading an image asset.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct UploadImageAssetToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// Name for the asset.
    pub asset_name: String,
    /// Base64-encoded image data.
    pub image_data_base64: String,
}

/// Parameters for uploading a text asset.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct UploadTextAssetToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// Name for the asset.
    pub asset_name: String,
    /// Text content for the asset.
    pub text_content: String,
}

/// Parameters for setting campaign ad schedule.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SetCampaignScheduleToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// Campaign ID to set schedule for.
    pub campaign_id: String,
    /// Schedule entries (day, start/end times).
    pub schedules: Vec<tools::scheduling::ScheduleEntry>,
}

/// Parameters for applying or dismissing a recommendation.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RecommendationActionToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// Recommendation resource ID.
    pub recommendation_id: String,
}

/// Parameters for removing a campaign asset (extension).
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RemoveExtensionToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// The campaign ID the extension is linked to.
    pub campaign_id: String,
    /// The asset ID of the extension.
    pub asset_id: String,
    /// Extension type: SITELINK, CALLOUT, or STRUCTURED_SNIPPET.
    pub field_type: String,
}

/// Parameters for removing keywords from an ad group.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RemoveKeywordsToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// The ad group ID containing the keywords.
    pub ad_group_id: String,
    /// Criterion IDs to remove.
    pub criterion_ids: Vec<String>,
}

/// Parameters for removing negative keywords from a campaign.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RemoveNegativeKeywordsToolParams {
    /// Customer ID (e.g. 123-456-7890). Defaults to configured customer_id.
    pub customer_id: Option<String>,
    /// The campaign ID containing the negative keywords.
    pub campaign_id: String,
    /// Criterion IDs to remove.
    pub criterion_ids: Vec<String>,
}

// ── Tool router ─────────────────────────────────────────────────────────

#[rmcp::tool_router]
impl GoogleAdsMcp {
    /// Check the health of the MCP Google Ads server and its configuration.
    #[tool(description = "Check the health of the MCP Google Ads server and its configuration")]
    async fn health_check(&self) -> String {
        let mut status = Vec::new();
        status.push("MCP Google Ads Server: OK".to_string());
        status.push(format!(
            "Customer ID: {}",
            if self.config.ads.customer_id.is_empty() {
                "not configured"
            } else {
                &self.config.ads.customer_id
            }
        ));
        status.push(format!(
            "Developer token: {}",
            if self.config.ads.developer_token.is_empty() {
                "not configured"
            } else {
                "configured"
            }
        ));
        status.push(format!(
            "Credentials file: {}",
            if self.config.google.credentials_path.exists() {
                "found"
            } else {
                "not found"
            }
        ));
        status.push(format!(
            "Safety - dry run required: {}",
            self.config.safety.require_dry_run
        ));
        status.push(format!(
            "Safety - max daily budget: {:.2}",
            self.config.safety.max_daily_budget
        ));
        status.join("\n")
    }

    // ── Accounts ────────────────────────────────────────────────────────

    #[tool(
        description = "List all accessible Google Ads accounts. If a Manager (MCC) account is configured, lists all sub-accounts."
    )]
    async fn list_accounts(&self) -> String {
        let customer_id = self.resolve_mcc_or_customer_id();
        self.run_tool(|client| async move {
            tools::accounts::list_accounts(&client, &customer_id).await
        })
        .await
    }

    #[tool(
        description = "Get detailed information about a Google Ads account (currency, timezone, status)."
    )]
    async fn get_account_info(&self, Parameters(params): Parameters<CustomerIdParams>) -> String {
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        self.run_tool(
            |client| async move { tools::accounts::get_account_info(&client, &cid).await },
        )
        .await
    }

    // ── Campaigns ───────────────────────────────────────────────────────

    #[tool(
        description = "Get campaign-level performance metrics (impressions, clicks, cost, conversions, CTR, CPC, CPA). Defaults to last 30 days if no dates given."
    )]
    async fn get_campaign_performance(
        &self,
        Parameters(params): Parameters<DateRangeParams>,
    ) -> String {
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let ds = params.date_range_start;
        let de = params.date_range_end;
        self.run_tool(|client| async move {
            tools::campaigns::get_campaign_performance(&client, &cid, ds.as_deref(), de.as_deref())
                .await
        })
        .await
    }

    // ── Ads ─────────────────────────────────────────────────────────────

    #[tool(
        description = "Get ad-level performance metrics including headlines, descriptions, and final URLs. Defaults to last 30 days if no dates given."
    )]
    async fn get_ad_performance(&self, Parameters(params): Parameters<DateRangeParams>) -> String {
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let ds = params.date_range_start;
        let de = params.date_range_end;
        self.run_tool(|client| async move {
            tools::ads::get_ad_performance(&client, &cid, ds.as_deref(), de.as_deref()).await
        })
        .await
    }

    // ── Keywords ────────────────────────────────────────────────────────

    #[tool(
        description = "Get keyword-level performance metrics including quality score, match type, CPC, and conversions."
    )]
    async fn get_keyword_performance(
        &self,
        Parameters(params): Parameters<DateRangeParams>,
    ) -> String {
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let ds = params.date_range_start;
        let de = params.date_range_end;
        self.run_tool(|client| async move {
            tools::keywords::get_keyword_performance(&client, &cid, ds.as_deref(), de.as_deref())
                .await
        })
        .await
    }

    #[tool(
        description = "Get search terms report showing actual user queries that triggered your ads. Defaults to last 30 days. Returns top 200 by clicks."
    )]
    async fn get_search_terms(&self, Parameters(params): Parameters<DateRangeParams>) -> String {
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let ds = params.date_range_start;
        let de = params.date_range_end;
        self.run_tool(|client| async move {
            tools::keywords::get_search_terms(&client, &cid, ds.as_deref(), de.as_deref()).await
        })
        .await
    }

    #[tool(description = "Get all campaign-level negative keywords.")]
    async fn get_negative_keywords(
        &self,
        Parameters(params): Parameters<CustomerIdParams>,
    ) -> String {
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        self.run_tool(|client| async move {
            tools::keywords::get_negative_keywords(&client, &cid).await
        })
        .await
    }

    // ── Extensions (read) ──────────────────────────────────────────────

    #[tool(
        description = "List campaign-level extensions (sitelinks, callouts, structured snippets). Returns up to 500 non-removed extensions."
    )]
    async fn list_extensions(&self, Parameters(params): Parameters<CustomerIdParams>) -> String {
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        self.run_tool(
            |client| async move { tools::extensions::list_extensions(&client, &cid).await },
        )
        .await
    }

    // ── Reporting ───────────────────────────────────────────────────────

    #[tool(
        description = "Execute an arbitrary GAQL (Google Ads Query Language) query. Supports json, table, and csv output formats."
    )]
    async fn run_gaql(&self, Parameters(params): Parameters<RunGaqlParams>) -> String {
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let fmt = params.format.unwrap_or_else(|| "json".to_string());
        let query = params.query;
        self.run_tool(|client| async move {
            tools::reporting::run_gaql(&client, &cid, &query, &fmt).await
        })
        .await
    }

    // ── Geo ─────────────────────────────────────────────────────────────

    #[tool(
        description = "Search for geographic target constants by name. Useful for finding location IDs for geo-targeting."
    )]
    async fn search_geo_targets(&self, Parameters(params): Parameters<SearchGeoParams>) -> String {
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let query = params.query;
        self.run_tool(|client| async move {
            tools::geo::search_geo_targets(&client, &cid, &query).await
        })
        .await
    }

    #[tool(
        description = "Get geographic performance data showing metrics broken down by location."
    )]
    async fn get_geo_performance(&self, Parameters(params): Parameters<DateRangeParams>) -> String {
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let ds = params.date_range_start;
        let de = params.date_range_end;
        self.run_tool(|client| async move {
            tools::geo::get_geo_performance(&client, &cid, ds.as_deref(), de.as_deref()).await
        })
        .await
    }

    // ── Ad Groups (write) ────────────────────────────────────────────────

    #[tool(
        description = "Draft a new ad group in an existing campaign. Defaults to PAUSED — pass status='ENABLED' to start serving immediately. Returns a preview — call confirm_and_apply to execute."
    )]
    async fn create_ad_group(
        &self,
        Parameters(params): Parameters<CreateAdGroupToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();
        match tools::ad_groups_write::create_ad_group(
            &config,
            &cid,
            &params.campaign_id,
            &params.ad_group_name,
            params.cpc_bid_micros,
            params.status,
        ) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(
        description = "Draft ad group updates (name, CPC bid, ad rotation mode). ad_rotation_mode=ROTATE_FOREVER is 'Rotate indefinitely' in the UI. Returns a preview — call confirm_and_apply to execute."
    )]
    async fn update_ad_group(
        &self,
        Parameters(params): Parameters<UpdateAdGroupToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();
        match tools::ad_groups_write::update_ad_group(
            &config,
            &cid,
            &params.ad_group_id,
            params.name.as_deref(),
            params.cpc_bid_micros,
            params.ad_rotation_mode,
        ) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    // ── Write tools ──────────────────────────────────────────────────────

    #[tool(
        description = "Draft a new campaign (PAUSED) with budget, ad group, and optional keywords. Returns a preview — call confirm_and_apply to execute."
    )]
    async fn draft_campaign(
        &self,
        Parameters(params): Parameters<DraftCampaignToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();
        let channel_type = params.channel_type.as_deref().unwrap_or("SEARCH");
        let ad_group_name = params
            .ad_group_name
            .as_deref()
            .unwrap_or("Default Ad Group");
        let keywords: Vec<tools::campaigns_write::KeywordInput> =
            params.keywords.unwrap_or_default();

        match tools::campaigns_write::draft_campaign(&tools::campaigns_write::DraftCampaignParams {
            config: &config,
            customer_id: &cid,
            campaign_name: &params.campaign_name,
            daily_budget: params.daily_budget,
            bidding_strategy: &params.bidding_strategy,
            target_cpa: params.target_cpa,
            target_roas: params.target_roas,
            channel_type,
            ad_group_name,
            keywords,
            geo_target_ids: params.geo_target_ids,
            language_ids: params.language_ids,
            status: params.status,
        }) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(
        description = "Draft campaign updates (budget, bidding, targeting). Returns a preview — call confirm_and_apply to execute."
    )]
    async fn update_campaign(
        &self,
        Parameters(params): Parameters<UpdateCampaignToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();

        // A daily-budget update targets the campaign's budget resource, which has
        // its own ID distinct from the campaign ID. Resolve it via the API first
        // so the mutate hits the real budget (issue #5).
        let budget_resource_name: Option<String> = if params.daily_budget.is_some() {
            let client = match GoogleAdsClient::new(&config) {
                Ok(c) => c,
                Err(e) => return serde_json::json!({"error": e.to_string()}).to_string(),
            };
            match tools::campaigns_write::resolve_campaign_budget_resource(
                &client,
                &cid,
                &params.campaign_id,
            )
            .await
            {
                Ok(rn) => Some(rn),
                Err(e) => return serde_json::json!({"error": e.to_string()}).to_string(),
            }
        } else {
            None
        };

        match tools::campaigns_write::update_campaign(
            &tools::campaigns_write::UpdateCampaignParams {
                config: &config,
                customer_id: &cid,
                campaign_id: &params.campaign_id,
                bidding_strategy: params.bidding_strategy.as_deref(),
                target_cpa: params.target_cpa,
                target_roas: params.target_roas,
                daily_budget: params.daily_budget,
                budget_resource_name: budget_resource_name.as_deref(),
                geo_target_ids: params.geo_target_ids.unwrap_or_default(),
                language_ids: params.language_ids.unwrap_or_default(),
            },
        ) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(
        description = "Draft a Responsive Search Ad. Returns a preview — call confirm_and_apply to execute."
    )]
    async fn draft_responsive_search_ad(
        &self,
        Parameters(params): Parameters<DraftRsaToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();

        match tools::ads_write::draft_responsive_search_ad(&tools::ads_write::DraftRsaParams {
            config: &config,
            customer_id: &cid,
            ad_group_id: &params.ad_group_id,
            headlines: params.headlines,
            descriptions: params.descriptions,
            final_url: &params.final_url,
            path1: params.path1.as_deref(),
            path2: params.path2.as_deref(),
            status: params.status,
        }) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(
        description = "Draft keyword additions with match types (EXACT, PHRASE, BROAD). Returns a preview."
    )]
    async fn draft_keywords(
        &self,
        Parameters(params): Parameters<DraftKeywordsToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();

        let keywords: Vec<tools::keywords_write::KeywordWithMatchType> = params
            .keywords
            .into_iter()
            .map(|kw| tools::keywords_write::KeywordWithMatchType {
                text: kw.text,
                match_type: kw.match_type,
            })
            .collect();

        match tools::keywords_write::draft_keywords(&config, &cid, &params.ad_group_id, keywords) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(
        description = "Draft negative keyword additions to prevent ads from showing for irrelevant searches."
    )]
    async fn add_negative_keywords(
        &self,
        Parameters(params): Parameters<AddNegativeKeywordsToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();
        let match_type = params.match_type.as_deref().unwrap_or("EXACT");

        match tools::keywords_write::add_negative_keywords(
            &config,
            &cid,
            &params.campaign_id,
            params.keywords,
            match_type,
        ) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(
        description = "Exclude a geographic location from a campaign (negative location criterion — the inverse of update_campaign's positive geo targeting). Ads stop serving in that location. geo_target_id accepts a bare numeric ID or geoTargetConstants/{id}; find IDs via search_geo_targets. Returns a preview — call confirm_and_apply to execute."
    )]
    async fn exclude_geo_target(
        &self,
        Parameters(params): Parameters<ExcludeGeoTargetToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();

        match tools::geo::exclude_geo_target(
            &config,
            &cid,
            &params.campaign_id,
            &params.geo_target_id,
        ) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(
        description = "Remove a positively-targeted geographic location from a campaign (stop serving in a location that is currently an included target — e.g. trimming one country out of a multi-country campaign). Use this rather than exclude_geo_target when the location is already targeted: a negative criterion would collide with the existing positive one. Destructive — requires confirmed_twice. Returns a preview — call confirm_and_apply to execute."
    )]
    async fn remove_geo_target(
        &self,
        Parameters(params): Parameters<RemoveGeoTargetToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();

        match tools::geo::remove_geo_target(
            &config,
            &cid,
            &params.campaign_id,
            &params.geo_target_id,
        ) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(description = "Draft sitelink extensions for a campaign. Returns a preview.")]
    async fn draft_sitelinks(
        &self,
        Parameters(params): Parameters<DraftSitelinksToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();

        match tools::extensions_write::draft_sitelinks(
            &config,
            &cid,
            &params.campaign_id,
            params.sitelinks,
        ) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(description = "Draft callout extensions for a campaign.")]
    async fn create_callouts(
        &self,
        Parameters(params): Parameters<CreateCalloutsToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();

        match tools::extensions_write::create_callouts(
            &config,
            &cid,
            &params.campaign_id,
            params.callouts,
        ) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(description = "Draft structured snippet extensions for a campaign.")]
    async fn create_structured_snippets(
        &self,
        Parameters(params): Parameters<CreateSnippetsToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();

        match tools::extensions_write::create_structured_snippets(
            &config,
            &cid,
            &params.campaign_id,
            &params.header,
            params.values,
        ) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(
        description = "Draft REMOVING a campaign extension (IRREVERSIBLE). Specify campaign_id, asset_id, and field_type (SITELINK, CALLOUT, or STRUCTURED_SNIPPET)."
    )]
    async fn remove_extension(
        &self,
        Parameters(params): Parameters<RemoveExtensionToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();

        match tools::extensions_write::remove_extension(
            &config,
            &cid,
            &params.campaign_id,
            &params.asset_id,
            &params.field_type,
        ) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(
        description = "Draft REMOVING keywords from an ad group by criterion IDs (IRREVERSIBLE)."
    )]
    async fn remove_keywords(
        &self,
        Parameters(params): Parameters<RemoveKeywordsToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();

        match tools::keywords_write::remove_keywords(
            &config,
            &cid,
            &params.ad_group_id,
            params.criterion_ids,
        ) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(
        description = "Draft REMOVING negative keywords from a campaign by criterion IDs (IRREVERSIBLE)."
    )]
    async fn remove_negative_keywords(
        &self,
        Parameters(params): Parameters<RemoveNegativeKeywordsToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();

        match tools::keywords_write::remove_negative_keywords(
            &config,
            &cid,
            &params.campaign_id,
            params.criterion_ids,
        ) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(description = "Draft pausing a campaign, ad group, ad, or keyword.")]
    async fn pause_entity(&self, Parameters(params): Parameters<EntityActionParams>) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let entity_id = match self
            .resolve_entity_id_for_status(&cid, &params.entity_type, &params.entity_id)
            .await
        {
            Ok(id) => id,
            Err(e) => return serde_json::json!({"error": e}).to_string(),
        };
        let config = self.config.clone();

        match tools::entity_lifecycle::pause_entity(&config, &cid, &params.entity_type, &entity_id)
        {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(description = "Draft enabling a paused campaign, ad group, ad, or keyword.")]
    async fn enable_entity(&self, Parameters(params): Parameters<EntityActionParams>) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let entity_id = match self
            .resolve_entity_id_for_status(&cid, &params.entity_type, &params.entity_id)
            .await
        {
            Ok(id) => id,
            Err(e) => return serde_json::json!({"error": e}).to_string(),
        };
        let config = self.config.clone();

        match tools::entity_lifecycle::enable_entity(&config, &cid, &params.entity_type, &entity_id)
        {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(
        description = "Draft REMOVING an entity (IRREVERSIBLE). Use pause_entity instead if temporary."
    )]
    async fn remove_entity(&self, Parameters(params): Parameters<EntityActionParams>) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let entity_id = match self
            .resolve_entity_id_for_status(&cid, &params.entity_type, &params.entity_id)
            .await
        {
            Ok(id) => id,
            Err(e) => return serde_json::json!({"error": e}).to_string(),
        };
        let config = self.config.clone();

        match tools::entity_lifecycle::remove_entity(&config, &cid, &params.entity_type, &entity_id)
        {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    // ── Phase 5: PMax ─────────────────────────────────────────────────

    #[tool(
        description = "Create a Performance Max campaign with text assets. Image assets require separate upload. Returns a preview — call confirm_and_apply to execute."
    )]
    async fn create_pmax_campaign(
        &self,
        Parameters(params): Parameters<CreatePmaxCampaignToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();
        let start_paused = params.start_paused.unwrap_or(true);

        match tools::pmax::create_pmax_campaign(&tools::pmax::CreatePmaxCampaignParams {
            config: &config,
            customer_id: &cid,
            campaign_name: &params.campaign_name,
            daily_budget: params.daily_budget,
            bidding_strategy: &params.bidding_strategy,
            final_urls: params.final_urls,
            headlines: params.headlines,
            long_headlines: params.long_headlines,
            descriptions: params.descriptions,
            business_name: &params.business_name,
            geo_target_ids: params.geo_target_ids,
            start_paused,
        }) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    // ── Phase 5: Audiences ──────────────────────────────────────────────

    #[tool(
        description = "Create a custom audience (WEBSITE_VISITORS or CUSTOMER_MATCH). Returns a preview."
    )]
    async fn create_custom_audience(
        &self,
        Parameters(params): Parameters<CreateCustomAudienceToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();

        match tools::audiences::create_custom_audience(
            &config,
            &cid,
            &params.audience_name,
            &params.audience_type,
            params.urls_or_rules,
        ) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(
        description = "Add audience targeting (TARGETING or OBSERVATION) to a campaign. Returns a preview."
    )]
    async fn add_audience_targeting(
        &self,
        Parameters(params): Parameters<AddAudienceTargetingToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();

        match tools::audiences::add_audience_targeting(
            &config,
            &cid,
            &params.campaign_id,
            &params.audience_id,
            &params.targeting_mode,
        ) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    // ── Phase 5: Bidding ────────────────────────────────────────────────

    #[tool(
        description = "Create a portfolio bidding strategy (TARGET_CPA, TARGET_ROAS, TARGET_IMPRESSION_SHARE). Returns a preview."
    )]
    async fn create_portfolio_bidding_strategy(
        &self,
        Parameters(params): Parameters<CreatePortfolioBiddingToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();

        match tools::bidding::create_portfolio_bidding_strategy(
            &config,
            &cid,
            &params.name,
            &params.strategy_type,
            params.target_cpa,
            params.target_roas,
        ) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(
        description = "Update a keyword's CPC bid. Checks bid increase safety limit. Returns a preview."
    )]
    async fn update_keyword_bid(
        &self,
        Parameters(params): Parameters<UpdateKeywordBidToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();

        match tools::bidding::update_keyword_bid(
            &config,
            &cid,
            &params.ad_group_id,
            &params.criterion_id,
            params.current_bid,
            params.new_bid,
        ) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    // ── Phase 5: Assets ─────────────────────────────────────────────────

    #[tool(description = "Upload an image asset (base64-encoded). Returns a preview.")]
    async fn upload_image_asset(
        &self,
        Parameters(params): Parameters<UploadImageAssetToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();

        match tools::assets::upload_image_asset(
            &config,
            &cid,
            &params.asset_name,
            &params.image_data_base64,
        ) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(description = "Upload a reusable text asset. Returns a preview.")]
    async fn upload_text_asset(
        &self,
        Parameters(params): Parameters<UploadTextAssetToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();

        match tools::assets::upload_text_asset(
            &config,
            &cid,
            &params.asset_name,
            &params.text_content,
        ) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    // ── Phase 5: Scheduling ─────────────────────────────────────────────

    #[tool(
        description = "Set ad schedule for a campaign (day-of-week + time windows). Returns a preview."
    )]
    async fn set_campaign_schedule(
        &self,
        Parameters(params): Parameters<SetCampaignScheduleToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();

        match tools::scheduling::set_campaign_schedule(
            &config,
            &cid,
            &params.campaign_id,
            params.schedules,
        ) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    // ── Keyword Planner ─────────────────────────────────────────────────

    #[tool(description = "Discover keyword ideas from seed keywords using the Keyword Planner.")]
    async fn discover_keywords(
        &self,
        Parameters(params): Parameters<DiscoverKeywordsToolParams>,
    ) -> String {
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        self.run_tool(|client| async move {
            tools::keyword_planner::discover_keywords(&client, &cid, params.seed_keywords).await
        })
        .await
    }

    #[tool(description = "Get historical keyword performance metrics for forecasting.")]
    async fn get_keyword_forecasts(
        &self,
        Parameters(params): Parameters<KeywordForecastsToolParams>,
    ) -> String {
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        self.run_tool(|client| async move {
            tools::keyword_planner::get_keyword_forecasts(&client, &cid, params.keyword_texts).await
        })
        .await
    }

    // ── Policy ─────────────────────────────────────────────────────────

    #[tool(description = "Get policy issues for ads (disapproved, limited, under review).")]
    async fn get_policy_issues(&self, Parameters(params): Parameters<CustomerIdParams>) -> String {
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        self.run_tool(|client| async move { tools::policy::get_policy_issues(&client, &cid).await })
            .await
    }

    // ── Conversions ────────────────────────────────────────────────────

    #[tool(description = "List all conversion actions configured in the account.")]
    async fn get_conversion_actions(
        &self,
        Parameters(params): Parameters<CustomerIdParams>,
    ) -> String {
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        self.run_tool(|client| async move {
            tools::conversions::get_conversion_actions(&client, &cid).await
        })
        .await
    }

    #[tool(
        description = "Create a conversion action for server-side click uploads (type UPLOAD_CLICKS, gclid-based offline conversion import). Use this to create a 'Signup' or 'Activation' conversion. Returns a preview — call confirm_and_apply to execute. After apply, the numeric ID (for a *_CONVERSION_ACTION_ID env var) is the trailing segment of the returned conversionAction resourceName."
    )]
    async fn create_conversion_action(
        &self,
        Parameters(params): Parameters<CreateConversionActionToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();
        let category = params.category.unwrap_or_else(|| "SIGNUP".to_string());
        let counting_type = params
            .counting_type
            .unwrap_or_else(|| "ONE_PER_CLICK".to_string());
        let window = params.click_through_lookback_window_days.unwrap_or(30);

        match tools::conversions::create_conversion_action(
            &config,
            &cid,
            &params.name,
            &category,
            &counting_type,
            window,
        ) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(
        description = "Set a conversion action as primary (counts in the Conversions column and Smart Bidding) or secondary (observation only, excluded from the bidding signal). Use it to demote a value-0 signup event so it stops diluting a Maximize Conversions / Target CPA goal without losing its reporting. Returns a preview — call confirm_and_apply to execute."
    )]
    async fn set_conversion_action_primary_status(
        &self,
        Parameters(params): Parameters<SetConversionActionPrimaryToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();

        match tools::conversions::set_conversion_action_primary_status(
            &config,
            &cid,
            &params.conversion_action_id,
            params.primary,
        ) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    // ── Phase 5: Recommendations ────────────────────────────────────────

    #[tool(description = "List active (non-dismissed) recommendations for the account.")]
    async fn list_recommendations(
        &self,
        Parameters(params): Parameters<CustomerIdParams>,
    ) -> String {
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        self.run_tool(|client| async move {
            tools::recommendations::list_recommendations(&client, &cid).await
        })
        .await
    }

    #[tool(
        description = "Apply a recommendation. Returns a preview — call confirm_and_apply to execute."
    )]
    async fn apply_recommendation(
        &self,
        Parameters(params): Parameters<RecommendationActionToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();

        match tools::recommendations::apply_recommendation(&config, &cid, &params.recommendation_id)
        {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    #[tool(
        description = "Dismiss a recommendation. Returns a preview — call confirm_and_apply to execute."
    )]
    async fn dismiss_recommendation(
        &self,
        Parameters(params): Parameters<RecommendationActionToolParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let cid = self.resolve_customer_id(params.customer_id.as_deref());
        let config = self.config.clone();

        match tools::recommendations::dismiss_recommendation(
            &config,
            &cid,
            &params.recommendation_id,
        ) {
            Ok(preview) => preview.to_string(),
            Err(e) => serde_json::json!({"error": e.to_string()}).to_string(),
        }
    }

    // ── Confirm & Apply ─────────────────────────────────────────────────

    #[tool(
        description = "Execute a previously previewed change. IMPORTANT: defaults to dry_run=true. Set dry_run=false to make real changes. If config.safety.require_dry_run is true, dry_run=false will be rejected unless bypass_require_dry_run=true is also set."
    )]
    async fn confirm_and_apply(
        &self,
        Parameters(params): Parameters<ConfirmApplyParams>,
    ) -> String {
        if let Some(err) = self.check_write_allowed() {
            return err;
        }
        let config = self.config.clone();
        let dry_run = params.dry_run.unwrap_or(true);

        let input = tools::confirm::ConfirmApplyInput {
            plan_id: params.plan_id,
            dry_run,
            bypass_require_dry_run: params.bypass_require_dry_run.unwrap_or(false),
            confirmed_twice: params.confirmed_twice.unwrap_or(false),
        };

        match tools::confirm::confirm_and_apply(&config, input).await {
            Ok(result) => result.to_string(),
            Err(e) => {
                let hint = gaql::get_error_hint(&e.to_string())
                    .unwrap_or("No additional hints available.");
                serde_json::json!({
                    "error": e.to_string(),
                    "hint": hint,
                })
                .to_string()
            }
        }
    }
}

impl GoogleAdsMcp {
    pub fn new(config: Config) -> error::Result<Self> {
        Ok(Self {
            config,
            tool_router: Self::tool_router(),
        })
    }

    /// Resolve the customer ID: use the provided one or fall back to config.
    fn resolve_customer_id(&self, customer_id: Option<&str>) -> String {
        customer_id
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.config.ads.customer_id.clone())
    }

    /// For MCC queries: use login_customer_id if configured, otherwise customer_id.
    fn resolve_mcc_or_customer_id(&self) -> String {
        self.config
            .ads
            .login_customer_id
            .clone()
            .unwrap_or_else(|| self.config.ads.customer_id.clone())
    }

    /// Check if write operations are allowed. Returns error JSON if read-only mode is active.
    fn check_write_allowed(&self) -> Option<String> {
        if self.config.read_only {
            Some(
                serde_json::json!({
                    "error": "Write operations are disabled (GOOGLE_ADS_READ_ONLY=true)"
                })
                .to_string(),
            )
        } else {
            None
        }
    }

    /// Resolve the composite ad resource id `{ad_group_id}~{ad_id}` from a
    /// bare `ad_id`. Returns the input unchanged if it already contains `~`.
    ///
    /// Google Ads API addresses ads through the `adGroupAd` resource whose
    /// id is the composite `{ad_group_id}~{ad_id}`. Calling `mutate` against
    /// `customers/{cid}/adGroupAds/{ad_id}` alone fails with BAD_RESOURCE_ID.
    /// This helper runs a single GAQL query to retrieve the owning ad_group
    /// for the given ad, then assembles the composite id.
    async fn resolve_ad_composite_id(&self, cid: &str, ad_id: &str) -> Result<String, String> {
        if ad_id.contains('~') {
            return Ok(ad_id.to_string());
        }
        let client = GoogleAdsClient::new(&self.config).map_err(|e| e.to_string())?;
        let query = format!(
            "SELECT ad_group.id FROM ad_group_ad WHERE ad_group_ad.ad.id = {}",
            ad_id
        );
        let result = tools::reporting::run_gaql(&client, cid, &query, "json")
            .await
            .map_err(|e| e.to_string())?;
        let parsed: serde_json::Value =
            serde_json::from_str(&result).map_err(|e| format!("invalid GAQL JSON: {e}"))?;
        let ag_id = parsed["results"][0]["adGroup"]["id"]
            .as_str()
            .ok_or_else(|| {
                format!("no ad_group found for ad_id={ad_id} (does the ad exist in this customer?)")
            })?;
        Ok(format!("{}~{}", ag_id, ad_id))
    }

    /// Resolve `entity_id` for status-change operations, expanding bare ad ids
    /// to the `{ad_group_id}~{ad_id}` composite required by the Google Ads API.
    async fn resolve_entity_id_for_status(
        &self,
        cid: &str,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<String, String> {
        if entity_type == "ad" {
            self.resolve_ad_composite_id(cid, entity_id).await
        } else {
            Ok(entity_id.to_string())
        }
    }

    /// Run a tool helper, handling client creation and error serialization.
    async fn run_tool<F, Fut>(&self, f: F) -> String
    where
        F: FnOnce(GoogleAdsClient) -> Fut,
        Fut: std::future::Future<Output = error::Result<String>>,
    {
        let client = match GoogleAdsClient::new(&self.config) {
            Ok(c) => c,
            Err(e) => return format!("{{\"error\": \"{}\"}}", e),
        };

        match f(client).await {
            Ok(result) => result,
            Err(e) => {
                let hint = gaql::get_error_hint(&e.to_string())
                    .unwrap_or("No additional hints available.");
                serde_json::json!({
                    "error": e.to_string(),
                    "hint": hint,
                })
                .to_string()
            }
        }
    }
}

#[rmcp::tool_handler]
impl rmcp::ServerHandler for GoogleAdsMcp {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo {
            protocol_version: rmcp::model::ProtocolVersion::LATEST,
            capabilities: rmcp::model::ServerCapabilities {
                tools: Some(rmcp::model::ToolsCapability::default()),
                ..Default::default()
            },
            server_info: rmcp::model::Implementation {
                name: "mcp-google-ads".to_string(),
                title: None,
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: Some(
                    "MCP server for Google Ads API with safety guardrails".to_string(),
                ),
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "MCP server for Google Ads API. Provides tools for campaign management, \
                 reporting, and optimization with built-in safety guardrails."
                    .to_string(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Claude rejects MCP tools whose name is >= 64 characters.
    /// This test ensures all tool names stay under the limit.
    #[test]
    fn test_all_tool_names_under_64_chars() {
        let config = Config::default();
        let server = GoogleAdsMcp::new(config).unwrap();
        let tools = server.tool_router.list_all();

        const MAX_TOOL_NAME_LENGTH: usize = 64;
        let mut violations = Vec::new();

        for tool in &tools {
            let name = &tool.name;
            if name.len() >= MAX_TOOL_NAME_LENGTH {
                violations.push(format!("'{}' ({} chars)", name, name.len()));
            }
        }

        assert!(
            violations.is_empty(),
            "Tool names must be < {} characters. Claude rejects longer names.\nViolations:\n  {}",
            MAX_TOOL_NAME_LENGTH,
            violations.join("\n  ")
        );
    }

    #[test]
    fn test_tool_count() {
        let config = Config::default();
        let server = GoogleAdsMcp::new(config).unwrap();
        let tools = server.tool_router.list_all();

        assert!(
            tools.len() >= 37,
            "Expected at least 37 tools, got {}. Some tools may be missing from the router.",
            tools.len()
        );
    }
}
