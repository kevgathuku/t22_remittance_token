use anchor_lang::{
    prelude::Pubkey, solana_program::instruction::Instruction, InstructionData, ToAccountMetas,
};
use litesvm::LiteSVM;
use remittance_token::{accounts, instruction, ID};
use solana_keypair::Keypair;
use solana_signer::Signer;
use spl_token_2022_interface::{
    extension::StateWithExtensions,
    state::{Account as TokenAccount, AccountState},
    ID as TOKEN_2022_PROGRAM_ID,
};

mod common;
use common::*;

fn thaw_via_program_ix(freeze_authority: &Pubkey, account: &Pubkey, mint: &Pubkey) -> Instruction {
    Instruction {
        program_id: ID,
        accounts: accounts::ThawAccount {
            freeze_authority: *freeze_authority,
            token_account: *account,
            mint: *mint,
            token_program: TOKEN_2022_PROGRAM_ID,
        }
        .to_account_metas(None),
        data: instruction::ThawAccount {}.data(),
    }
}

fn account_state(svm: &LiteSVM, address: &Pubkey) -> AccountState {
    let account = svm.get_account(address).unwrap();
    let state = StateWithExtensions::<TokenAccount>::unpack(&account.data).unwrap();
    state.base.state
}

#[test]
fn thaw_via_program_clears_kyc() {
    let (mut svm, payer) = setup();
    let mint = create_mint(&mut svm, &payer);
    let alice = Keypair::new();
    let bob = Keypair::new();

    // New accounts start frozen.
    let source = token_account(&mut svm, &payer, &mint.pubkey(), &alice.pubkey());
    assert_eq!(account_state(&svm, &source), AccountState::Frozen);

    // The freeze authority clears this holder after off-chain KYC.
    send(
        &mut svm,
        &payer,
        &[thaw_via_program_ix(
            &payer.pubkey(),
            &source,
            &mint.pubkey(),
        )],
        &[],
    );
    assert_eq!(account_state(&svm, &source), AccountState::Initialized);

    // And the holder can now move funds.
    fund(&mut svm, &payer, &mint.pubkey(), &source, MINT_AMOUNT);
    let destination = funded_account(&mut svm, &payer, &mint.pubkey(), &bob, 0);
    send(
        &mut svm,
        &payer,
        &[transfer_ix(
            &alice.pubkey(),
            &source,
            &destination,
            &mint.pubkey(),
            TRANSFER_AMOUNT,
        )],
        &[&alice],
    );
    assert_eq!(
        read_holding(&svm, &destination),
        (TRANSFER_AMOUNT - EXPECTED_FEE, EXPECTED_FEE)
    );
}

#[test]
fn thaw_by_non_authority_fails() {
    let (mut svm, payer) = setup();
    let mint = create_mint(&mut svm, &payer);
    let holder = Keypair::new();
    let stranger = Keypair::new();

    let account = token_account(&mut svm, &payer, &mint.pubkey(), &holder.pubkey());

    // Stranger pays nothing and proves nothing: the mint's freeze authority
    // is the payer, and Token-2022 checks it, not us.
    let err = send_raw(
        &mut svm,
        &payer,
        &[thaw_via_program_ix(
            &stranger.pubkey(),
            &account,
            &mint.pubkey(),
        )],
        &[&stranger],
    )
    .expect_err("thaw by non-authority should fail");

    // Token-2022 logs the message text here, not the variant name.
    assert!(
        format!("{:?}", err.meta.logs).contains("does not match"),
        "rejected for the wrong reason: {:?}",
        err.err
    );
    assert_eq!(account_state(&svm, &account), AccountState::Frozen);
}
