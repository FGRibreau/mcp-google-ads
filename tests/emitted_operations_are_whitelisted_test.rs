//! Every mutate operation a tool emits must be one the client will send.
//!
//! `GoogleAdsClient::mutate` validates each operation key against
//! `VALID_MUTATE_OPERATION_KEYS` before any HTTP traffic, so a tool that emits
//! a key missing from that list fails on every call — client-side, with a
//! validation error that reads like a caller mistake. `create_custom_audience`
//! shipped that way: it emitted `customAudienceOperation`, a key that exists in
//! no version of the API (custom audiences have their own service, off the
//! `googleAds:mutate` path), so the tool could never have worked.
//!
//! The tool sources are discovered from the directory rather than listed, so a
//! new tool file is covered the day it is added.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use mcp_google_ads::client::VALID_MUTATE_OPERATION_KEYS;

/// Collect every `"...Operation":` JSON key written in the tool sources.
fn emitted_operation_keys() -> BTreeSet<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tools");
    let entries = fs::read_dir(&dir).expect("src/tools is readable");
    let mut keys = BTreeSet::new();

    for entry in entries {
        let path = entry.expect("directory entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let source = fs::read_to_string(&path).expect("tool source is readable");
        for (idx, _) in source.match_indices("Operation\":") {
            // Walk back to the opening quote of the JSON key.
            let before = &source[..idx + "Operation".len()];
            let Some(quote) = before.rfind('"') else {
                continue;
            };
            let key = &before[quote + 1..];
            if !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric()) {
                keys.insert(key.to_string());
            }
        }
    }
    keys
}

#[test]
fn every_emitted_operation_key_is_whitelisted() {
    let emitted = emitted_operation_keys();
    assert!(
        !emitted.is_empty(),
        "found no operation keys — the scan itself is broken"
    );

    let unknown: Vec<_> = emitted
        .iter()
        .filter(|k| !VALID_MUTATE_OPERATION_KEYS.contains(&k.as_str()))
        .collect();

    assert!(
        unknown.is_empty(),
        "these tools emit operation keys the client rejects before sending, \
         so they fail on every call: {unknown:?}"
    );
}
