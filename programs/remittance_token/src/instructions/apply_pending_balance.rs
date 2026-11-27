use anchor_lang::{prelude::*, solana_program::program::invoke};
use anchor_spl::token_interface::{spl_token_2022, TokenInterface};
use spl_token_2022::extension::confidential_transfer::DecryptableBalance;

use crate::AE_CIPHERTEXT_LEN;

#[derive(Accounts)]
pub struct ApplyPendingBalance<'info> {
    /// CHECK: validated by Token-2022.
    #[account(mut, owner = token_program.key())]
    pub token_account: UncheckedAccount<'info>,

    pub authority: Signer<'info>,
    pub token_program: Interface<'info, TokenInterface>,
}

impl<'info> ApplyPendingBalance<'info> {
    /// Move pending into available. The program cannot compute the new
    /// available balance — it has no access to the owner's AES key — so the
    /// ciphertext arrives as an argument and the program passes it through.
    pub fn apply_pending_balance(
        &self,
        expected_pending_balance_credit_counter: u64,
        new_decryptable_available_balance: [u8; AE_CIPHERTEXT_LEN],
    ) -> Result<()> {
        let balance = DecryptableBalance::from(new_decryptable_available_balance);
        let ix =
            spl_token_2022::extension::confidential_transfer::instruction::apply_pending_balance(
                &self.token_program.key(),
                &self.token_account.key(),
                expected_pending_balance_credit_counter,
                &balance,
                &self.authority.key(),
                &[],
            )?;
        invoke(
            &ix,
            &[
                self.token_account.to_account_info(),
                self.authority.to_account_info(),
                self.token_program.to_account_info(),
            ],
        )
        .map_err(Into::into)
    }
}
