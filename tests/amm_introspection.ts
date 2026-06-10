import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { 
  createMint, 
  createAccount, 
  mintTo, 
  getOrCreateAssociatedTokenAccount,
  createBurnInstruction,
  TOKEN_PROGRAM_ID
} from "@solana/spl-token";
import { assert } from "chai";

describe("amm_introspection", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.AmmIntrospection as Program<any>;
  const wallet = provider.wallet as anchor.Wallet;

  let mintA: anchor.web3.PublicKey;
  let mintB: anchor.web3.PublicKey;
  let userAtaA: anchor.web3.PublicKey;
  let userAtaB: anchor.web3.PublicKey;
  let vaultB: anchor.web3.Keypair;
  
  let poolPda: anchor.web3.PublicKey;

  before(async () => {
    // Airdrop some SOL just in case
    const sig = await provider.connection.requestAirdrop(wallet.publicKey, 10 * anchor.web3.LAMPORTS_PER_SOL);
    await provider.connection.confirmTransaction(sig);

    // Create Mints
    mintA = await createMint(provider.connection, wallet.payer, wallet.publicKey, null, 6);
    mintB = await createMint(provider.connection, wallet.payer, wallet.publicKey, null, 6);

    // Create Pool PDA
    [poolPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("pool")],
      program.programId
    );

    // Create ATAs for User
    userAtaA = (await getOrCreateAssociatedTokenAccount(provider.connection, wallet.payer, mintA, wallet.publicKey)).address;
    userAtaB = (await getOrCreateAssociatedTokenAccount(provider.connection, wallet.payer, mintB, wallet.publicKey)).address;

    // Create Vault for Pool (Mint B)
    vaultB = anchor.web3.Keypair.generate();

    // Mint some A to user to burn later
    await mintTo(provider.connection, wallet.payer, mintA, userAtaA, wallet.payer, 1000_000000);
  });

  it("Initializes the AMM Pool", async () => {
    await program.methods.initializePool()
      .accounts({
        admin: wallet.publicKey,
        pool: poolPda,
        mintA: mintA,
        mintB: mintB,
        vaultB: vaultB.publicKey,
        systemProgram: anchor.web3.SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([vaultB])
      .rpc();

    // Mint some B into the pool's vault so it can pay users
    await mintTo(provider.connection, wallet.payer, mintB, vaultB.publicKey, wallet.payer, 10000_000000);
    
    console.log("Pool initialized successfully");
  });

  it("Executes Swap Payout WITH valid Introspection (Burn + Swap)", async () => {
    const amountToSwap = 5_000000; // 5 tokens

    // 1. Create the Burn Instruction
    const burnIx = createBurnInstruction(
      userAtaA,
      mintA,
      wallet.publicKey,
      amountToSwap,
      [],
      TOKEN_PROGRAM_ID
    );

    // 2. Create the Swap Payout Instruction
    const swapIx = await program.methods.swapPayout()
      .accounts({
        user: wallet.publicKey,
        pool: poolPda,
        mintA: mintA,
        vaultB: vaultB.publicKey,
        userAtaB: userAtaB,
        instructions: anchor.web3.SYSVAR_INSTRUCTIONS_PUBKEY,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .instruction();

    // 3. Put them BOTH in the same transaction
    const tx = new anchor.web3.Transaction().add(burnIx).add(swapIx);

    // 4. Send transaction
    const txSig = await provider.sendAndConfirm(tx);
    console.log("Success! Transaction Hash:", txSig);

    // 5. Verify User received the B tokens
    const balanceB = await provider.connection.getTokenAccountBalance(userAtaB);
    assert.equal(balanceB.value.amount, amountToSwap.toString(), "User did not receive correct payout!");
  });

  it("Fails when trying to Swap WITHOUT burning first", async () => {
    try {
      const tx = await program.methods.swapPayout()
        .accounts({
          user: wallet.publicKey,
          pool: poolPda,
          mintA: mintA,
          vaultB: vaultB.publicKey,
          userAtaB: userAtaB,
          instructions: anchor.web3.SYSVAR_INSTRUCTIONS_PUBKEY,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .rpc();
      
      assert.fail("Should have failed because there was no prior instruction!");
    } catch (e: any) {
      assert.include(e.message, "No prior instruction found in transaction");
    }
  });

});
