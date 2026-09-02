//! Tests for NUT-06: Mint Information
//!
//! Test patterns follow CDK: https://github.com/cashubtc/cdk

use cashu_core_lite::nuts::nut06::{ContactInfo, MintInfo, NutSettings, PaymentMethod};

fn sample_contact() -> ContactInfo {
    ContactInfo {
        method: "email".to_string(),
        info: "test@example.com".to_string(),
    }
}

fn sample_nuts() -> Vec<(String, NutSettings)> {
    vec![
        ("3".to_string(), NutSettings { methods: vec![] }),
        (
            "4".to_string(),
            NutSettings {
                methods: vec![PaymentMethod {
                    method: "bolt11".to_string(),
                    unit: "sat".to_string(),
                }],
            },
        ),
        ("7".to_string(), NutSettings { methods: vec![] }),
    ]
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
        nuts: sample_nuts(),
    };

    let mut buf = vec![];
    minicbor::encode(&info, &mut buf).expect("encode");
    let decoded: MintInfo = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.name, "Test Mint");
    assert_eq!(decoded.version, "0.1.0");
    assert_eq!(decoded.nuts, sample_nuts());
}

#[test]
fn test_mint_info_minimal() {
    let info = MintInfo {
        name: "Minimal".to_string(),
        pubkey: "02cafe".to_string(),
        version: "1.0".to_string(),
        description: "".to_string(),
        contact: vec![],
        nuts: vec![],
    };

    let mut buf = vec![];
    minicbor::encode(&info, &mut buf).expect("encode");
    let decoded: MintInfo = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.contact.len(), 0);
    assert_eq!(decoded.nuts.len(), 0);
}

#[test]
fn test_nut_settings_entries() {
    let nuts = sample_nuts();

    assert_eq!(nuts.len(), 3);
    let nut4 = nuts
        .iter()
        .find(|(nut, _)| nut == "4")
        .expect("nut 4 advertised");
    assert_eq!(nut4.1.methods.len(), 1);
    assert_eq!(nut4.1.methods[0].method, "bolt11");
    assert_eq!(nut4.1.methods[0].unit, "sat");

    let nut7 = nuts
        .iter()
        .find(|(nut, _)| nut == "7")
        .expect("nut 7 advertised");
    assert_eq!(nut7.1.methods.len(), 0);
}

#[test]
fn test_nut_settings_cbor_roundtrip() {
    let settings = NutSettings {
        methods: vec![
            PaymentMethod {
                method: "bolt11".to_string(),
                unit: "sat".to_string(),
            },
            PaymentMethod {
                method: "bolt11".to_string(),
                unit: "usd".to_string(),
            },
        ],
    };

    let mut buf = vec![];
    minicbor::encode(&settings, &mut buf).expect("encode");
    let decoded: NutSettings = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded, settings);
    assert_eq!(decoded.methods[1].unit, "usd");
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
        nuts: sample_nuts(),
    };

    let mut buf = vec![];
    minicbor::encode(&info, &mut buf).expect("encode");
    let decoded: MintInfo = minicbor::decode(&buf).expect("decode");

    assert_eq!(decoded.contact.len(), 2);
}
