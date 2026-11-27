use anchor_lang::{prelude::*, solana_program::program::invoke};
use anchor_spl::token_interface::{spl_token_2022, TokenInterface};

#[derive(Accounts)]
pub struct DepositConfidential<'info> {
    /// CHECK: validated by Token-2022, which rejects any account that is not
    /// a token account for this mint configured for confidential transfers.
    #[account(mut, owner = token_program.key())]
    pub token_account: UncheckedAccount<'info>,

    /// CHECK: validated by Token-2022 during the deposit.
    #[account(owner = token_program.key())]
    pub mint: UncheckedAccount<'info>,

    pub authority: Signer<'info>,
    pub token_program: Interface<'info, TokenInterface>,
}

impl<'info> DepositConfidential<'info> {
    /// Move tokens from the public balance into the pending confidential
    /// balance. The only confidential step a program can drive on its own:
    /// no zero-knowledge proof is needed because the amount was public.
    pub fn deposit_confidential(&self, amount: u64, decimals: u8) -> Result<()> {
        let ix = spl_token_2022::extension::confidential_transfer::instruction::deposit(
            &self.token_program.key(),
            &self.token_account.key(),
            &self.mint.key(),
            amount,
            decimals,
            &self.authority.key(),
            &[],
        )?;
        invoke(
            &ix,
            &[
                self.token_account.to_account_info(),
                self.mint.to_account_info(),
                self.authority.to_account_info(),
                self.token_program.to_account_info(),
            ],
        )
        .map_err(Into::into)
    }
}
