use crate::{error::MintError, MAX_FEE_BASIS_POINTS};
use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    default_account_state_initialize, initialize_mint2, metadata_pointer_initialize,
    mint_close_authority_initialize, spl_token_2022, transfer_fee_initialize,
    DefaultAccountStateInitialize, InitializeMint2, MetadataPointerInitialize,
    MintCloseAuthorityInitialize, TokenInterface, TransferFeeInitialize,
};
use spl_token_2022::{
    extension::ExtensionType,
    state::{AccountState, Mint as MintState},
};

#[derive(Accounts)]
pub struct CreateMint<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    /// Unchecked because the account does not exist yet and no constraint can
    /// describe a transfer fee mint. Created and initialized in the handler.
    ///
    /// CHECK: created and initialized in the handler, and required to sign
    /// because the account is made at its own address.
    #[account(mut, signer)]
    pub mint: UncheckedAccount<'info>,

    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> CreateMint<'info> {
    /// Issue the remittance stablecoin mint.
    ///
    /// TransferFeeConfig is not one of Anchor's seven declarative
    /// `extensions::` constraints, so a mint that charges a transfer fee
    /// cannot be expressed as constraints at all. The fallback is the manual
    /// three-phase path: size via `try_calculate_account_len`, initialize
    /// every extension, seal with `InitializeMint2` last. Nothing can be added
    /// after that point, so a mistake here is permanent.
    pub fn create_mint(
        &mut self,
        decimals: u8,
        basis_points: u16,
        maximum_fee: u64,
    ) -> Result<()> {
        require!(
            basis_points <= MAX_FEE_BASIS_POINTS,
            MintError::InvalidFee
        );

        let extensions = [
            ExtensionType::MetadataPointer,
            ExtensionType::MintCloseAuthority,
            ExtensionType::TransferFeeConfig,
            ExtensionType::DefaultAccountState,
        ];

        // Phase 1: allocate at the full extended length.
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

        // Phase 2: initialize each extension, before the mint itself exists.
        // On-chain metadata lives on the mint itself: no off-chain registry.
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

        // The issuer can decommission the mint.
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

        // The protocol fee on every transfer; the issuer collects it.
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

        // New accounts start frozen until KYC clears them.
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

        // Phase 3: seal the mint. The payer stays on as mint, freeze, fee,
        // close and metadata authority: the issuer roles for this mint.
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

        msg!("mint {} created with {} bytes", self.mint.key(), space);
        Ok(())
    }
}
