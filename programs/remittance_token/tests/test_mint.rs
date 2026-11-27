use anchor_lang::{
    prelude::Pubkey,
    solana_program::{instruction::Instruction, system_program},
    InstructionData, ToAccountMetas,
};
use litesvm::LiteSVM;
use solana_keypair::Keypair;
use solana_message::Message;
use solana_signer::Signer;
use solana_transaction::Transaction;
use spl_token_2022_interface::{
    extension::{
        default_account_state::DefaultAccountState, metadata_pointer::MetadataPointer,
        mint_close_authority::MintCloseAuthority,
        transfer_fee::instruction::initialize_transfer_fee_config,
        transfer_fee::TransferFeeConfig, BaseStateWithExtensions, ExtensionType,
        StateWithExtensions,
    },
    instruction::initialize_mint2,
    state::{AccountState, Mint as MintState},
    ID as TOKEN_2022_PROGRAM_ID,
};
use remittance_token::{accounts, instruction, MAX_FEE_BASIS_POINTS, ID};

const DECIMALS: u8 = 6;
const BASIS_POINTS: u16 = 50;
const MAXIMUM_FEE: u64 = 100_000;

fn setup() -> (LiteSVM, Keypair) {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();

    let program_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/deploy/remittance_token.so");
    assert!(
        program_path.exists(),
        "program binary not found at {}. Run anchor build first.",
        program_path.display()
    );
    svm.add_program_from_file(ID, program_path).unwrap();

    (svm, payer)
}

/// Send instructions and require success.
fn send(svm: &mut LiteSVM, payer: &Keypair, ixs: &[Instruction], extra_signers: &[&Keypair]) {
    let mut signers: Vec<&Keypair> = vec![payer];
    signers.extend_from_slice(extra_signers);

    let blockhash = svm.latest_blockhash();
    let mut transaction = Transaction::new_unsigned(Message::new(ixs, Some(&payer.pubkey())));
    transaction.try_sign(&signers, blockhash).unwrap();

    if let Err(err) = svm.send_transaction(transaction) {
        panic!("transaction failed:\n{err:#?}");
    }
}

/// Send instructions and require failure, returning the rendered logs.
fn send_expecting_failure(
    svm: &mut LiteSVM,
    payer: &Keypair,
    ixs: &[Instruction],
    extra_signers: &[&Keypair],
) -> String {
    let mut signers: Vec<&Keypair> = vec![payer];
    signers.extend_from_slice(extra_signers);

    let blockhash = svm.latest_blockhash();
    let mut transaction = Transaction::new_unsigned(Message::new(ixs, Some(&payer.pubkey())));
    transaction.try_sign(&signers, blockhash).unwrap();

    match svm.send_transaction(transaction) {
        Ok(_) => panic!("expected the transaction to fail, but it succeeded"),
        Err(err) => format!("{err:#?}"),
    }
}

fn create_mint_ix(payer: &Pubkey, mint: &Pubkey, basis_points: u16) -> Instruction {
    Instruction {
        program_id: ID,
        accounts: accounts::CreateMint {
            payer: *payer,
            mint: *mint,
            token_program: TOKEN_2022_PROGRAM_ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: instruction::CreateMint {
            decimals: DECIMALS,
            basis_points,
            maximum_fee: MAXIMUM_FEE,
        }
        .data(),
    }
}

#[test]
fn mint_carries_exactly_the_four_declared_extensions() {
    let (mut svm, payer) = setup();
    let mint = Keypair::new();

    send(
        &mut svm,
        &payer,
        &[create_mint_ix(&payer.pubkey(), &mint.pubkey(), BASIS_POINTS)],
        &[&mint],
    );

    let account = svm.get_account(&mint.pubkey()).unwrap();
    assert_eq!(account.owner, TOKEN_2022_PROGRAM_ID);

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

    let logs = send_expecting_failure(
        &mut svm,
        &payer,
        &[create_mint_ix(
            &payer.pubkey(),
            &mint.pubkey(),
            MAX_FEE_BASIS_POINTS + 1,
        )],
        &[&mint],
    );

    assert!(
        logs.contains("InvalidFee"),
        "rejected for the wrong reason:\n{logs}"
    );
}

#[test]
fn extension_init_after_mint_init_fails() {
    let (mut svm, payer) = setup();
    let mint = Keypair::new();

    // Same instructions the program issues, except `InitializeMint2` runs
    // first: extensions can only be added before the mint is initialized.
    let space = ExtensionType::try_calculate_account_len::<MintState>(&[
        ExtensionType::TransferFeeConfig,
    ])
    .unwrap();
    let lamports = svm.minimum_balance_for_rent_exemption(space);

    let logs = send_expecting_failure(
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
    );

    assert!(
        logs.contains("InvalidAccountData"),
        "rejected for the wrong reason:\n{logs}"
    );
}
