# Cashu Conformance Matrix — 2026-09-02 14:19 UTC

**Summary**: 68 passed, 38 failed, 3 skipped (109 total)

## Invoice Description

| Scenario | `http://localhost:3030` |
|---|---|
| `invoice_description_truncated_quote_id` | ⏭️ |

## NUT-02 Keysets

| Scenario | `http://localhost:3030` |
|---|---|
| `keysets_returns_active_keyset` | ✅ |
| `keys_returns_pubkey_for_amount` | ✅ |
| `keyset_has_correct_unit` | ✅ |
| `keyset_fee_ppk_present` | ✅ |
| `multiple_keysets_unit_filter` | ✅ |
| `keyset_keys_are_valid_pubkeys` | ✅ |

## NUT-04 Accounting

| Scenario | `http://localhost:3030` |
|---|---|
| `mint_quote_has_accounting_fields` | ✅ |
| `mint_quote_uuid_v7` | ❌ |
| `mint_quote_accounting_after_payment` | ✅ |
| `mint_quote_accounting_after_mint` | ✅ |
| `mint_quote_updated_at_monotonic` | ✅ |

> ❌ `mint_quote_uuid_v7` @ `http://localhost:3030`: quote='0000000000000003' does not match UUID v7 pattern

## NUT-08 Fees

| Scenario | `http://localhost:3030` |
|---|---|
| `fee_zero_ppk_swap_succeeds` | ✅ |
| `fee_calculated_correctly` | ✅ |
| `fee_insufficient_outputs_fails` | ✅ |
| `fee_exact_balance_succeeds` | ✅ |
| `fee_melt_quote_includes_fee_reserve` | ✅ |
| `fee_per_proof_not_per_amount` | ⏭️ |

## Melt spending conditions

| Scenario | `http://localhost:3030` |
|---|---|
| `melt_p2pk_unsigned_fails` | ✅ |
| `melt_p2pk_signed_succeeds` | ❌ |
| `melt_p2pk_sigall_unsigned_fails` | ✅ |
| `melt_p2pk_sigall_transaction_signature_succeeds` | ❌ |
| `melt_htlc_preimage_only_no_pubkeys_succeeds` | ❌ |
| `melt_htlc_preimage_only_fails` | ✅ |
| `melt_htlc_signature_only_fails` | ✅ |
| `melt_htlc_preimage_and_signature_succeeds` | ❌ |
| `melt_htlc_sigall_preimage_and_transaction_signature_succeeds` | ❌ |
| `melt_p2pk_post_locktime_anyone_can_spend` | ❌ |
| `melt_p2pk_before_locktime_wrong_key_fails` | ✅ |
| `melt_p2pk_before_locktime_correct_key_succeeds` | ❌ |

> ❌ `melt_p2pk_signed_succeeds` @ `http://localhost:3030`: got 400: {'code': 'AMOUNT_MISMATCH', 'detail': 'input/output amount mismatch', 'error_kind': 'AMOUNT_MISMATCH'}

> ❌ `melt_p2pk_sigall_transaction_signature_succeeds` @ `http://localhost:3030`: got 400: {'code': 'AMOUNT_MISMATCH', 'detail': 'input/output amount mismatch', 'error_kind': 'AMOUNT_MISMATCH'}

> ❌ `melt_htlc_preimage_only_no_pubkeys_succeeds` @ `http://localhost:3030`: got 400: {'code': 'AMOUNT_MISMATCH', 'detail': 'input/output amount mismatch', 'error_kind': 'AMOUNT_MISMATCH'}

> ❌ `melt_htlc_preimage_and_signature_succeeds` @ `http://localhost:3030`: got 400: {'code': 'AMOUNT_MISMATCH', 'detail': 'input/output amount mismatch', 'error_kind': 'AMOUNT_MISMATCH'}

> ❌ `melt_htlc_sigall_preimage_and_transaction_signature_succeeds` @ `http://localhost:3030`: got 400: {'code': 'AMOUNT_MISMATCH', 'detail': 'input/output amount mismatch', 'error_kind': 'AMOUNT_MISMATCH'}

> ❌ `melt_p2pk_post_locktime_anyone_can_spend` @ `http://localhost:3030`: got 400: {'code': 'AMOUNT_MISMATCH', 'detail': 'input/output amount mismatch', 'error_kind': 'AMOUNT_MISMATCH'}

> ❌ `melt_p2pk_before_locktime_correct_key_succeeds` @ `http://localhost:3030`: got 400: {'code': 'AMOUNT_MISMATCH', 'detail': 'input/output amount mismatch', 'error_kind': 'AMOUNT_MISMATCH'}

## NUT-11 P2PK SIG_ALL

| Scenario | `http://localhost:3030` |
|---|---|
| `p2pk_sigall_requires_transaction_signature` | ❌ |
| `p2pk_sigall_sig_inputs_fail` | ❌ |
| `p2pk_sigall_multisig_2of3` | ✅ |
| `p2pk_sigall_wrong_signer_fails` | ❌ |
| `p2pk_sigall_duplicate_signatures_fail` | ❌ |
| `p2pk_sigall_locktime_before_expiry_primary_only` | ❌ |
| `p2pk_sigall_locktime_after_expiry_primary_still_works` | ✅ |
| `p2pk_sigall_locktime_after_expiry_no_refund_anyone_can_spend` | ✅ |
| `p2pk_sigall_multisig_locktime_primary_still_works` | ✅ |
| `p2pk_sigall_mixed_proofs_different_data_fail` | ❌ |
| `p2pk_sigall_mixed_proofs_different_kind_fail` | ❌ |
| `p2pk_sigall_mixed_proofs_different_tags_fail` | ❌ |
| `p2pk_sigall_multisig_before_locktime` | ✅ |
| `p2pk_sigall_more_signatures_than_required` | ✅ |
| `p2pk_sigall_refund_multisig_2of2` | ✅ |
| `p2pk_sigall_output_amounts_swapped_fail` | ❌ |

> ❌ `p2pk_sigall_requires_transaction_signature` @ `http://localhost:3030`: got 200

> ❌ `p2pk_sigall_sig_inputs_fail` @ `http://localhost:3030`: got 200

> ❌ `p2pk_sigall_wrong_signer_fails` @ `http://localhost:3030`: got 200

> ❌ `p2pk_sigall_duplicate_signatures_fail` @ `http://localhost:3030`: got 200

> ❌ `p2pk_sigall_locktime_before_expiry_primary_only` @ `http://localhost:3030`: refund should be blocked: 200

> ❌ `p2pk_sigall_mixed_proofs_different_data_fail` @ `http://localhost:3030`: got 200

> ❌ `p2pk_sigall_mixed_proofs_different_kind_fail` @ `http://localhost:3030`: got 200

> ❌ `p2pk_sigall_mixed_proofs_different_tags_fail` @ `http://localhost:3030`: got 200

> ❌ `p2pk_sigall_output_amounts_swapped_fail` @ `http://localhost:3030`: got 200

## NUT-11 P2PK SIG_INPUTS

| Scenario | `http://localhost:3030` |
|---|---|
| `p2pk_swap_unsigned_fails` | ❌ |
| `p2pk_swap_signed_succeeds` | ✅ |
| `p2pk_wrong_signer_fails` | ❌ |
| `p2pk_locktime_after_expiry_primary_still_works` | ✅ |
| `p2pk_locktime_after_expiry_refund_succeeds` | ✅ |
| `p2pk_multisig_2of3` | ✅ |
| `p2pk_partial_signatures_fail` | ❌ |
| `p2pk_duplicate_signatures_fail` | ❌ |
| `p2pk_locktime_before_expiry_refund_blocked` | ❌ |
| `p2pk_locktime_after_expiry_no_refund_anyone_can_spend` | ✅ |

> ❌ `p2pk_swap_unsigned_fails` @ `http://localhost:3030`: got 200

> ❌ `p2pk_wrong_signer_fails` @ `http://localhost:3030`: got 200

> ❌ `p2pk_partial_signatures_fail` @ `http://localhost:3030`: got 200

> ❌ `p2pk_duplicate_signatures_fail` @ `http://localhost:3030`: got 200

> ❌ `p2pk_locktime_before_expiry_refund_blocked` @ `http://localhost:3030`: got 200

## NUT-12 DLEQ

| Scenario | `http://localhost:3030` |
|---|---|
| `dleq_proofs_present_in_mint_response` | ✅ |
| `dleq_proof_valid` | ✅ |
| `dleq_proof_absent_graceful` | ⏭️ |
| `dleq_proof_in_signature_response` | ✅ |
| `dleq_invalid_proof_rejected` | ✅ |
| `hash_e_test_vector_verification` | ✅ |

## NUT-12 HTLC SIG_INPUTS

| Scenario | `http://localhost:3030` |
|---|---|
| `htlc_preimage_only_no_pubkeys_succeeds` | ✅ |
| `htlc_preimage_only_fails` | ❌ |
| `htlc_signature_only_fails` | ❌ |
| `htlc_swap_preimage_and_signature_succeeds` | ✅ |
| `htlc_wrong_preimage_fails` | ❌ |
| `htlc_locktime_after_expiry_refund_succeeds` | ✅ |
| `htlc_multisig_2of3` | ✅ |
| `htlc_receiver_path_after_locktime` | ✅ |

> ❌ `htlc_preimage_only_fails` @ `http://localhost:3030`: got 200

> ❌ `htlc_signature_only_fails` @ `http://localhost:3030`: got 200

> ❌ `htlc_wrong_preimage_fails` @ `http://localhost:3030`: got 200

## NUT-12 HTLC SIG_ALL

| Scenario | `http://localhost:3030` |
|---|---|
| `htlc_sigall_preimage_only_no_pubkeys_succeeds` | ✅ |
| `htlc_sigall_preimage_only_fails` | ❌ |
| `htlc_sigall_signature_only_fails` | ❌ |
| `htlc_sigall_requires_preimage_and_transaction_signature` | ✅ |
| `htlc_sigall_wrong_preimage_fails` | ❌ |
| `htlc_sigall_locktime_after_expiry_refund_succeeds` | ✅ |
| `htlc_sigall_multisig_2of3` | ✅ |
| `htlc_sigall_receiver_path_after_locktime` | ✅ |

> ❌ `htlc_sigall_preimage_only_fails` @ `http://localhost:3030`: got 200

> ❌ `htlc_sigall_signature_only_fails` @ `http://localhost:3030`: got 200

> ❌ `htlc_sigall_wrong_preimage_fails` @ `http://localhost:3030`: got 200

## NUT-13 Deterministic Secrets

| Scenario | `http://localhost:3030` |
|---|---|
| `nut13_keyset_id_integer` | ✅ |
| `nut13_secret_derivation` | ✅ |
| `nut13_restore_works` | ✅ |

## NUT-18 Payment Request

| Scenario | `http://localhost:3030` |
|---|---|
| `nut18_payment_request_decode` | ✅ |
| `nut18_payment_request_amount` | ✅ |

## NUT-20 Quote Sig

| Scenario | `http://localhost:3030` |
|---|---|
| `nut20_locked_quote_requires_signature` | ❌ |
| `nut20_locked_quote_valid_signature_succeeds` | ✅ |
| `nut20_locked_quote_wrong_signature_fails` | ❌ |
| `nut20_quote_echoes_pubkey` | ❌ |

> ❌ `nut20_locked_quote_requires_signature` @ `http://localhost:3030`: expected rejection, got 200: {'signatures': [{'C_': '021ec45bcdd99ee388c7e9fb2ded4dab3e240865cb90eae304a77f56c7d3c64e96', 'amount': 8, 'dleq': {'e': '0ff3ec70303a4398eca0cc5536476ef916313be97664149298903f4ba023cf66', 's': 'b0e2b3

> ❌ `nut20_locked_quote_wrong_signature_fails` @ `http://localhost:3030`: expected rejection, got 200: {'signatures': [{'C_': '039a50afcc3e32bc719410ab9b32993530c690cd5251b50c92546af056864f0fc9', 'amount': 8, 'dleq': {'e': 'a74de9ea798691329d16820733a59692ab4c6583fb246166af438996868c1000', 's': 'cb8cf8

> ❌ `nut20_quote_echoes_pubkey` @ `http://localhost:3030`: expected pubkey=02e409156d4a79489672..., got ''

## NUT-26 Bech32m

| Scenario | `http://localhost:3030` |
|---|---|
| `nut26_encode_token_v4` | ✅ |
| `nut26_decode_token_v4` | ✅ |

## NUT-29 Batch Ops

| Scenario | `http://localhost:3030` |
|---|---|
| `batch_check_returns_quotes` | ❌ |
| `batch_check_rejects_too_many` | ❌ |
| `batch_mint_rejects_too_many_outputs` | ❌ |

> ❌ `batch_check_returns_quotes` @ `http://localhost:3030`: HTTP 405: 

> ❌ `batch_check_rejects_too_many` @ `http://localhost:3030`: expected batch_too_large, got 405: 

> ❌ `batch_mint_rejects_too_many_outputs` @ `http://localhost:3030`: expected too_many_outputs, got 404: 

## NUT-03 Swap Basics

| Scenario | `http://localhost:3030` |
|---|---|
| `swap_valid_proofs_succeeds` | ✅ |
| `swap_already_spent_fails` | ✅ |
| `swap_wrong_keyset_fails` | ✅ |

## NUT-04 Mint Quote Basics

| Scenario | `http://localhost:3030` |
|---|---|
| `mint_quote_creates_invoice` | ✅ |
| `mint_quote_zero_amount_fails` | ✅ |
| `mint_tokens_after_quote` | ✅ |

## NUT-05 Melt Basics

| Scenario | `http://localhost:3030` |
|---|---|
| `melt_quote_creates_quote` | ✅ |
| `melt_valid_proofs_succeeds` | ❌ |

> ❌ `melt_valid_proofs_succeeds` @ `http://localhost:3030`: got 400: {'code': 'AMOUNT_MISMATCH', 'detail': 'input/output amount mismatch', 'error_kind': 'AMOUNT_MISMATCH'}

## NUT-07 Checkstate Basics

| Scenario | `http://localhost:3030` |
|---|---|
| `checkstate_unspent_returns_unspent` | ✅ |
| `checkstate_spent_returns_spent` | ✅ |

## NUT-09 Restore Basics

| Scenario | `http://localhost:3030` |
|---|---|
| `restore_returns_signatures` | ✅ |

## NUT-00 Token Format Basics

| Scenario | `http://localhost:3030` |
|---|---|
| `token_v3_parses` | ✅ |
| `token_v4_parses` | ✅ |

## NUT-19 Cache Basics

| Scenario | `http://localhost:3030` |
|---|---|
| `mint_info_nut19_supported` | ❌ |

> ❌ `mint_info_nut19_supported` @ `http://localhost:3030`: nut19 not found in nuts list

## NUT-06 Mint Info Basics

| Scenario | `http://localhost:3030` |
|---|---|
| `mint_info_returns_required_fields` | ✅ |

## Security: Concurrency

| Scenario | `http://localhost:3030` |
|---|---|
| `concurrent_double_melt_rejected` | ❌ |
| `sequential_double_melt_rejected` | ❌ |

> ❌ `concurrent_double_melt_rejected` @ `http://localhost:3030`: neither melt paid (A=400/{'code': 'AMOUNT_MISMATCH', 'detail': 'input/output amount mismatch', 'error_kind': 'AMOUNT_MISMATCH'}, B=400/{'code': 'AMOUNT_MISMATCH', 'detail': 'input/output amount mismatch', 'error_kind': 'AMOUNT_MISMATCH'}) — quote may be unpayable

> ❌ `sequential_double_melt_rejected` @ `http://localhost:3030`: first melt did not pay (400/{'code': 'AMOUNT_MISMATCH', 'detail': 'input/output amount mismatch', 'error_kind': 'AMOUNT_MISMATCH'})
