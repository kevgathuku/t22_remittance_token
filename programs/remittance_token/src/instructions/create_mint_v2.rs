use anchor_lang::{prelude::*, solana_program::program::invoke};
use anchor_spl::token_interface::{
    default_account_state_initialize, initialize_mint2, metadata_pointer_initialize,
    mint_close_authority_initialize, permanent_delegate_initialize, spl_token_2022,
    transfer_fee_initialize, DefaultAccountStateInitialize, InitializeMint2,
    MetadataPointerInitialize, MintCloseAuthorityInitialize, PermanentDelegateInitialize,
    TokenInterface, TransferFeeInitialize,
};
use spl_token_2022::{
    extension::{
        confidential_transfer::instruction as confidential_instruction,
        confidential_transfer_fee::instruction as confidential_fee_instruction, ExtensionType,
    },
    state::{AccountState, Mint as MintState},
};

/// The v2 extension set: v1's four plus seizure, confidentiality, and the
/// fee bridge Token-2022 demands for the fee+confidential pair.
const EXTENSIONS: [ExtensionType; 7] = [
    ExtensionType::MetadataPointer,
    ExtensionType::MintCloseAuthority,
    ExtensionType::TransferFeeConfig,
    ExtensionType::DefaultAccountState,
    ExtensionType::PermanentDelegate,
    ExtensionType::ConfidentialTransferMint,
    ExtensionType::ConfidentialTransferFeeConfig,
];

#[derive(Accounts)]
pub struct CreateMintV2<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    /// Unchecked: the account does not exist yet. Created and initialized
    /// in the handler.
    ///
    /// CHECK: signer as it is made at its own address.
    #[account(mut, signer)]
    pub mint: UncheckedAccount<'info>,

    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> CreateMintV2<'info> {
    /// Re-issue the mint with seizure and confidentiality.
    ///
    /// Neither extension can be added to a live mint, so v1's four are carried
    /// forward. Manual approval (`auto_approve_new_accounts: false`, no auditor
    /// key): no backdoor decryption key over transfer amounts.
    ///
    /// Pairing `TransferFeeConfig` with `ConfidentialTransferMint` requires
    /// `ConfidentialTransferFeeConfig`, else Token-2022 rejects the mint.
    pub fn create_mint_v2(
        &mut self,
        decimals: u8,
        basis_points: u16,
        maximum_fee: u64,
        withdraw_withheld_authority_elgamal_pubkey: [u8; 32],
    ) -> Result<()> {
        let extensions = EXTENSIONS;

        let space = ExtensionType::try_calculate_account_len::<MintState>(&extensions)?;
        let lamports = Rent::get()?.minimum_balance(space);

        anchor_lang::system_program::create_account(
            CpiContext::new(
                self.system_program.key(),
                anchor_lang::system_program::CreateAccount {
                    from: self.payer.to_account_info(),
                    to: self.mint.to_account_info(),
                },
            ),
            lamports,
            space as u64,
            &self.token_program.key(),
        )?;

        metadata_pointer_initialize(
            CpiContext::new(
                self.token_program.key(),
                MetadataPointerInitialize {
                    token_program_id: self.token_program.to_account_info(),
                    mint: self.mint.to_account_info(),
                },
            ),
            Some(self.payer.key()),
            Some(self.mint.key()),
        )?;

        mint_close_authority_initialize(
            CpiContext::new(
                self.token_program.key(),
                MintCloseAuthorityInitialize {
                    token_program_id: self.token_program.to_account_info(),
                    mint: self.mint.to_account_info(),
                },
            ),
            Some(&self.payer.key()),
        )?;

        transfer_fee_initialize(
            CpiContext::new(
                self.token_program.key(),
                TransferFeeInitialize {
                    token_program_id: self.token_program.to_account_info(),
                    mint: self.mint.to_account_info(),
                },
            ),
            Some(&self.payer.key()),
            Some(&self.payer.key()),
            basis_points,
            maximum_fee,
        )?;

        default_account_state_initialize(
            CpiContext::new(
                self.token_program.key(),
                DefaultAccountStateInitialize {
                    token_program_id: self.token_program.to_account_info(),
                    mint: self.mint.to_account_info(),
                },
            ),
            &AccountState::Frozen,
        )?;

        permanent_delegate_initialize(
            CpiContext::new(
                self.token_program.key(),
                PermanentDelegateInitialize {
                    token_program_id: self.token_program.to_account_info(),
                    mint: self.mint.to_account_info(),
                },
            ),
            &self.payer.key(),
        )?;

        // Manual approval; no auditor key.
        let mint_info = self.mint.to_account_info();
        let program_info = self.token_program.to_account_info();
        invoke(
            &confidential_instruction::initialize_mint(
                &self.token_program.key(),
                &self.mint.key(),
                Some(self.payer.key()),
                false,
                None,
            )?,
            &[mint_info.clone(), program_info.clone()],
        )?;

        // Required bridge for the fee+confidential pair; after `ConfidentialTransferMint`.
        invoke(
            &confidential_fee_instruction::initialize_confidential_transfer_fee_config(
                &self.token_program.key(),
                &self.mint.key(),
                Some(self.payer.key()),
                &withdraw_withheld_authority_elgamal_pubkey.into(),
            )?,
            &[mint_info, program_info],
        )?;

        initialize_mint2(
            CpiContext::new(
                self.token_program.key(),
                InitializeMint2 {
                    mint: self.mint.to_account_info(),
                },
            ),
            decimals,
            &self.payer.key(),
            Some(&self.payer.key()),
        )?;

        msg!("mint v2 {} created with {} bytes", self.mint.key(), space);
        Ok(())
    }
}
