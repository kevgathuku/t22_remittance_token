pub mod constants;
pub mod error;
pub mod instructions;

use anchor_lang::prelude::*;

pub use constants::*;
pub use error::*;
pub use instructions::*;

declare_id!("JAnBk9WV8d9xkBpzsXo1Cx63HiTfGF85YqT7KKHJmobL");

#[program]
pub mod remittance_token {
    use super::*;

    pub fn create_mint(
        ctx: Context<CreateMint>,
        decimals: u8,
        basis_points: u16,
        maximum_fee: u64,
    ) -> Result<()> {
        ctx.accounts
            .create_mint(decimals, basis_points, maximum_fee)
    }

    pub fn transfer(ctx: Context<Transfer>, amount: u64) -> Result<()> {
        ctx.accounts.transfer(amount)
    }

    pub fn deposit_confidential(
        ctx: Context<DepositConfidential>,
        amount: u64,
        decimals: u8,
    ) -> Result<()> {
        ctx.accounts.deposit_confidential(amount, decimals)
    }

    pub fn apply_pending_balance(
        ctx: Context<ApplyPendingBalance>,
        expected_pending_balance_credit_counter: u64,
        new_decryptable_available_balance: [u8; AE_CIPHERTEXT_LEN],
    ) -> Result<()> {
        ctx.accounts.apply_pending_balance(
            expected_pending_balance_credit_counter,
            new_decryptable_available_balance,
        )
    }

    pub fn thaw_account(ctx: Context<ThawAccount>) -> Result<()> {
        ctx.accounts.thaw_account()
    }

    pub fn create_mint_v2(
        ctx: Context<CreateMintV2>,
        decimals: u8,
        basis_points: u16,
        maximum_fee: u64,
        withdraw_withheld_authority_elgamal_pubkey: [u8; 32],
    ) -> Result<()> {
        ctx.accounts.create_mint_v2(
            decimals,
            basis_points,
            maximum_fee,
            withdraw_withheld_authority_elgamal_pubkey,
        )
    }
}
