use anchor_lang::prelude::*;

/// Maximum protocol fee in basis points (10_000 bps = 100%).
/// `create_mint` rejects anything above this.
#[constant]
pub const MAX_FEE_BASIS_POINTS: u16 = 10_000;

/// Ciphertext length of a decryptable balance (AES-encrypted u64 + nonce).
pub const AE_CIPHERTEXT_LEN: usize = 36;
