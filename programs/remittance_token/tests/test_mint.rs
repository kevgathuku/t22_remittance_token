use anchor_lang::prelude::Pubkey;
use solana_instruction_error::InstructionError;
use solana_keypair::Keypair;
use solana_signer::Signer;
use spl_token_2022_interface::{
    extension::{
        default_account_state::DefaultAccountState, metadata_pointer::MetadataPointer,
        mint_close_authority::MintCloseAuthority,
        transfer_fee::instruction::initialize_transfer_fee_config, transfer_fee::TransferFeeConfig,
        BaseStateWithExtensions, ExtensionType, StateWithExtensions,
    },
    instruction::initialize_mint2,
    state::{AccountState, Mint as MintState},
};

mod common;
use common::*;
use remittance_token::{MintError, MAX_FEE_BASIS_POINTS};

#[test]
fn mint_carries_exactly_the_four_declared_extensions() {
    let (mut svm, payer) = setup();
    let mint = create_mint(&mut svm, &payer);

    let account = svm.get_account(&mint.pubkey()).unwrap();
    assert_eq!(account.owner, spl_token_2022_interface::ID);

    // The same calculation the program runs, computed here independently.
    let expected = ExtensionType::try_calculate_account_len::<MintState>(&[
        ExtensionType::MetadataPointer,
        ExtensionType::MintCloseAuthority,
        ExtensionType::TransferFeeConfig,
        ExtensionType::DefaultAccountState,
    ])
    .unwrap();
    assert_eq!(account.data.len(), expected);

    // All state reads go through `StateWithExtensions`, never raw unpack.
    let state = StateWithExtensions::<MintState>::unpack(&account.data).unwrap();

    assert_eq!(state.base.decimals, DECIMALS);
    assert_eq!(
        Option::<Pubkey>::from(state.base.mint_authority),
        Some(payer.pubkey())
    );
    assert_eq!(
        Option::<Pubkey>::from(state.base.freeze_authority),
        Some(payer.pubkey())
    );
    // TLV entries land in init order.
    assert_eq!(
        state.get_extension_types().unwrap(),
        vec![
            ExtensionType::MetadataPointer,
            ExtensionType::MintCloseAuthority,
            ExtensionType::TransferFeeConfig,
            ExtensionType::DefaultAccountState,
        ]
    );

    let pointer = state.get_extension::<MetadataPointer>().unwrap();
    assert_eq!(
        Option::<Pubkey>::from(pointer.authority),
        Some(payer.pubkey())
    );
    assert_eq!(
        Option::<Pubkey>::from(pointer.metadata_address),
        Some(mint.pubkey())
    );

    let close = state.get_extension::<MintCloseAuthority>().unwrap();
    assert_eq!(
        Option::<Pubkey>::from(close.close_authority),
        Some(payer.pubkey())
    );

    let fee = state.get_extension::<TransferFeeConfig>().unwrap();
    assert_eq!(
        Option::<Pubkey>::from(fee.transfer_fee_config_authority),
        Some(payer.pubkey())
    );
    assert_eq!(
        Option::<Pubkey>::from(fee.withdraw_withheld_authority),
        Some(payer.pubkey())
    );
    assert_eq!(
        u16::from(fee.newer_transfer_fee.transfer_fee_basis_points),
        BASIS_POINTS
    );
    assert_eq!(u64::from(fee.newer_transfer_fee.maximum_fee), MAXIMUM_FEE);

    // New accounts start frozen until KYC clears them.
    let default_state = state.get_extension::<DefaultAccountState>().unwrap();
    assert_eq!(
        AccountState::try_from(default_state.state).unwrap(),
        AccountState::Frozen
    );
}

#[test]
fn fee_above_maximum_is_rejected() {
    let (mut svm, payer) = setup();
    let mint = Keypair::new();

    let err = send_raw(
        &mut svm,
        &payer,
        &[create_mint_ix(
            &payer.pubkey(),
            &mint.pubkey(),
            MAX_FEE_BASIS_POINTS + 1,
        )],
        &[&mint],
    )
    .expect_err("fee above maximum should fail");

    assert!(
        format!("{:?}", err.meta.logs).contains(&format!("{:?}", MintError::InvalidFee)),
        "rejected for the wrong reason: {:?}",
        err.err
    );
}

#[test]
fn extension_init_after_mint_init_fails() {
    use spl_token_2022_interface::ID as TOKEN_2022_PROGRAM_ID;

    let (mut svm, payer) = setup();
    let mint = Keypair::new();

    // Same instructions the program issues, except `InitializeMint2` runs
    // first: extensions can only be added before the mint is initialized.
    let space =
        ExtensionType::try_calculate_account_len::<MintState>(&[ExtensionType::TransferFeeConfig])
            .unwrap();
    let lamports = svm.minimum_balance_for_rent_exemption(space);

    let err = send_raw(
        &mut svm,
        &payer,
        &[
            solana_system_interface::instruction::create_account(
                &payer.pubkey(),
                &mint.pubkey(),
                lamports,
                space as u64,
                &TOKEN_2022_PROGRAM_ID,
            ),
            initialize_mint2(
                &TOKEN_2022_PROGRAM_ID,
                &mint.pubkey(),
                &payer.pubkey(),
                Some(&payer.pubkey()),
                DECIMALS,
            )
            .unwrap(),
            initialize_transfer_fee_config(
                &TOKEN_2022_PROGRAM_ID,
                &mint.pubkey(),
                Some(&payer.pubkey()),
                Some(&payer.pubkey()),
                BASIS_POINTS,
                MAXIMUM_FEE,
            )
            .unwrap(),
        ],
        &[&mint],
    )
    .expect_err("extension init after mint init should fail");

    // Match exact Token-2022's error name
    assert!(
        format!("{:?}", err.meta.logs).contains(&format!(
            "{:?}",
            InstructionError::InvalidAccountData
        )),
        "wrong error: {:?}",
        err.err
    );
}
