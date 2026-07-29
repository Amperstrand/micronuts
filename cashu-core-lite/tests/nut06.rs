//! Tests for NUT-06: Mint Information
//!
//! Test patterns follow CDK: https://github.com/cashubtc/cdk

use cashu_core_lite::nuts::nut06::{ContactInfo, MintInfo, NutSupport};

fn sample_contact() -> ContactInfo {
    ContactInfo {
        method: "email".to_string(),
        info: "test@example.com".to_string(),
    }
}

#[test]
fn test_contact_info_cbor_roundtrip() {
    let contact = sample_contact();

    let mut buf = vec![];
    minicbor::encode(&contact, &mut buf).expect("encode");
    let decoded: ContactInfo = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.method, "email");
    assert_eq!(decoded.info, "test@example.com");
}

#[test]
fn test_mint_info_cbor_roundtrip() {
    let info = MintInfo {
        name: "Test Mint".to_string(),
        pubkey: "02deadbeef...".to_string(),
        version: "0.1.0".to_string(),
        description: "A test mint for unit tests".to_string(),
        contact: vec![sample_contact()],
        nuts: NutSupport {
            supported: vec![0, 1, 2, 3, 4, 5, 6, 7],
        },
    };

    let mut buf = vec![];
    minicbor::encode(&info, &mut buf).expect("encode");
    let decoded: MintInfo = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.name, "Test Mint");
    assert_eq!(decoded.version, "0.1.0");
}

#[test]
fn test_mint_info_minimal() {
    let info = MintInfo {
        name: "Minimal".to_string(),
        pubkey: "02cafe".to_string(),
        version: "1.0".to_string(),
        description: "".to_string(),
        contact: vec![],
        nuts: NutSupport { supported: vec![0] },
    };

    let mut buf = vec![];
    minicbor::encode(&info, &mut buf).expect("encode");
    let decoded: MintInfo = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.contact.len(), 0);
}

#[test]
fn test_nut_support_supported_nuts() {
    let support = NutSupport {
        supported: vec![0, 1, 2, 3, 4, 5, 6, 7, 13],
    };

    assert_eq!(support.supported.len(), 9);
    assert!(support.supported.contains(&0));
    assert!(support.supported.contains(&13));
}

#[test]
fn test_mint_info_multiple_contacts() {
    let info = MintInfo {
        name: "Multi Contact".to_string(),
        pubkey: "02abc".to_string(),
        version: "1.0".to_string(),
        description: "test".to_string(),
        contact: vec![
            ContactInfo {
                method: "email".to_string(),
                info: "admin@test.com".to_string(),
            },
            ContactInfo {
                method: "nostr".to_string(),
                info: "npub1...".to_string(),
            },
        ],
        nuts: NutSupport { supported: vec![0] },
    };

    let mut buf = vec![];
    minicbor::encode(&info, &mut buf).expect("encode");
    let decoded: MintInfo = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.contact.len(), 2);
}
