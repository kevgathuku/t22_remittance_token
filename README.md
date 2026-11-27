# remittance_token

[![CI](https://github.com/kevgathuku/remittance_token/actions/workflows/ci.yml/badge.svg)](https://github.com/kevgathuku/remittance_token/actions/workflows/ci.yml)

A remittance stablecoin on Token-2022. The issuer earns a protocol fee on every transfer, new accounts start frozen until KYC clears them, and a re-issued mint adds seizure authority plus confidential transfers.

1. Creates the v1 mint stacking `TransferFeeConfig`, `MetadataPointer` (at the mint itself), `DefaultAccountState` (frozen), and `MintCloseAuthority` — every extension init ordered before `InitializeMint2` (`create_mint`)
2. Transfers with the protocol fee computed live via `calculate_epoch_fee`, never a cached rate (`transfer`)
3. Unfreezes one holder at a time after off-chain KYC, separate from the mint-level default (`thaw_account`)
4. Re-issues the mint as v2, carrying v1's set plus `PermanentDelegate` (seizure) and `ConfidentialTransferMint` (manual approval) — with the `ConfidentialTransferFeeConfig` bridge Token-2022 demands for the fee+confidential pair (`create_mint_v2`)
5. Runs the full confidential lifecycle: owner-only configure, program-driven deposit and apply-pending, a with-fee confidential transfer, and withdraw after applying pending (`deposit_confidential`, `apply_pending_balance`, client-side configure/transfer/withdraw)

Accounts are read exclusively through `StateWithExtensions` — the typed Anchor account hides extensions, so the program re-borrows the raw bytes wherever it needs them.

## Setup

Requirements: Rust 1.98.1 (see `rust-toolchain.toml`), Solana CLI, Anchor CLI, a funded localnet wallet.

Wallet for the provider (see `Anchor.toml`):

```txt
~/.config/solana/id.json
```

## Commands

```bash
# build the program
anchor build
# run all LiteSVM tests
anchor test
```

## Tests

### mint

Issue the v1 mint and prove the layout.

Verifies:

- mint carries exactly the four declared extensions at the `try_calculate_account_len` size, owned by Token-2022.
- base authorities, fee schedule, metadata pointer (at the mint), frozen default, and close authority all read back through `StateWithExtensions`.
- TLV entries land in init order.
- Rejects fees above 100% (`InvalidFee`).
- Rejects extension init after `InitializeMint2` (`InvalidAccountData`) — the rule that forces the v2 re-issue.

### transfer

Move tokens with the fee withheld.

Verifies:

- the fee is computed from the live on-chain config, not a constant, and matches the withheld amount.
- the source is debited whole; the destination is credited net of fee.
- Rejects transfers from a frozen account (Token-2022 `"frozen"`).

### thaw

Unfreeze one account at a time through the program instruction.

Verifies:

- thaw flips a frozen account to `Initialized`, after which funding and transfers work.
- Rejects thaw from anyone but the freeze authority (`"does not match"`), leaving the account frozen.

### mint_v2

Re-issue with seizure and confidentiality, and prove the re-issue was necessary.

Verifies:

- v2 carries all seven extensions (v1's four plus seizure, confidential mint, and the fee bridge) at the calculated size, with the withdraw authority, manual approval (`auto_approve: false`), and no auditor key.
- Adding confidentiality to the live v1 mint fails (`"already in use"`) — the gap v2 exists to close.

### confidential

The end-to-end confidential lifecycle on the v2 mint.

Verifies:

- only each owner can configure their account (creation is by anyone, configuration is owner-signed with a pubkey-validity proof), then the mint authority approves under the manual policy.
- program-driven deposit lands in pending; program-driven apply moves it to available.
- a with-fee confidential transfer credits the receiver net of the encrypted fee (2,500 → 2,487 pending).
- the receiver applies pending, then withdraws 1,000 back to the public balance.
- proof context accounts are closed and their rent reclaimed.

## Notes

- Tests run against the freshly built `.so` — rebuild after any `src/` change
- Tests build instructions through shared helpers in `programs/remittance_token/tests/common/mod.rs`
- Token setup (mints, accounts, funding) goes through real Token-2022 transactions; the bundled LiteSVM 0.16 programs verify the zero-knowledge proofs in-process
- The confidential test generates real proofs
- State-changing test setup (e.g. thawing, approving) goes through real instructions, not direct account writes
