use hmac::{Hmac, Mac};
use sha2::Sha256;
use storedoc::auth::{
    build_auth_header, build_canonical_string, canonicalize_uri, generate_hmac_signature,
    generate_nonce, verify_webhook_signature,
};
use storedoc::validation::{assert_uuid, normalize_object_path};
use storedoc::{StoreDocClient, StoreDocConfig, StoreDocError};

#[test]
fn canonicalizes_uri_like_server() {
    assert_eq!(canonicalize_uri("/api/storage/buckets"), "storage/buckets");
    assert_eq!(
        canonicalize_uri("/api/storage/buckets?page=2&per_page=10"),
        "storage/buckets?page=2&per_page=10"
    );
    assert_eq!(
        canonicalize_uri("///storage/objects/123"),
        "storage/objects/123"
    );
    assert_eq!(canonicalize_uri("/api"), "");
}

#[test]
fn generates_valid_hmac_signature() {
    let nonce = generate_nonce();
    assert_eq!(nonce.len(), 32);

    let canonical = build_canonical_string("post", "/api/storage/buckets?x=1", 1700000000, &nonce);
    assert_eq!(
        canonical,
        format!("POST\nstorage/buckets?x=1\n1700000000\n{nonce}")
    );

    let sig = generate_hmac_signature(
        "POST",
        "/api/storage/buckets?x=1",
        1700000000,
        "secret_123",
        &nonce,
    )
    .unwrap();
    assert_eq!(sig.len(), 64);

    let config = StoreDocConfig::new("https://storedoc.example.com/api")
        .with_hmac("STD_AK_TEST", "STD_SK_TEST");
    let (name, val) = build_auth_header("GET", "storage/buckets", &config).unwrap();
    assert_eq!(name, "Authorization");
    assert!(val.starts_with("HMAC-SHA256 Credential=STD_AK_TEST, Timestamp="));
}

#[test]
fn verifies_webhook_signature_in_constant_time() {
    let payload = br#"{"event":"object.uploaded","id":"123"}"#;
    let secret = "whsec_test_secret";

    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(payload);
    let sig_hex = hex::encode(mac.finalize().into_bytes());

    assert!(verify_webhook_signature(
        payload,
        &format!("sha256={sig_hex}"),
        secret,
        None,
        None
    ));
    assert!(!verify_webhook_signature(
        payload,
        &format!("sha256={}", "0".repeat(64)),
        secret,
        None,
        None
    ));
}

#[test]
fn rejects_non_https_remote_url_and_bad_paths() {
    let bad = StoreDocClient::new(StoreDocConfig::new("http://example.com/api"));
    assert!(matches!(bad, Err(StoreDocError::InvalidConfig(_))));

    let ok_local = StoreDocClient::new(StoreDocConfig::new("http://127.0.0.1:8000/api"));
    assert!(ok_local.is_ok());

    assert!(normalize_object_path("../etc/passwd").is_err());
    assert_eq!(
        normalize_object_path("/folder\\file.png").unwrap(),
        "folder/file.png"
    );
    assert!(assert_uuid("not-a-uuid", "bucket_id").is_err());
    assert!(assert_uuid("550e8400-e29b-41d4-a716-446655440000", "bucket_id").is_ok());
}
