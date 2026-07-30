use mcp_google_ads::client::{DEFAULT_BASE_URL, VALID_MUTATE_OPERATION_KEYS};
use std::collections::BTreeSet;

#[test]
fn defaults_to_google_ads_api_v25() {
    assert_eq!(DEFAULT_BASE_URL, "https://googleads.googleapis.com/v25");
}

#[test]
fn mutate_operation_whitelist_exactly_matches_v25_oneof() {
    let expected = BTreeSet::from([
        "adGroupAdLabelOperation",
        "adGroupAdOperation",
        "adGroupAssetOperation",
        "adGroupBidModifierOperation",
        "adGroupCriterionCustomizerOperation",
        "adGroupCriterionLabelOperation",
        "adGroupCriterionOperation",
        "adGroupCustomizerOperation",
        "adGroupLabelOperation",
        "adGroupOperation",
        "adOperation",
        "adParameterOperation",
        "assetGroupAssetOperation",
        "assetGroupListingGroupFilterOperation",
        "assetGroupOperation",
        "assetGroupSignalOperation",
        "assetOperation",
        "assetSetAssetOperation",
        "assetSetOperation",
        "audienceOperation",
        "biddingDataExclusionOperation",
        "biddingSeasonalityAdjustmentOperation",
        "biddingStrategyOperation",
        "bookCampaignsOperation",
        "campaignAssetOperation",
        "campaignAssetSetOperation",
        "campaignBidModifierOperation",
        "campaignBudgetOperation",
        "campaignConversionGoalOperation",
        "campaignCriterionOperation",
        "campaignCustomizerOperation",
        "campaignDraftOperation",
        "campaignGroupOperation",
        "campaignLabelOperation",
        "campaignOperation",
        "campaignSharedSetOperation",
        "conversionActionOperation",
        "conversionCustomVariableOperation",
        "conversionGoalCampaignConfigOperation",
        "conversionValueRuleOperation",
        "conversionValueRuleSetOperation",
        "customConversionGoalOperation",
        "customerAssetOperation",
        "customerConversionGoalOperation",
        "customerCustomizerOperation",
        "customerLabelOperation",
        "customerNegativeCriterionOperation",
        "customerOperation",
        "customizerAttributeOperation",
        "experimentArmOperation",
        "experimentOperation",
        "keywordPlanAdGroupKeywordOperation",
        "keywordPlanAdGroupOperation",
        "keywordPlanCampaignKeywordOperation",
        "keywordPlanCampaignOperation",
        "keywordPlanOperation",
        "labelOperation",
        "quoteCampaignsOperation",
        "recommendationSubscriptionOperation",
        "remarketingActionOperation",
        "sharedCriterionOperation",
        "sharedSetOperation",
        "smartCampaignSettingOperation",
        "userListOperation",
    ]);
    let actual = VALID_MUTATE_OPERATION_KEYS
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();

    assert_eq!(
        VALID_MUTATE_OPERATION_KEYS.len(),
        actual.len(),
        "whitelist contains duplicate keys"
    );
    assert_eq!(actual, expected);
}
