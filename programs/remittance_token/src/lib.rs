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
        ctx.accounts.create_mint(decimals, basis_points, maximum_fee)
    }
}
