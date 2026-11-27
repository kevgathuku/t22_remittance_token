use anchor_lang::{
    prelude::Pubkey, solana_program::instruction::Instruction, InstructionData, ToAccountMetas,
};
use litesvm::LiteSVM;
use remittance_token::{accounts, instruction, ID};
use solana_compute_budget_interface::ComputeBudgetInstruction;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_zk_elgamal_proof_interface::{
    instruction::{close_context_state, ContextStateInfo, ProofInstruction},
    proof_data::ZkProofData,
    state::ProofContextState,
    ID as ZK_PROGRAM_ID,
};
use solana_zk_sdk::encryption::{
    auth_encryption::{AeCiphertext, AeKey},
    derivation::derive_confidential_keys,
    elgamal::{ElGamalCiphertext, ElGamalKeypair, ElGamalPubkey},
};
use spl_token_2022_interface::{
    extension::{
        confidential_transfer::{instruction as ct_ix, ConfidentialTransferAccount},
        BaseStateWithExtensions, ExtensionType, StateWithExtensions,
    },
    state::Account as TokenAccountState,
    ID as TOKEN_2022_PROGRAM_ID,
};
use spl_token_confidential_transfer_proof_extraction::instruction::ProofLocation;
use spl_token_confidential_transfer_proof_generation::{
    transfer_with_fee::transfer_with_fee_split_proof_data, withdraw::withdraw_proof_data,
};
use std::num::NonZeroI8;

mod common;
use common::*;

const TRANSFER_AMOUNT_CT: u64 = 2_500;
// ceil(2_500 * 50 / 10_000), well under the cap.
const TRANSFER_FEE_CT: u64 = 13;
const WITHDRAW_AMOUNT: u64 = 1_000;

struct Holder {
    account: Pubkey,
    elgamal: ElGamalKeypair,
    aes: AeKey,
}

fn fee_keys(payer: &Keypair) -> (ElGamalKeypair, [u8; 32]) {
    let (elgamal, _) = derive_confidential_keys(payer, b"fee-authority").unwrap();
    let pubkey: [u8; 32] = elgamal.pubkey().into();
    (elgamal, pubkey)
}

fn create_and_configure(
    svm: &mut LiteSVM,
    payer: &Keypair,
    mint: &Pubkey,
    owner: &Keypair,
) -> Holder {
    let ta = Keypair::new();
    let space = ExtensionType::try_calculate_account_len::<TokenAccountState>(&[
        ExtensionType::ConfidentialTransferAccount,
        ExtensionType::TransferFeeAmount,
        ExtensionType::ConfidentialTransferFeeAmount,
    ])
    .unwrap();
    let lamports = svm.minimum_balance_for_rent_exemption(space);
    send(
        svm,
        payer,
        &[
            solana_system_interface::instruction::create_account(
                &payer.pubkey(),
                &ta.pubkey(),
                lamports,
                space as u64,
                &TOKEN_2022_PROGRAM_ID,
            ),
            spl_token_2022_interface::instruction::initialize_account3(
                &TOKEN_2022_PROGRAM_ID,
                &ta.pubkey(),
                mint,
                &owner.pubkey(),
            )
            .unwrap(),
        ],
        &[&ta],
    );

    // Owner-only: configuring needs the owner's signature and keys derived
    // from it. Anyone can create the underlying token account; only the
    // owner can configure it for confidential transfers.
    let (elgamal, aes) = derive_confidential_keys(owner, b"").unwrap();
    let proof =
        solana_zk_sdk::zk_elgamal_proof_program::pubkey_validity::build_pubkey_validity_proof_data(
            &elgamal,
        )
        .unwrap();
    let ixs = ct_ix::configure_account(
        &TOKEN_2022_PROGRAM_ID,
        &ta.pubkey(),
        mint,
        &aes.encrypt(0).into(),
        65536,
        &owner.pubkey(),
        &[],
        spl_token_confidential_transfer_proof_extraction::instruction::ProofLocation::InstructionOffset(
            NonZeroI8::new(1).unwrap(),
            &proof,
        ),
    )
    .unwrap();
    send(svm, payer, &ixs, &[owner]);

    // Manual approval: the mint authority must approve before use.
    send(
        svm,
        payer,
        &[ct_ix::approve_account(
            &TOKEN_2022_PROGRAM_ID,
            &ta.pubkey(),
            mint,
            &payer.pubkey(),
            &[],
        )
        .unwrap()],
        &[],
    );

    Holder {
        account: ta.pubkey(),
        elgamal,
        aes,
    }
}

fn read_ct(svm: &LiteSVM, account: &Pubkey) -> ConfidentialTransferAccount {
    let acct = svm.get_account(account).unwrap();
    let state = StateWithExtensions::<TokenAccountState>::unpack(&acct.data).unwrap();
    *state
        .get_extension::<ConfidentialTransferAccount>()
        .unwrap()
}

fn available_balance(ct: &ConfidentialTransferAccount, elgamal: &ElGamalKeypair) -> u64 {
    let ciphertext: ElGamalCiphertext = ct.available_balance.try_into().unwrap();
    elgamal.secret().decrypt_u32(&ciphertext).unwrap()
}

fn pending_balance(ct: &ConfidentialTransferAccount, elgamal: &ElGamalKeypair) -> u64 {
    let lo: ElGamalCiphertext = ct.pending_balance_lo.try_into().unwrap();
    let hi: ElGamalCiphertext = ct.pending_balance_hi.try_into().unwrap();
    let lo = elgamal.secret().decrypt_u32(&lo).unwrap();
    let hi = elgamal.secret().decrypt_u32(&hi).unwrap();
    lo + (hi << 16)
}

fn deposit_ix(account: &Pubkey, mint: &Pubkey, authority: &Pubkey, amount: u64) -> Instruction {
    Instruction {
        program_id: ID,
        accounts: accounts::DepositConfidential {
            token_account: *account,
            mint: *mint,
            authority: *authority,
            token_program: TOKEN_2022_PROGRAM_ID,
        }
        .to_account_metas(None),
        data: instruction::DepositConfidential {
            amount,
            decimals: DECIMALS,
        }
        .data(),
    }
}

fn apply_pending(svm: &mut LiteSVM, payer: &Keypair, holder: &Holder, owner: &Keypair) {
    let ct = read_ct(svm, &holder.account);
    let counter: u64 = ct.pending_balance_credit_counter.into();
    let new_available =
        available_balance(&ct, &holder.elgamal) + pending_balance(&ct, &holder.elgamal);
    let ciphertext: [u8; 36] = holder.aes.encrypt(new_available).to_bytes();

    send(
        svm,
        payer,
        &[Instruction {
            program_id: ID,
            accounts: accounts::ApplyPendingBalance {
                token_account: holder.account,
                authority: owner.pubkey(),
                token_program: TOKEN_2022_PROGRAM_ID,
            }
            .to_account_metas(None),
            data: instruction::ApplyPendingBalance {
                expected_pending_balance_credit_counter: counter,
                new_decryptable_available_balance: ciphertext,
            }
            .data(),
        }],
        &[owner],
    );
}

fn stage_proof<T, U>(
    svm: &mut LiteSVM,
    payer: &Keypair,
    instruction_kind: ProofInstruction,
    proof: &T,
) -> Pubkey
where
    T: bytemuck::Pod + ZkProofData<U>,
    U: bytemuck::Pod,
{
    let context_len = std::mem::size_of::<ProofContextState<U>>();
    let context = Keypair::new();
    let lamports = svm.minimum_balance_for_rent_exemption(context_len);
    send(
        svm,
        payer,
        &[solana_system_interface::instruction::create_account(
            &payer.pubkey(),
            &context.pubkey(),
            lamports,
            context_len as u64,
            &ZK_PROGRAM_ID,
        )],
        &[&context],
    );
    let ix = instruction_kind.encode_verify_proof(
        Some(ContextStateInfo {
            context_state_account: &context.pubkey(),
            context_state_authority: &payer.pubkey(),
        }),
        proof,
    );
    send(
        svm,
        payer,
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            ix,
        ],
        &[],
    );
    context.pubkey()
}

fn close_contexts(svm: &mut LiteSVM, payer: &Keypair, contexts: &[Pubkey]) {
    let ixs: Vec<Instruction> = contexts
        .iter()
        .map(|c| {
            close_context_state(
                ContextStateInfo {
                    context_state_account: c,
                    context_state_authority: &payer.pubkey(),
                },
                &payer.pubkey(),
            )
        })
        .collect();
    send(svm, payer, &ixs, &[]);
    for c in contexts {
        assert!(svm
            .get_account(c)
            .map(|a| a.data.is_empty())
            .unwrap_or(true));
    }
}

#[test]
fn full_confidential_lifecycle() {
    let (mut svm, payer) = setup();
    let (_fee_elgamal, fee_pubkey) = fee_keys(&payer);
    let mint = create_mint_v2(&mut svm, &payer, fee_pubkey);

    // Owner-only configuration: anyone could have created the token
    // accounts, but only each owner can configure theirs.
    let alice_owner = payer.insecure_clone();
    let bob_owner = Keypair::new();
    svm.airdrop(&bob_owner.pubkey(), 10_000_000_000).unwrap();

    let alice = create_and_configure(&mut svm, &payer, &mint.pubkey(), &alice_owner);
    let bob = create_and_configure(&mut svm, &payer, &mint.pubkey(), &bob_owner);

    // Public funding needs thawed accounts even though configuration did not.
    thaw(&mut svm, &payer, &mint.pubkey(), &alice.account);
    thaw(&mut svm, &payer, &mint.pubkey(), &bob.account);

    // Public funding, then deposit into the pending confidential balance.
    send(
        &mut svm,
        &payer,
        &[spl_token_2022_interface::instruction::mint_to(
            &TOKEN_2022_PROGRAM_ID,
            &mint.pubkey(),
            &alice.account,
            &payer.pubkey(),
            &[],
            MINT_AMOUNT,
        )
        .unwrap()],
        &[],
    );
    send(
        &mut svm,
        &payer,
        &[deposit_ix(
            &alice.account,
            &mint.pubkey(),
            &alice_owner.pubkey(),
            MINT_AMOUNT,
        )],
        &[&alice_owner],
    );
    let ct = read_ct(&svm, &alice.account);
    assert_eq!(pending_balance(&ct, &alice.elgamal), MINT_AMOUNT);
    assert_eq!(available_balance(&ct, &alice.elgamal), 0);

    // Pending becomes available.
    apply_pending(&mut svm, &payer, &alice, &alice_owner);
    let ct = read_ct(&svm, &alice.account);
    assert_eq!(available_balance(&ct, &alice.elgamal), MINT_AMOUNT);

    // Confidential transfer with fee: the fee stays encrypted under the
    // withdraw authority's key, so the proof needs that extra pubkey.
    let fee_elgamal_pubkey = {
        let (fee_elgamal, _) = fee_keys(&payer);
        fee_elgamal.pubkey_owned()
    };
    let ct = read_ct(&svm, &alice.account);
    let current_available: ElGamalCiphertext = ct.available_balance.try_into().unwrap();
    let current_decryptable: AeCiphertext = ct.decryptable_available_balance.try_into().unwrap();
    let bob_ct = read_ct(&svm, &bob.account);
    let bob_pubkey: ElGamalPubkey = bob_ct.elgamal_pubkey.try_into().unwrap();

    let proofs = transfer_with_fee_split_proof_data(
        &current_available,
        &current_decryptable,
        TRANSFER_AMOUNT_CT,
        &alice.elgamal,
        &alice.aes,
        &bob_pubkey,
        None,
        &fee_elgamal_pubkey,
        BASIS_POINTS,
        MAXIMUM_FEE,
    )
    .unwrap();

    let eq_ctx = stage_proof(
        &mut svm,
        &payer,
        ProofInstruction::VerifyCiphertextCommitmentEquality,
        &proofs.equality_proof_data,
    );
    let val_ctx = stage_proof(
        &mut svm,
        &payer,
        ProofInstruction::VerifyBatchedGroupedCiphertext3HandlesValidity,
        &proofs
            .transfer_amount_ciphertext_validity_proof_data_with_ciphertext
            .proof_data,
    );
    let sigma_ctx = stage_proof(
        &mut svm,
        &payer,
        ProofInstruction::VerifyPercentageWithCap,
        &proofs.percentage_with_cap_proof_data,
    );
    let fee_ctx = stage_proof(
        &mut svm,
        &payer,
        ProofInstruction::VerifyBatchedGroupedCiphertext2HandlesValidity,
        &proofs.fee_ciphertext_validity_proof_data,
    );
    let range_ctx = stage_proof(
        &mut svm,
        &payer,
        ProofInstruction::VerifyBatchedRangeProofU256,
        &proofs.range_proof_data,
    );

    let new_alice_decryptable = alice.aes.encrypt(MINT_AMOUNT - TRANSFER_AMOUNT_CT);
    let ixs = ct_ix::transfer_with_fee(
        &TOKEN_2022_PROGRAM_ID,
        &alice.account,
        &mint.pubkey(),
        &bob.account,
        &new_alice_decryptable.into(),
        &proofs
            .transfer_amount_ciphertext_validity_proof_data_with_ciphertext
            .ciphertext_lo,
        &proofs
            .transfer_amount_ciphertext_validity_proof_data_with_ciphertext
            .ciphertext_hi,
        &alice_owner.pubkey(),
        &[],
        ProofLocation::ContextStateAccount(&eq_ctx),
        ProofLocation::ContextStateAccount(&val_ctx),
        ProofLocation::ContextStateAccount(&sigma_ctx),
        ProofLocation::ContextStateAccount(&fee_ctx),
        ProofLocation::ContextStateAccount(&range_ctx),
    )
    .unwrap();
    send(&mut svm, &payer, &ixs, &[&alice_owner]);
    close_contexts(
        &mut svm,
        &payer,
        &[eq_ctx, val_ctx, sigma_ctx, fee_ctx, range_ctx],
    );

    // Bob's share lands in pending, net of the encrypted fee.
    let bob_ct = read_ct(&svm, &bob.account);
    assert_eq!(
        pending_balance(&bob_ct, &bob.elgamal),
        TRANSFER_AMOUNT_CT - TRANSFER_FEE_CT
    );
    assert_eq!(available_balance(&bob_ct, &bob.elgamal), 0);

    apply_pending(&mut svm, &payer, &bob, &bob_owner);
    let bob_ct = read_ct(&svm, &bob.account);
    assert_eq!(
        available_balance(&bob_ct, &bob.elgamal),
        TRANSFER_AMOUNT_CT - TRANSFER_FEE_CT
    );

    // Withdraw back to public: pending must be applied first, so the
    // available balance the proof commits to is current.
    let bob_ct = read_ct(&svm, &bob.account);
    let bob_available = available_balance(&bob_ct, &bob.elgamal);
    let bob_current: ElGamalCiphertext = bob_ct.available_balance.try_into().unwrap();
    let wproofs =
        withdraw_proof_data(&bob_current, bob_available, WITHDRAW_AMOUNT, &bob.elgamal).unwrap();

    let weq_ctx = stage_proof(
        &mut svm,
        &payer,
        ProofInstruction::VerifyCiphertextCommitmentEquality,
        &wproofs.equality_proof_data,
    );
    let wrange_ctx = stage_proof(
        &mut svm,
        &payer,
        ProofInstruction::VerifyBatchedRangeProofU64,
        &wproofs.range_proof_data,
    );

    let new_bob_decryptable = bob.aes.encrypt(bob_available - WITHDRAW_AMOUNT);
    let ixs = ct_ix::withdraw(
        &TOKEN_2022_PROGRAM_ID,
        &bob.account,
        &mint.pubkey(),
        WITHDRAW_AMOUNT,
        DECIMALS,
        &new_bob_decryptable.into(),
        &bob_owner.pubkey(),
        &[],
        ProofLocation::ContextStateAccount(&weq_ctx),
        ProofLocation::ContextStateAccount(&wrange_ctx),
    )
    .unwrap();
    send(&mut svm, &payer, &ixs, &[&bob_owner]);
    close_contexts(&mut svm, &payer, &[weq_ctx, wrange_ctx]);

    let acct = svm.get_account(&bob.account).unwrap();
    let state = StateWithExtensions::<TokenAccountState>::unpack(&acct.data).unwrap();
    assert_eq!(state.base.amount, WITHDRAW_AMOUNT);

    let bob_ct = read_ct(&svm, &bob.account);
    assert_eq!(
        available_balance(&bob_ct, &bob.elgamal),
        TRANSFER_AMOUNT_CT - TRANSFER_FEE_CT - WITHDRAW_AMOUNT
    );
}
