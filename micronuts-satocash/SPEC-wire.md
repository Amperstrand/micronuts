# SatoCash wire spec (extracted)

Verbatim command blocks from `src/main/java/org/satocash/applet/Satocash.java`
(Toporin/Satocash-Applet master @ ee413cc702f79e53f2238d5efce5cfc94a498a1e).
Regenerate: `python3 scripts/extract_wire_spec.py <Satocash.java> > SPEC-wire.md`.

## SATOCASH_GET_STATUS (0xb0)

ins: 0xB0
p1: 0x00
p2: 0x00
data: none
return: [versions(4b) | PIN0-PUK0-PIN1-PUK1 tries (4b) |
needs2FA (1b) | RFU(1b) | setupDone(1b) | needs_secure_channel(1b) | nfc_policy(1b) |
pin_policy(1b) | RFU(1b) |
MAX_NB_MINT (1b) | NB_USED_MINT(1b) |
MAX_NB_KEYSET(1b) | NB_USED_KEYSET(1b)  |
MAX_NB_PROOFS(2b) | NB_PROOFS_UNSPENT(2b) | NB_PROOFS_SPENT(2b) |
RFU(6b) |
size(NB_UNITS*PIN_POLICY_AMOUNT_THRESHOLD_SIZE) | pin_policy_amount_threshold |
size(NB_UNITS*PIN_POLICY_AMOUNT_THRESHOLD_SIZE) | pin_policy_cumulative_amount
]

## IMPORT_MINT (0xb1)

ins: 0xB1
p1: RFU
p2: RFU
data: [mint_url_size(1b) | mint_url]
return: [MINT_INDEX(1b)]

## EXPORT_MINT (0xb2)

ins: 0xB2
p1: index
p2: RFU
data: none
return: [URL_size (1b) | URL ]

## IMPORT_KEYSET (0xb4)

ins: 0xB4
p1: RFU
p2: RFU
data: [keyset_id(8b) | mint_index(1b) | unit(1b) ]
return: [KEYSET_INDEX(1b)]

## IMPORT_PROOF (0xb7)

ins: 0xB7
p1: RFU
p2: RFU
data: [keyset_index(1b) | amount_exponent(1b) | unblinded_key(33b) | secret(32b)]
return: [index(2b)]

## EXPORT_PROOFS (0xb8)

ins: 0xB8
p1: RFU
p2: OP_INIT | OP_PROCESS
data (OP_INIT): [ proof_index_list_size(1b) | proof_index(2b) ... | 2FA_size(1b) | 2FA ] else
data (OP_PROCESS): []
return: [proof_index(2b) | proof_state(1b) | keyset_index(1b) | amount_exponent(1b) | unblinded_key(33b) | secret(32b)]
exceptions (OP_INIT): 9C06 SW_UNAUTHORIZED, 9C11 SW_INCORRECT_P2, 6700 SW_WRONG_LENGTH, 9C0F SW_INVALID_PARAMETER, 9C61 SW_MATH_OVERFLOW, 9C62 SW_MULTIPLE_UNITS,
exceptions (OP_PROCESS): 9C06 SW_UNAUTHORIZED, 9C11 SW_INCORRECT_P2, 9C13 SW_INCORRECT_INITIALIZATION,

## GET_STATUS (0x3c)

ins: 0x3C
p1: 0x00
p2: 0x00
data: none
return: [versions(4b) | PIN0-PUK0-PIN1-PUK1 tries (4b) | needs2FA (1b) | is_seeded(1b) | setupDone(1b) | needs_secure_channel(1b) | nfc_policy(1b)]

