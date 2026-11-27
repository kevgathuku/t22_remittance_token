use anchor_lang::{prelude::*, solana_program::program::invoke};
use anchor_spl::token_interface::{spl_token_2022, TokenInterface};

#[derive(Accounts)]
pub struct ThawAccount<'info> {
    /// The mint's freeze authority. Signs every thaw.
    pub freeze_authority: Signer<'info>,

    /// CHECK: validated by Token-2022, which checks the freeze authority
    /// against the mint. Thawed here after off-chain KYC clears.
    #[account(mut, owner = token_program.key())]
    pub token_account: UncheckedAccount<'info>,

    /// CHECK: validated by Token-2022 during the thaw.
    #[account(owner = token_program.key())]
    pub mint: UncheckedAccount<'info>,

    pub token_program: Interface<'info, TokenInterface>,
}

impl<'info> ThawAccount<'info> {
    /// Unfreeze one account. Separate from the mint-level default; each
    /// holder clears individually after KYC.
    pub fn thaw_account(&self) -> Result<()> {
        let ix = spl_token_2022::instruction::thaw_account(
            &self.token_program.key(),
            &self.token_account.key(),
            &self.mint.key(),
            &self.freeze_authority.key(),
            &[],
        )?;
        invoke(
            &ix,
            &[
                self.token_account.to_account_info(),
                self.mint.to_account_info(),
                self.freeze_authority.to_account_info(),
            ],
        )
        .map_err(Into::into)
    }
}
