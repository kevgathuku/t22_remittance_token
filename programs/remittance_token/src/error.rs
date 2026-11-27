use anchor_lang::prelude::*;

#[error_code]
pub enum MintError {
    #[msg("fee can only be between 0 and 10,000 basis points")]
    InvalidFee,
}
