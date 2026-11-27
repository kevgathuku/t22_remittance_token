use anchor_lang::prelude::Pubkey;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_zk_sdk::encryption::derivation::derive_confidential_keys;
use spl_token_2022_interface::{
    extension::{
        confidential_transfer::instruction as confidential_instruction,
        confidential_transfer::ConfidentialTransferMint,
        confidential_transfer_fee::ConfidentialTransferFeeConfig,
        default_account_state::DefaultAccountState, metadata_pointer::MetadataPointer,
        mint_close_authority::MintCloseAuthority, permanent_delegate::PermanentDelegate,
        transfer_fee::TransferFeeConfig, BaseStateWithExtensions, ExtensionType,
        StateWithExtensions,
    },
    state::{AccountState, Mint as MintState},
    ID as TOKEN_2022_PROGRAM_ID,
};

mod common;
use common::*;

/// The expected v2 extension set, in init order.
const EXTENSIONS: [ExtensionType; 7] = [
    ExtensionType::MetadataPointer,
    ExtensionType::MintCloseAuthority,
    ExtensionType::TransferFeeConfig,
    ExtensionType::DefaultAccountState,
    ExtensionType::PermanentDelegate,
    ExtensionType::ConfidentialTransferMint,
    ExtensionType::ConfidentialTransferFeeConfig,
];

fn fee_authority_elgamal(payer: &Keypair) -> [u8; 32] {
    // The withheld fee is a ciphertext, so the withdraw authority needs an
    // ElGamal key, not just an address. Only its holder can total the fees.
    let (elgamal, _) = derive_confidential_keys(payer, b"fee-authority").unwrap();
    elgamal.pubkey().into()
}

#[test]
fn mint_v2_carries_seizure_and_confidentiality() {
    let (mut svm, payer) = setup();
    let mint = create_mint_v2(&mut svm, &payer, fee_authority_elgamal(&payer));

    let account = svm.get_account(&mint.pubkey()).unwrap();
    assert_eq!(account.owner, TOKEN_2022_PROGRAM_ID);

    // v1's four extensions carried forward, plus seizure, confidentiality,
    // and the fee bridge Token-2022 demands for the fee+confidential pair.
    let expected = ExtensionType::try_calculate_account_len::<MintState>(&EXTENSIONS).unwrap();
    assert_eq!(account.data.len(), expected);

    let state = StateWithExtensions::<MintState>::unpack(&account.data).unwrap();

    assert_eq!(state.get_extension_types().unwrap(), EXTENSIONS.to_vec());

    // The carried-forward set is unchanged.
    let fee = state.get_extension::<TransferFeeConfig>().unwrap();
    assert_eq!(
        u16::from(fee.newer_transfer_fee.transfer_fee_basis_points),
        BASIS_POINTS
    );
    let pointer = state.get_extension::<MetadataPointer>().unwrap();
    assert_eq!(
        Option::<Pubkey>::from(pointer.metadata_address),
        Some(mint.pubkey())
    );
    let default_state = state.get_extension::<DefaultAccountState>().unwrap();
    assert_eq!(
        AccountState::try_from(default_state.state).unwrap(),
        AccountState::Frozen
    );
    let close = state.get_extension::<MintCloseAuthority>().unwrap();
    assert_eq!(
        Option::<Pubkey>::from(close.close_authority),
        Some(payer.pubkey())
    );

    // The issuer can seize funds from any account of this mint.
    let seize = state.get_extension::<PermanentDelegate>().unwrap();
    assert_eq!(Option::<Pubkey>::from(seize.delegate), Some(payer.pubkey()));

    // Manual approval: every confidential account waits on the authority,
    // and no auditor key exists that could decrypt all amounts.
    let confidential = state.get_extension::<ConfidentialTransferMint>().unwrap();
    assert_eq!(
        Option::<Pubkey>::from(confidential.authority),
        Some(payer.pubkey())
    );
    assert!(!bool::from(confidential.auto_approve_new_accounts));

    // The bridge: withheld fees stay confidential, decryptable only under
    // the withdraw authority's ElGamal key.
    let confidential_fee = state
        .get_extension::<ConfidentialTransferFeeConfig>()
        .unwrap();
    assert_eq!(
        Option::<Pubkey>::from(confidential_fee.authority),
        Some(payer.pubkey())
    );
}

#[test]
fn confidentiality_cannot_be_added_after_creation() {
    let (mut svm, payer) = setup();
    let mint = create_mint(&mut svm, &payer);

    // The gap that forces the re-issue: extensions initialize only before
    // `InitializeMint2`, so a live v1 mint can never gain confidentiality
    // (or a seizure authority) in place.
    let err = send_raw(
        &mut svm,
        &payer,
        &[confidential_instruction::initialize_mint(
            &TOKEN_2022_PROGRAM_ID,
            &mint.pubkey(),
            Some(payer.pubkey()),
            false,
            None,
        )
        .unwrap()],
        &[],
    )
    .expect_err("adding confidentiality to a live mint should fail");

    assert!(
        format!("{:?}", err.meta.logs).contains("already in use"),
        "rejected for the wrong reason: {:?}",
        err.err
    );
}
