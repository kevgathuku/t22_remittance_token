use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    spl_token_2022, transfer_checked_with_fee, TokenInterface, TransferCheckedWithFee,
};
use spl_token_2022::{
    extension::{transfer_fee::TransferFeeConfig, BaseStateWithExtensions, StateWithExtensions},
    state::Mint as MintState,
};

use crate::error::MintError;

#[derive(Accounts)]
pub struct Transfer<'info> {
    /// Owner of the source account. Signs every transfer.
    pub authority: Signer<'info>,

    /// CHECK: validated by Token-2022. Must be unfrozen (thawed after KYC).
    #[account(mut, owner = token_program.key())]
    pub source: UncheckedAccount<'info>,

    /// CHECK: validated by Token-2022.
    #[account(mut, owner = token_program.key())]
    pub destination: UncheckedAccount<'info>,

    /// CHECK: ownership enforced; read via `StateWithExtensions` since the
    /// typed account hides extensions.
    #[account(owner = token_program.key())]
    pub mint: UncheckedAccount<'info>,

    pub token_program: Interface<'info, TokenInterface>,
}

impl<'info> Transfer<'info> {
    /// Move tokens, withholding the protocol fee computed live via
    /// `calculate_epoch_fee` — never a caller-supplied or cached rate.
    pub fn transfer(&mut self, amount: u64) -> Result<()> {
        let (decimals, fee) = {
            let mint_info = self.mint.to_account_info();
            let data = mint_info.try_borrow_data()?;
            let mint = StateWithExtensions::<MintState>::unpack(&data)?;
            let fee = mint
                .get_extension::<TransferFeeConfig>()?
                .calculate_epoch_fee(Clock::get()?.epoch, amount)
                .ok_or(MintError::FeeOverflow)?;
            (mint.base.decimals, fee)
        };

        transfer_checked_with_fee(
            CpiContext::new(
                self.token_program.key(),
                TransferCheckedWithFee {
                    token_program_id: self.token_program.to_account_info(),
                    source: self.source.to_account_info(),
                    mint: self.mint.to_account_info(),
                    destination: self.destination.to_account_info(),
                    authority: self.authority.to_account_info(),
                },
            ),
            amount,
            decimals,
            fee,
        )
    }
}
