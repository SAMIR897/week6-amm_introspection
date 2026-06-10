use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};
use solana_program::sysvar::instructions::{load_current_index_checked, load_instruction_at_checked};

declare_id!("4KFwmL8svYJRz6jiP5hiDXPt1HyvZvgJfEstkq2Tofqs");

#[program]
pub mod amm_introspection {
    use super::*;

    pub fn initialize_pool(ctx: Context<InitializePool>) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        pool.bump = ctx.bumps.pool;
        pool.mint_a = ctx.accounts.mint_a.key();
        pool.mint_b = ctx.accounts.mint_b.key();
        Ok(())
    }

    pub fn swap_payout(ctx: Context<SwapPayout>) -> Result<()> {
        // 1. Get the Instructions Sysvar
        let ixs_sysvar = &ctx.accounts.instructions;

        // 2. Load the current instruction index
        let current_index = load_current_index_checked(&ixs_sysvar.to_account_info())?;
        require!(current_index >= 1, IntrospectionError::NoPriorInstruction);

        // 3. Load the previous instruction
        let prev_ix = load_instruction_at_checked((current_index - 1) as usize, &ixs_sysvar.to_account_info())?;

        // 4. Verify the previous instruction was executed by the SPL Token Program
        require_keys_eq!(prev_ix.program_id, token::ID, IntrospectionError::InvalidProgram);

        // 5. Verify the instruction data is a SPL Token `Burn` instruction
        // The `Burn` instruction in spl_token has a 1-byte discriminator of 8, followed by an 8-byte u64 amount
        require!(prev_ix.data.len() == 9, IntrospectionError::InvalidInstructionData);
        require!(prev_ix.data[0] == 8, IntrospectionError::NotBurnInstruction);

        // 6. Decode the amount burned
        let amount_bytes: [u8; 8] = prev_ix.data[1..9].try_into().unwrap();
        let amount_burned = u64::from_le_bytes(amount_bytes);
        require!(amount_burned > 0, IntrospectionError::InvalidBurnAmount);

        // 7. Verify the correct mint was burned (Account index 1 in spl_token Burn is the mint)
        require_keys_eq!(prev_ix.accounts[1].pubkey, ctx.accounts.mint_a.key(), IntrospectionError::InvalidMintBurned);

        // 8. Verify the user was the authority for the burn (Account index 2)
        require_keys_eq!(prev_ix.accounts[2].pubkey, ctx.accounts.user.key(), IntrospectionError::InvalidBurnAuthority);

        // 9. Now that we verified the user safely burned Token A in the previous instruction, 
        // we can payout the exact equivalent amount of Token B from our vault. (1:1 ratio for simplicity)
        let bump = ctx.accounts.pool.bump;
        let seeds = &["pool".as_bytes(), &[bump]];
        let signer_seeds = &[&seeds[..]];

        let cpi_accounts = Transfer {
            from: ctx.accounts.vault_b.to_account_info(),
            to: ctx.accounts.user_ata_b.to_account_info(),
            authority: ctx.accounts.pool.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.key();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer_seeds);

        token::transfer(cpi_ctx, amount_burned)?;

        Ok(())
    }
}

#[derive(Accounts)]
pub struct InitializePool<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        init,
        payer = admin,
        space = 8 + 1 + 32 + 32,
        seeds = [b"pool"],
        bump
    )]
    pub pool: Account<'info, Pool>,

    pub mint_a: Account<'info, Mint>,
    
    pub mint_b: Account<'info, Mint>,

    #[account(
        init,
        payer = admin,
        token::mint = mint_b,
        token::authority = pool,
    )]
    pub vault_b: Account<'info, TokenAccount>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct SwapPayout<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        seeds = [b"pool"],
        bump = pool.bump,
    )]
    pub pool: Account<'info, Pool>,

    pub mint_a: Account<'info, Mint>,

    #[account(mut)]
    pub vault_b: Account<'info, TokenAccount>,

    #[account(mut)]
    pub user_ata_b: Account<'info, TokenAccount>,

    /// CHECK: Instructions sysvar checked by framework
    #[account(address = solana_program::sysvar::instructions::ID)]
    pub instructions: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token>,
}

#[account]
pub struct Pool {
    pub bump: u8,
    pub mint_a: Pubkey,
    pub mint_b: Pubkey,
}

#[error_code]
pub enum IntrospectionError {
    #[msg("No prior instruction found in transaction")]
    NoPriorInstruction,
    #[msg("Prior instruction was not executed by the Token Program")]
    InvalidProgram,
    #[msg("Prior instruction has invalid data length")]
    InvalidInstructionData,
    #[msg("Prior instruction was not a Burn instruction")]
    NotBurnInstruction,
    #[msg("Burned an invalid mint")]
    InvalidMintBurned,
    #[msg("Burn authority does not match the user")]
    InvalidBurnAuthority,
    #[msg("Amount burned must be greater than 0")]
    InvalidBurnAmount,
}
