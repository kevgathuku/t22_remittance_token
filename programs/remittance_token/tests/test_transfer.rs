use solana_keypair::Keypair;
use solana_signer::Signer;

mod common;
use common::*;

#[test]
fn transfer_withholds_the_protocol_fee() {
    let (mut svm, payer) = setup();
    let mint = create_mint(&mut svm, &payer);
    let alice = Keypair::new();
    let bob = Keypair::new();
    let source = funded_account(&mut svm, &payer, &mint.pubkey(), &alice, MINT_AMOUNT);
    let destination = funded_account(&mut svm, &payer, &mint.pubkey(), &bob, 0);

    // The fee is a live calculation, not a constant: read the on-chain config
    // and run the same epoch schedule the program runs.
    assert_eq!(
        expected_fee(&svm, &mint.pubkey(), TRANSFER_AMOUNT),
        EXPECTED_FEE
    );

    // The funded payer covers fees; Alice only signs as source owner.
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

    // Whole amount leaves the source
    assert_eq!(
        read_holding(&svm, &source),
        (MINT_AMOUNT - TRANSFER_AMOUNT, 0)
    );
    // the recipient is credited net of fee
    assert_eq!(
        read_holding(&svm, &destination),
        (TRANSFER_AMOUNT - EXPECTED_FEE, EXPECTED_FEE)
    );
}

#[test]
fn transfer_from_a_frozen_account_fails() {
    let (mut svm, payer) = setup();
    let mint = create_mint(&mut svm, &payer);

    // Funded, then frozen again: KYC cleared and later revoked.
    let holder = Keypair::new();
    let account = funded_account(&mut svm, &payer, &mint.pubkey(), &holder, MINT_AMOUNT);
    freeze(&mut svm, &payer, &mint.pubkey(), &account);

    let destination = funded_account(&mut svm, &payer, &mint.pubkey(), &payer, 0);
    let result = send_raw(
        &mut svm,
        &payer,
        &[transfer_ix(
            &holder.pubkey(),
            &account,
            &destination,
            &mint.pubkey(),
            TRANSFER_AMOUNT,
        )],
        &[&holder],
    );

    // Token-2022 logs the message text here, not the variant name
    let err = result.expect_err("transfer from a frozen account should fail");
    assert!(
        format!("{:?}", err.meta.logs).contains("frozen"),
        "blocked for the wrong reason: {:?}",
        err.err
    );
}
