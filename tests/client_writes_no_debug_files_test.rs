//! The API client must not persist request or response bodies to disk.
//!
//! Until this guard, every `mutate` wrote its full payload to
//! `/tmp/mcp-google-ads-last-request.json` and every failure wrote the error
//! body next to it. Both were fixed, world-readable paths in a shared
//! directory: any local user could read the account's pending changes, a
//! symlink planted at either path would be followed on write, and concurrent
//! runs overwrote each other's file.
//!
//! Pinning the absence of the two legacy paths is not enough — a different
//! name would reintroduce the same problem — so this rejects any filesystem
//! write from the client. Diagnostics belong in the error response, which
//! carries `error_code`, `api_errors` and `failure_details`.

const CLIENT_SOURCE: &str = include_str!("../src/client.rs");

#[test]
fn client_does_not_write_request_or_error_bodies_to_disk() {
    for forbidden in ["fs::write", "File::create", "OpenOptions"] {
        assert!(
            !CLIENT_SOURCE.contains(forbidden),
            "src/client.rs must not write to the filesystem (found `{forbidden}`): \
             request and error bodies stay in the response, never on disk"
        );
    }
}

#[test]
fn client_does_not_name_the_legacy_debug_dump_paths() {
    for path in [
        "/tmp/mcp-google-ads-last-request.json",
        "/tmp/mcp-google-ads-last-error.json",
    ] {
        assert!(
            !CLIENT_SOURCE.contains(path),
            "src/client.rs must not persist payloads at the legacy path {path}"
        );
    }
}
