//! `list_accessible_customers` must GET `customers:listAccessibleCustomers`
//! and must NOT send a `login-customer-id` header — that endpoint isn't
//! scoped to any manager account, unlike `search`/`mutate`.

mod common;

use mcp_google_ads::client::GoogleAdsClient;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test]
async fn list_accessible_customers_hits_get_endpoint() {
    let (mock, client) = common::spawn_mock_google_ads().await;

    Mock::given(method("GET"))
        .and(path("/v25/customers:listAccessibleCustomers"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "resourceNames": [
                "customers/1112223333",
                "customers/4445556666"
            ]
        })))
        .expect(1)
        .mount(&mock)
        .await;

    let resource_names = client
        .list_accessible_customers()
        .await
        .expect("call should succeed against mock");

    assert_eq!(
        resource_names,
        vec![
            "customers/1112223333".to_string(),
            "customers/4445556666".to_string(),
        ]
    );
}

#[tokio::test]
async fn list_accessible_customers_omits_login_customer_id_header() {
    let mock = wiremock::MockServer::start().await;
    let base_url = format!("{}/v25", mock.uri());

    let mut config = common::test_config();
    config.ads.login_customer_id = Some("9998887777".to_string());
    let client = GoogleAdsClient::with_base_url(&config, base_url).unwrap();

    Mock::given(method("GET"))
        .and(path("/v25/customers:listAccessibleCustomers"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"resourceNames": []})))
        .expect(1)
        .mount(&mock)
        .await;

    let _ = client.list_accessible_customers().await.unwrap();

    let requests = mock.received_requests().await.unwrap();
    assert!(
        !requests[0].headers.contains_key("login-customer-id"),
        "listAccessibleCustomers must not send login-customer-id"
    );
}
