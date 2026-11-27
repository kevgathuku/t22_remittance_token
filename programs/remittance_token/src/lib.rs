use anchor_lang::prelude::*;

declare_id!("JAnBk9WV8d9xkBpzsXo1Cx63HiTfGF85YqT7KKHJmobL");

#[program]
pub mod remittance_token {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        msg!("Greetings from: {:?}", ctx.program_id);
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize {}
