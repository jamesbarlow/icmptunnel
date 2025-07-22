use anchor_client::solana_sdk::signature::Signer;
use solana_vntr_sniper::{
    common::{config::Config, constants::RUN_MSG, cache::WALLET_TOKEN_ACCOUNTS},
    engine::{
        market_maker::{start_market_maker, MarketMakerConfig},
    },
    services::{telegram, cache_maintenance, blockhash_processor::BlockhashProcessor},
    core::token,
};
use solana_program_pack::Pack;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::transaction::Transaction;
use anchor_client::solana_sdk::system_instruction;
use anchor_client::solana_sdk::signature::Keypair;
use std::str::FromStr;
use colored::Colorize;
use spl_token::instruction::sync_native;
use spl_token::ui_amount_to_amount;
use spl_associated_token_account::get_associated_token_address;
use std::sync::Arc;
use std::fs;
use std::path::Path;

/// Generate wallets and save them to ./wallet directory
async fn generate_wallets() -> Result<(), String> {
    let logger = solana_vntr_sniper::common::logger::Logger::new("[WALLET-GEN] => ".green().to_string());
    
    // Read WALLET_COUNT from environment
    let wallet_count = std::env::var("WALLET_COUNT")
        .unwrap_or_else(|_| "5".to_string())
        .parse::<usize>()
        .map_err(|e| format!("Invalid WALLET_COUNT: {}", e))?;
    
    logger.log(format!("Generating {} wallets...", wallet_count));
    
    // Create wallet directory if it doesn't exist
    let wallet_dir = "./wallet";
    if !Path::new(wallet_dir).exists() {
        fs::create_dir_all(wallet_dir)
            .map_err(|e| format!("Failed to create wallet directory: {}", e))?;
        logger.log(format!("Created wallet directory: {}", wallet_dir));
    }
    
    // Generate wallets
    for i in 1..=wallet_count {
        // Generate new keypair
        let keypair = Keypair::new();
        let pubkey = keypair.pubkey();
        let private_key_encoded = keypair.to_base58_string();
        
        // Create filename with format: [wallet_number]_[wallet_pubkey].txt
        let filename = format!("{}_{}.txt", i, pubkey);
        let filepath = format!("{}/{}", wallet_dir, filename);
        
        // Save encoded private key to file
        fs::write(&filepath, &private_key_encoded)
            .map_err(|e| format!("Failed to write wallet file {}: {}", filepath, e))?;
        
        logger.log(format!("Generated wallet {}: {} -> {}", i, pubkey, filename));
    }
    
    logger.log(format!("Successfully generated {} wallets in {}/", wallet_count, wallet_dir));
    Ok(())
}

/// Initialize the wallet token account list by fetching all token accounts owned by the wallet
async fn initialize_token_account_list(config: &Config) {
    let logger = solana_vntr_sniper::common::logger::Logger::new("[INIT-TOKEN-ACCOUNTS] => ".green().to_string());
    
    if let Ok(wallet_pubkey) = config.app_state.wallet.try_pubkey() {
        logger.log(format!("Initializing token account list for wallet: {}", wallet_pubkey));
        
        // Get the token program pubkey
        let token_program = Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap();
        
        // Query all token accounts owned by the wallet
        let accounts = config.app_state.rpc_client.get_token_accounts_by_owner(
            &wallet_pubkey,
            anchor_client::solana_client::rpc_request::TokenAccountsFilter::ProgramId(token_program)
        );
        match accounts {
            Ok(accounts) => {
                logger.log(format!("Found {} token accounts", accounts.len()));
                
                // Add each token account to our global cache
                for account in accounts {
                    WALLET_TOKEN_ACCOUNTS.insert(Pubkey::from_str(&account.pubkey).unwrap());
                    logger.log(format!("Added token account: {}", account.pubkey ));
                }
                
                logger.log(format!("Token account list initialized with {} accounts", WALLET_TOKEN_ACCOUNTS.size()));
            },
            Err(e) => {
                logger.log(format!("Error fetching token accounts: {}", e));
            }
        }
    } else {
        logger.log("Failed to get wallet pubkey, can't initialize token account list".to_string());
    }
}

/// Wrap SOL to Wrapped SOL (WSOL)
async fn wrap_sol(config: &Config, amount: f64) -> Result<(), String> {
    let logger = solana_vntr_sniper::common::logger::Logger::new("[WRAP-SOL] => ".green().to_string());
    
    // Get wallet pubkey
    let wallet_pubkey = match config.app_state.wallet.try_pubkey() {
        Ok(pk) => pk,
        Err(_) => return Err("Failed to get wallet pubkey".to_string()),
    };
    
    // Create WSOL account instructions
    let (wsol_account, mut instructions) = match token::create_wsol_account(wallet_pubkey) {
        Ok(result) => result,
        Err(e) => return Err(format!("Failed to create WSOL account: {}", e)),
    };
    
    logger.log(format!("WSOL account address: {}", wsol_account));
    
    // Convert UI amount to lamports (1 SOL = 10^9 lamports)
    let lamports = ui_amount_to_amount(amount, 9);
    logger.log(format!("Wrapping {} SOL ({} lamports)", amount, lamports));
    
    // Transfer SOL to the WSOL account
    instructions.push(
        system_instruction::transfer(
            &wallet_pubkey,
            &wsol_account,
            lamports,
        )
    );
    
    // Sync native instruction to update the token balance
    instructions.push(
        sync_native(
            &spl_token::id(),
            &wsol_account,
        ).map_err(|e| format!("Failed to create sync native instruction: {}", e))?
    );
    
    // Send transaction
    let recent_blockhash = config.app_state.rpc_client.get_latest_blockhash()
        .map_err(|e| format!("Failed to get recent blockhash: {}", e))?;
    
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&wallet_pubkey),
        &[&config.app_state.wallet],
        recent_blockhash,
    );
    
    match config.app_state.rpc_client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            logger.log(format!("SOL wrapped successfully, signature: {}", signature));
            Ok(())
        },
        Err(e) => {
            Err(format!("Failed to wrap SOL: {}", e))
        }
    }
}

/// Unwrap SOL from Wrapped SOL (WSOL) account
async fn unwrap_sol(config: &Config) -> Result<(), String> {
    let logger = solana_vntr_sniper::common::logger::Logger::new("[UNWRAP-SOL] => ".green().to_string());
    
    // Get wallet pubkey
    let wallet_pubkey = match config.app_state.wallet.try_pubkey() {
        Ok(pk) => pk,
        Err(_) => return Err("Failed to get wallet pubkey".to_string()),
    };
    
    // Get the WSOL ATA address
    let wsol_account = get_associated_token_address(
        &wallet_pubkey,
        &spl_token::native_mint::id()
    );
    
    logger.log(format!("WSOL account address: {}", wsol_account));
    
    // Check if WSOL account exists
    match config.app_state.rpc_client.get_account(&wsol_account) {
        Ok(_) => {
            logger.log(format!("Found WSOL account: {}", wsol_account));
        },
        Err(_) => {
            return Err(format!("WSOL account does not exist: {}", wsol_account));
        }
    }
    
    // Close the WSOL account to recover SOL
    let close_instruction = token::close_account(
        wallet_pubkey,
        wsol_account,
        wallet_pubkey,
        wallet_pubkey,
        &[&wallet_pubkey],
    ).map_err(|e| format!("Failed to create close account instruction: {}", e))?;
    
    // Send transaction
    let recent_blockhash = config.app_state.rpc_client.get_latest_blockhash()
        .map_err(|e| format!("Failed to get recent blockhash: {}", e))?;
    
    let transaction = Transaction::new_signed_with_payer(
        &[close_instruction],
        Some(&wallet_pubkey),
        &[&config.app_state.wallet],
        recent_blockhash,
    );
    
    match config.app_state.rpc_client.send_and_confirm_transaction(&transaction) {
        Ok(signature) => {
            logger.log(format!("WSOL unwrapped successfully, signature: {}", signature));
            Ok(())
        },
        Err(e) => {
            Err(format!("Failed to unwrap WSOL: {}", e))
        }
    }
}

/// Close all token accounts owned by the wallet
async fn close_all_token_accounts(config: &Config) -> Result<(), String> {
    let logger = solana_vntr_sniper::common::logger::Logger::new("[CLOSE-TOKEN-ACCOUNTS] => ".green().to_string());
    
    // Get wallet pubkey
    let wallet_pubkey = match config.app_state.wallet.try_pubkey() {
        Ok(pk) => pk,
        Err(_) => return Err("Failed to get wallet pubkey".to_string()),
    };
    
    // Get the token program pubkey
    let token_program = Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap();
    
    // Query all token accounts owned by the wallet
    let accounts = config.app_state.rpc_client.get_token_accounts_by_owner(
        &wallet_pubkey,
        anchor_client::solana_client::rpc_request::TokenAccountsFilter::ProgramId(token_program)
    ).map_err(|e| format!("Failed to get token accounts: {}", e))?;
    
    if accounts.is_empty() {
        logger.log("No token accounts found to close".to_string());
        return Ok(());
    }
    
    logger.log(format!("Found {} token accounts to close", accounts.len()));
    
    let mut closed_count = 0;
    let mut failed_count = 0;
    
    // Close each token account
    for account_info in accounts {
        let token_account = Pubkey::from_str(&account_info.pubkey)
            .map_err(|_| format!("Invalid token account pubkey: {}", account_info.pubkey))?;
        
        // Skip WSOL accounts with non-zero balance (these need to be unwrapped first)
        let account_data = match config.app_state.rpc_client.get_account(&token_account) {
            Ok(data) => data,
            Err(e) => {
                logger.log(format!("Failed to get account data for {}: {}", token_account, e).red().to_string());
                failed_count += 1;
                continue;
            }
        };
        
        // Check if this is a WSOL account with balance
        if let Ok(token_data) = spl_token::state::Account::unpack(&account_data.data) {
            if token_data.mint == spl_token::native_mint::id() && token_data.amount > 0 {
                logger.log(format!("Skipping WSOL account with non-zero balance: {} ({})", 
                                 token_account, 
                                 token_data.amount as f64 / 1_000_000_000.0));
                continue;
            }
        }
        
        // Create close instruction
        let close_instruction = token::close_account(
            wallet_pubkey,
            token_account,
            wallet_pubkey,
            wallet_pubkey,
            &[&wallet_pubkey],
        ).map_err(|e| format!("Failed to create close instruction for {}: {}", token_account, e))?;
        
        // Send transaction
        let recent_blockhash = config.app_state.rpc_client.get_latest_blockhash()
            .map_err(|e| format!("Failed to get recent blockhash: {}", e))?;
        
        let transaction = Transaction::new_signed_with_payer(
            &[close_instruction],
            Some(&wallet_pubkey),
            &[&config.app_state.wallet],
            recent_blockhash,
        );
        
        match config.app_state.rpc_client.send_and_confirm_transaction(&transaction) {
            Ok(signature) => {
                logger.log(format!("Closed token account {}, signature: {}", token_account, signature));
                closed_count += 1;
            },
            Err(e) => {
                logger.log(format!("Failed to close token account {}: {}", token_account, e).red().to_string());
                failed_count += 1;
            }
        }
    }
    
    logger.log(format!("Closed {} token accounts, {} failed", closed_count, failed_count));
    
    if failed_count > 0 {
        Err(format!("Failed to close {} token accounts", failed_count))
    } else {
        Ok(())
    }
}

/// Initialize target wallet token list by fetching all token accounts owned by the target wallet
async fn initialize_target_wallet_token_list(config: &Config, target_addresses: &[String]) -> Result<(), String> {
    let logger = solana_vntr_sniper::common::logger::Logger::new("[INIT-TARGET-TOKENS] => ".green().to_string());
    
    // Check if we should initialize
    let should_check = std::env::var("IS_CHECK_TARGET_WALLET_TOKEN_ACCOUNT")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(false);
        
    if !should_check {
        logger.log("Skipping target wallet token check as IS_CHECK_TARGET_WALLET_TOKEN_ACCOUNT is not true".to_string());
        return Ok(());
    }
    
    // Get the token program pubkey
    let token_program = Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap();
    
    for target_address in target_addresses {
        // Parse target wallet address
        let target_pubkey = match Pubkey::from_str(target_address) {
            Ok(pk) => pk,
            Err(e) => {
                logger.log(format!("Invalid target address {}: {}", target_address, e).red().to_string());
                continue;
            }
        };
        
        // Query all token accounts owned by the target wallet
        match config.app_state.rpc_client.get_token_accounts_by_owner(
            &target_pubkey,
            anchor_client::solana_client::rpc_request::TokenAccountsFilter::ProgramId(token_program)
        ) {
            Ok(accounts) => {
                logger.log(format!("Found {} token accounts for target {}", accounts.len(), target_address));
                
                // Add each token's mint to our global cache
                for account in accounts {
                    if let Ok(token_account) = config.app_state.rpc_client.get_account(&Pubkey::from_str(&account.pubkey).unwrap()) {
                        if let Ok(parsed) = spl_token::state::Account::unpack(&token_account.data) {
                            solana_vntr_sniper::common::cache::TARGET_WALLET_TOKENS.insert(parsed.mint.to_string());
                            logger.log(format!("Added token mint {} to target wallet list", parsed.mint));
                        }
                    }
                }
            },
            Err(e) => {
                logger.log(format!("Error fetching token accounts for target {}: {}", target_address, e).red().to_string());
            }
        }
    }
    
    logger.log(format!(
        "Target wallet token list initialized with {} tokens",
        solana_vntr_sniper::common::cache::TARGET_WALLET_TOKENS.size()
    ));
    
    Ok(())
}

/// Load all wallets from the ./wallet directory
fn load_all_wallets() -> Result<Vec<Arc<Keypair>>, String> {
    let wallet_dir = "./wallet";
    if !std::path::Path::new(wallet_dir).exists() {
        return Err("Wallet directory not found. Please run with --wallet first to generate wallets.".to_string());
    }
    
    let entries = std::fs::read_dir(wallet_dir)
        .map_err(|e| format!("Failed to read wallet directory: {}", e))?;
    
    let mut wallets = Vec::new();
    
    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
        let path = entry.path();
        
        if path.is_file() && path.extension().map_or(false, |ext| ext == "txt") {
            let private_key = std::fs::read_to_string(&path)
                .map_err(|e| format!("Failed to read wallet file {:?}: {}", path, e))?
                .trim()
                .to_string();
            
            if private_key.len() < 85 {
                continue; // Skip invalid wallets
            }
            
            let keypair = Keypair::from_base58_string(&private_key);
            wallets.push(Arc::new(keypair));
        }
    }
    
    if wallets.is_empty() {
        return Err("No valid wallets found in the wallet directory".to_string());
    }
    
    Ok(wallets)
}

/// Distribute SOL from main wallet to all generated wallets and convert to WSOL
async fn distribute_sol(config: &Config) -> Result<(), String> {
    let logger = solana_vntr_sniper::common::logger::Logger::new("[DISTRIBUTE] => ".yellow().bold().to_string());
    
    // Load all wallets
    let wallets = load_all_wallets()?;
    logger.log(format!("Found {} wallets to distribute to", wallets.len()));
    
    // Get main wallet balance
    let main_wallet_pubkey = config.app_state.wallet.pubkey();
    let main_balance = config.app_state.rpc_client.get_balance(&main_wallet_pubkey)
        .map_err(|e| format!("Failed to get main wallet balance: {}", e))?;
    
    let main_balance_sol = main_balance as f64 / 1_000_000_000.0;
    logger.log(format!("Main wallet balance: {} SOL", main_balance_sol));
    
    // Calculate distribution amount (leave 0.1 SOL for fees)
    let fee_reserve = 0.1;
    let distributable_amount = main_balance_sol - fee_reserve;
    
    if distributable_amount <= 0.0 {
        return Err("Insufficient balance for distribution".to_string());
    }
    
    let amount_per_wallet = distributable_amount / wallets.len() as f64;
    logger.log(format!("Distributing {} SOL to each wallet", amount_per_wallet));
    
    // Distribute SOL to each wallet
    for (i, wallet) in wallets.iter().enumerate() {
        let wallet_pubkey = wallet.pubkey();
        let lamports_to_send = (amount_per_wallet * 1_000_000_000.0) as u64;
        
        logger.log(format!("Distributing {} SOL to wallet {}: {}", 
                          amount_per_wallet, i + 1, wallet_pubkey));
        
        // Create transfer instruction
        let transfer_instruction = system_instruction::transfer(
            &main_wallet_pubkey,
            &wallet_pubkey,
            lamports_to_send,
        );
        
        // Send transaction
        let recent_blockhash = config.app_state.rpc_client.get_latest_blockhash()
            .map_err(|e| format!("Failed to get recent blockhash: {}", e))?;
        
        let transaction = Transaction::new_signed_with_payer(
            &[transfer_instruction],
            Some(&main_wallet_pubkey),
            &[&config.app_state.wallet],
            recent_blockhash,
        );
        
        match config.app_state.rpc_client.send_and_confirm_transaction(&transaction) {
            Ok(signature) => {
                logger.log(format!("✅ Distributed {} SOL to wallet {}, signature: {}", 
                                  amount_per_wallet, i + 1, signature));
            },
            Err(e) => {
                logger.log(format!("❌ Failed to distribute to wallet {}: {}", i + 1, e).red().to_string());
                continue;
            }
        }
        
        // Wait a bit to avoid rate limiting
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
    
    // Now convert 75% of SOL to WSOL in each wallet
    logger.log("Converting 75% of SOL to WSOL in each wallet...".to_string());
    
    for (i, wallet) in wallets.iter().enumerate() {
        let wallet_pubkey = wallet.pubkey();
        let wsol_amount = amount_per_wallet * 0.75; // 75% to WSOL, 25% kept for fees
        
        logger.log(format!("Converting {} SOL to WSOL for wallet {}", wsol_amount, i + 1));
        
        // Create WSOL account instructions
        let (wsol_account, mut instructions) = match token::create_wsol_account(wallet_pubkey) {
            Ok(result) => result,
            Err(e) => {
                logger.log(format!("❌ Failed to create WSOL account for wallet {}: {}", i + 1, e).red().to_string());
                continue;
            }
        };
        
        // Convert to lamports
        let lamports = (wsol_amount * 1_000_000_000.0) as u64;
        
        // Transfer SOL to the WSOL account
        instructions.push(
            system_instruction::transfer(
                &wallet_pubkey,
                &wsol_account,
                lamports,
            )
        );
        
        // Sync native instruction
        instructions.push(
            sync_native(&spl_token::id(), &wsol_account)
                .map_err(|e| format!("Failed to create sync native instruction: {}", e))?
        );
        
        // Send transaction
        let recent_blockhash = config.app_state.rpc_client.get_latest_blockhash()
            .map_err(|e| format!("Failed to get recent blockhash: {}", e))?;
        
        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&wallet_pubkey),
            &[wallet],
            recent_blockhash,
        );
        
        match config.app_state.rpc_client.send_and_confirm_transaction(&transaction) {
            Ok(signature) => {
                logger.log(format!("✅ Converted {} SOL to WSOL for wallet {}, signature: {}", 
                                  wsol_amount, i + 1, signature));
            },
            Err(e) => {
                logger.log(format!("❌ Failed to convert SOL to WSOL for wallet {}: {}", i + 1, e).red().to_string());
            }
        }
        
        // Wait a bit to avoid rate limiting
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    }
    
    logger.log("Distribution and WSOL conversion completed!".green().bold().to_string());
    Ok(())
}

/// Collect all SOL from generated wallets back to main wallet
async fn collect_sol(config: &Config) -> Result<(), String> {
    let logger = solana_vntr_sniper::common::logger::Logger::new("[COLLECT] => ".red().bold().to_string());
    
    // Load all wallets
    let wallets = load_all_wallets()?;
    logger.log(format!("Found {} wallets to collect from", wallets.len()));
    
    let main_wallet_pubkey = config.app_state.wallet.pubkey();
    let mut total_collected = 0.0;
    
    // Get the token program pubkey
    let token_program = Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap();
    
    for (i, wallet) in wallets.iter().enumerate() {
        let wallet_pubkey = wallet.pubkey();
        logger.log(format!("Processing wallet {}: {}", i + 1, wallet_pubkey));
        
        // First, close all token accounts for this wallet
        match config.app_state.rpc_client.get_token_accounts_by_owner(
            &wallet_pubkey,
            anchor_client::solana_client::rpc_request::TokenAccountsFilter::ProgramId(token_program)
        ) {
            Ok(token_accounts) => {
                logger.log(format!("Found {} token accounts for wallet {}", token_accounts.len(), i + 1));
                
                for account_info in token_accounts {
                    let token_account = Pubkey::from_str(&account_info.pubkey)
                        .map_err(|_| format!("Invalid token account pubkey: {}", account_info.pubkey))?;
                    
                    // Get account data to check if it's WSOL with balance
                    if let Ok(account_data) = config.app_state.rpc_client.get_account(&token_account) {
                        if let Ok(token_data) = spl_token::state::Account::unpack(&account_data.data) {
                            // If it's WSOL with balance, unwrap it first
                            if token_data.mint == spl_token::native_mint::id() && token_data.amount > 0 {
                                logger.log(format!("Unwrapping WSOL account: {} ({})", 
                                                 token_account, 
                                                 token_data.amount as f64 / 1_000_000_000.0));
                                
                                // Close the WSOL account to recover SOL
                                let close_instruction = token::close_account(
                                    wallet_pubkey,
                                    token_account,
                                    wallet_pubkey,
                                    wallet_pubkey,
                                    &[&wallet_pubkey],
                                ).map_err(|e| format!("Failed to create close instruction: {}", e))?;
                                
                                let recent_blockhash = config.app_state.rpc_client.get_latest_blockhash()
                                    .map_err(|e| format!("Failed to get recent blockhash: {}", e))?;
                                
                                let transaction = Transaction::new_signed_with_payer(
                                    &[close_instruction],
                                    Some(&wallet_pubkey),
                                    &[wallet],
                                    recent_blockhash,
                                );
                                
                                match config.app_state.rpc_client.send_and_confirm_transaction(&transaction) {
                                    Ok(signature) => {
                                        logger.log(format!("✅ Unwrapped WSOL account {}, signature: {}", 
                                                          token_account, signature));
                                    },
                                    Err(e) => {
                                        logger.log(format!("❌ Failed to unwrap WSOL account {}: {}", 
                                                          token_account, e).red().to_string());
                                    }
                                }
                            } else {
                                // Close other token accounts (should be empty)
                                let close_instruction = token::close_account(
                                    wallet_pubkey,
                                    token_account,
                                    wallet_pubkey,
                                    wallet_pubkey,
                                    &[&wallet_pubkey],
                                ).map_err(|e| format!("Failed to create close instruction: {}", e))?;
                                
                                let recent_blockhash = config.app_state.rpc_client.get_latest_blockhash()
                                    .map_err(|e| format!("Failed to get recent blockhash: {}", e))?;
                                
                                let transaction = Transaction::new_signed_with_payer(
                                    &[close_instruction],
                                    Some(&wallet_pubkey),
                                    &[wallet],
                                    recent_blockhash,
                                );
                                
                                match config.app_state.rpc_client.send_and_confirm_transaction(&transaction) {
                                    Ok(signature) => {
                                        logger.log(format!("✅ Closed token account {}, signature: {}", 
                                                          token_account, signature));
                                    },
                                    Err(e) => {
                                        logger.log(format!("❌ Failed to close token account {}: {}", 
                                                          token_account, e).red().to_string());
                                    }
                                }
                            }
                        }
                    }
                    
                    // Wait a bit to avoid rate limiting
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
            },
            Err(e) => {
                logger.log(format!("❌ Failed to get token accounts for wallet {}: {}", i + 1, e).red().to_string());
            }
        }
        
        // Now collect all remaining SOL
        match config.app_state.rpc_client.get_balance(&wallet_pubkey) {
            Ok(balance) => {
                if balance > 5000 { // Leave minimum for rent
                    let balance_to_send = balance - 5000; // Leave 5000 lamports for rent
                    let balance_sol = balance_to_send as f64 / 1_000_000_000.0;
                    
                    logger.log(format!("Collecting {} SOL from wallet {}", balance_sol, i + 1));
                    
                    // Create transfer instruction
                    let transfer_instruction = system_instruction::transfer(
                        &wallet_pubkey,
                        &main_wallet_pubkey,
                        balance_to_send,
                    );
                    
                    // Send transaction
                    let recent_blockhash = config.app_state.rpc_client.get_latest_blockhash()
                        .map_err(|e| format!("Failed to get recent blockhash: {}", e))?;
                    
                    let transaction = Transaction::new_signed_with_payer(
                        &[transfer_instruction],
                        Some(&wallet_pubkey),
                        &[wallet],
                        recent_blockhash,
                    );
                    
                    match config.app_state.rpc_client.send_and_confirm_transaction(&transaction) {
                        Ok(signature) => {
                            logger.log(format!("✅ Collected {} SOL from wallet {}, signature: {}", 
                                              balance_sol, i + 1, signature));
                            total_collected += balance_sol;
                        },
                        Err(e) => {
                            logger.log(format!("❌ Failed to collect from wallet {}: {}", i + 1, e).red().to_string());
                        }
                    }
                } else {
                    logger.log(format!("Wallet {} has insufficient balance to collect", i + 1));
                }
            },
            Err(e) => {
                logger.log(format!("❌ Failed to get balance for wallet {}: {}", i + 1, e).red().to_string());
            }
        }
        
        // Wait a bit to avoid rate limiting
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    }
    
    logger.log(format!("Collection completed! Total collected: {} SOL", total_collected).green().bold().to_string());
    Ok(())
}

#[tokio::main]
async fn main() {
    /* Initial Settings */
    let config = Config::new().await;
    let config = config.lock().await;

    /* Running Bot */
    let run_msg = RUN_MSG;
    println!("{}", run_msg);
    
    // Initialize blockhash processor
    match BlockhashProcessor::new(config.app_state.rpc_client.clone()).await {
        Ok(processor) => {
            if let Err(e) = processor.start().await {
                eprintln!("Failed to start blockhash processor: {}", e);
                return;
            }
            println!("Blockhash processor started successfully");
        },
        Err(e) => {
            eprintln!("Failed to initialize blockhash processor: {}", e);
            return;
        }
    }

    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        // Check for wallet generation argument
        if args.contains(&"--wallet".to_string()) {
            println!("Generating wallets...");
            
            match generate_wallets().await {
                Ok(_) => {
                    println!("✅ Wallet generation completed successfully!");
                    return;
                },
                Err(e) => {
                    eprintln!("❌ Failed to generate wallets: {}", e);
                    return;
                }
            }
        }
        // Check for command line arguments
        else if args.contains(&"--wrap".to_string()) {
            println!("Wrapping SOL to WSOL...");
            
            // Get wrap amount from .env
            let wrap_amount = std::env::var("WRAP_AMOUNT")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.1);
            
            match wrap_sol(&config, wrap_amount).await {
                Ok(_) => {
                    println!("Successfully wrapped {} SOL to WSOL", wrap_amount);
                    return;
                },
                Err(e) => {
                    eprintln!("Failed to wrap SOL: {}", e);
                    return;
                }
            }
        } else if args.contains(&"--unwrap".to_string()) {
            println!("Unwrapping WSOL to SOL...");
            
            match unwrap_sol(&config).await {
                Ok(_) => {
                    println!("Successfully unwrapped WSOL to SOL");
                    return;
                },
                Err(e) => {
                    eprintln!("Failed to unwrap WSOL: {}", e);
                    return;
                }
            }
        } else if args.contains(&"--close".to_string()) {
            println!("Closing all token accounts...");
            
            match close_all_token_accounts(&config).await {
                Ok(_) => {
                    println!("Successfully closed all token accounts");
                    return;
                },
                Err(e) => {
                    eprintln!("Failed to close all token accounts: {}", e);
                    return;
                }
            }
        } else if args.contains(&"--check-tokens".to_string()) {
            println!("Token monitoring feature disabled in this version");
            return;
        } else if args.contains(&"--distribute".to_string()) {
            println!("Distributing SOL to all wallets and converting to WSOL...");
            
            match distribute_sol(&config).await {
                Ok(_) => {
                    println!("✅ SOL distribution and WSOL conversion completed successfully!");
                    return;
                },
                Err(e) => {
                    eprintln!("❌ Failed to distribute SOL: {}", e);
                    return;
                }
            }
        } else if args.contains(&"--collect".to_string()) {
            println!("Collecting all SOL from wallets back to main wallet...");
            
            match collect_sol(&config).await {
                Ok(_) => {
                    println!("✅ SOL collection completed successfully!");
                    return;
                },
                Err(e) => {
                    eprintln!("❌ Failed to collect SOL: {}", e);
                    return;
                }
            }
        }
    }

    // Initialize Telegram bot
    match telegram::init().await {
        Ok(_) => println!("Telegram bot initialized successfully"),
        Err(e) => println!("Failed to initialize Telegram bot: {}. Continuing without notifications.", e),
    }
    
    // Initialize token account list
    initialize_token_account_list(&config).await;
    
    // Start cache maintenance service (clean up expired cache entries every 60 seconds)
    cache_maintenance::start_cache_maintenance(60).await;
    println!("Cache maintenance service started");

    // Market maker mode - no need for target addresses

    // Create stealth market maker config with 100 wallets
    let market_maker_config = MarketMakerConfig::stealth_mode(
        config.yellowstone_grpc_http.clone(),
        config.yellowstone_grpc_token.clone(),
        std::sync::Arc::new(config.app_state.clone()),
        config.target_token_mint.clone(),
    );
    
    // Start the advanced stealth market maker bot
    println!("🚀 Starting Advanced Stealth Market Maker for mint: {}", config.target_token_mint);
    println!("🎯 Using 100 wallets with sophisticated randomization");
    println!("💰 Buy amount ratio: 50% - 90% of wrapped WSOL");
    println!("🎲 70% Buy / 30% Sell ratio");
    println!("🔄 Wallet rotation every 2 trades");
    println!("⏰ Randomized intervals: 10 minutes - 2 hours");
    println!("📊 Activity reports every 30 minutes");
    println!("🎯 Buy: amount_in = WSOL lamports, minimum_amount_out = 0");
    println!("🎯 Sell: amount_in = token balance, minimum_amount_out = 0");
    
    if let Err(e) = start_market_maker(market_maker_config).await {
        eprintln!("Advanced Market Maker error: {}", e);
        
        // Send error notification via Telegram
        if let Err(te) = telegram::send_error_notification(&format!("Advanced Market Maker bot crashed: {}", e)).await {
            eprintln!("Failed to send Telegram notification: {}", te);
        }
    }
}
