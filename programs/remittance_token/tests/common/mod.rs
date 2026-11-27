// Shared LiteSVM harness for the remittance Token-2022 flows

#![allow(dead_code)]

use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, system_program},
        InstructionData, ToAccountMetas,
    },
    litesvm::{types::FailedTransactionMetadata, LiteSVM},
    remittance_token::{accounts, instruction, ID},
    solana_keypair::Keypair,
    solana_message::Message,
    solana_signer::Signer,
    solana_transaction::Transaction,
    spl_token_2022_interface::{
        extension::{
            transfer_fee::TransferFeeAmount, BaseStateWithExtensions, ExtensionType,
            StateWithExtensions,
        },
        instruction::{freeze_account, initialize_account3, mint_to, thaw_account},
        state::{Account as TokenAccount, Mint as MintState},
        ID as TOKEN_2022_PROGRAM_ID,
    },
};

pub const DECIMALS: u8 = 6;
pub const BASIS_POINTS: u16 = 50;
pub const MAXIMUM_FEE: u64 = 100_000;
pub const MINT_AMOUNT: u64 = 1_000_000;
pub const TRANSFER_AMOUNT: u64 = 10_000;
// 10_000 at 50 bps, well under the cap.
pub const EXPECTED_FEE: u64 = 50;

pub fn setup() -> (LiteSVM, Keypair) {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();

    let program_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/deploy/remittance_token.so");
    assert!(
        program_path.exists(),
        "program binary not found at {}. Run anchor build first.",
        program_path.display()
    );
    svm.add_program_from_file(ID, program_path).unwrap();

    (svm, payer)
}

/// Send instructions and require success.
pub fn send(svm: &mut LiteSVM, payer: &Keypair, ixs: &[Instruction], extra_signers: &[&Keypair]) {
    send_raw(svm, payer, ixs, extra_signers).expect("transaction failed");
}

/// Send instructions, returning the raw result for exact error matching.
#[allow(clippy::result_large_err)]
pub fn send_raw(
    svm: &mut LiteSVM,
    payer: &Keypair,
    ixs: &[Instruction],
    extra_signers: &[&Keypair],
) -> Result<(), FailedTransactionMetadata> {
    let mut signers: Vec<&Keypair> = vec![payer];
    signers.extend_from_slice(extra_signers);

    let blockhash = svm.latest_blockhash();
    let mut transaction = Transaction::new_unsigned(Message::new(ixs, Some(&payer.pubkey())));
    transaction.try_sign(&signers, blockhash).unwrap();

    svm.send_transaction(transaction).map(|_| ())
}

pub fn create_mint_ix(payer: &Pubkey, mint: &Pubkey, basis_points: u16) -> Instruction {
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

pub fn create_mint(svm: &mut LiteSVM, payer: &Keypair) -> Keypair {
    let mint = Keypair::new();
    send(
        svm,
        payer,
        &[create_mint_ix(
            &payer.pubkey(),
            &mint.pubkey(),
            BASIS_POINTS,
        )],
        &[&mint],
    );
    mint
}

/// A holder account for a fee-charging mint must reserve `TransferFeeAmount`.
/// Starts frozen; the payer (freeze authority) thaws after KYC.
pub fn token_account(svm: &mut LiteSVM, payer: &Keypair, mint: &Pubkey, owner: &Pubkey) -> Pubkey {
    let account = Keypair::new();
    let space = ExtensionType::try_calculate_account_len::<TokenAccount>(&[
        ExtensionType::TransferFeeAmount,
    ])
    .unwrap();
    let lamports = svm.minimum_balance_for_rent_exemption(space);

    send(
        svm,
        payer,
        &[
            solana_system_interface::instruction::create_account(
                &payer.pubkey(),
                &account.pubkey(),
                lamports,
                space as u64,
                &TOKEN_2022_PROGRAM_ID,
            ),
            initialize_account3(&TOKEN_2022_PROGRAM_ID, &account.pubkey(), mint, owner).unwrap(),
        ],
        &[&account],
    );
    account.pubkey()
}

/// Clear one account after KYC
pub fn thaw(svm: &mut LiteSVM, payer: &Keypair, mint: &Pubkey, account: &Pubkey) {
    send(
        svm,
        payer,
        &[thaw_account(&TOKEN_2022_PROGRAM_ID, account, mint, &payer.pubkey(), &[]).unwrap()],
        &[],
    );
}

/// Re-freeze one account
pub fn freeze(svm: &mut LiteSVM, payer: &Keypair, mint: &Pubkey, account: &Pubkey) {
    send(
        svm,
        payer,
        &[freeze_account(&TOKEN_2022_PROGRAM_ID, account, mint, &payer.pubkey(), &[]).unwrap()],
        &[],
    );
}

pub fn fund(svm: &mut LiteSVM, payer: &Keypair, mint: &Pubkey, account: &Pubkey, amount: u64) {
    send(
        svm,
        payer,
        &[mint_to(
            &TOKEN_2022_PROGRAM_ID,
            mint,
            account,
            &payer.pubkey(),
            &[],
            amount,
        )
        .unwrap()],
        &[],
    );
}

/// Thawed and funded in one call: the standard post-KYC holder.
pub fn funded_account(
    svm: &mut LiteSVM,
    payer: &Keypair,
    mint: &Pubkey,
    owner: &Keypair,
    amount: u64,
) -> Pubkey {
    let account = token_account(svm, payer, mint, &owner.pubkey());
    thaw(svm, payer, mint, &account);
    fund(svm, payer, mint, &account, amount);
    account
}

pub fn transfer_ix(
    source_owner: &Pubkey,
    source: &Pubkey,
    destination: &Pubkey,
    mint: &Pubkey,
    amount: u64,
) -> Instruction {
    Instruction {
        program_id: ID,
        accounts: accounts::Transfer {
            authority: *source_owner,
            source: *source,
            destination: *destination,
            mint: *mint,
            token_program: TOKEN_2022_PROGRAM_ID,
        }
        .to_account_metas(None),
        data: instruction::Transfer { amount }.data(),
    }
}

/// Returns `(balance, withheld_fees)`. All reads go through
/// `StateWithExtensions`, never raw unpack.
pub fn read_holding(svm: &LiteSVM, address: &Pubkey) -> (u64, u64) {
    let account = svm.get_account(address).unwrap();
    let state = StateWithExtensions::<TokenAccount>::unpack(&account.data).unwrap();
    let withheld = state.get_extension::<TransferFeeAmount>().unwrap();
    (state.base.amount, u64::from(withheld.withheld_amount))
}

/// The fee the mint schedule dictates at the genesis epoch: the same pure
/// calculation the program runs on-chain.
pub fn expected_fee(svm: &LiteSVM, mint: &Pubkey, amount: u64) -> u64 {
    use spl_token_2022_interface::extension::transfer_fee::TransferFeeConfig;

    let account = svm.get_account(mint).unwrap();
    let state = StateWithExtensions::<MintState>::unpack(&account.data).unwrap();
    state
        .get_extension::<TransferFeeConfig>()
        .unwrap()
        .calculate_epoch_fee(0, amount)
        .unwrap()
}
