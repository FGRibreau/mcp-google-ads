#[test]
fn client_source_has_no_fixed_unredacted_debug_dump_paths() {
    let client_source = include_str!("../src/client.rs");

    assert!(
        !client_source.contains("/tmp/mcp-google-ads-last-request.json"),
        "client must not persist mutate requests at the legacy fixed path"
    );
    assert!(
        !client_source.contains("/tmp/mcp-google-ads-last-error.json"),
        "client must not persist Google Ads error bodies at the legacy fixed path"
    );
}
