
## Project Structure

The codebase is organized into several modules:

- **engine/** - Core trading logic including copy trading, selling strategies, and transaction parsing
- **dex/** - Protocol-specific implementations for PumpFun and PumpSwap
- **services/** - External services integration including Telegram notifications
- **common/** - Shared utilities, configuration, and constants
- **core/** - Core system functionality
- **error/** - Error handling and definitions

## Setup

### Environment Variables

To run this bot, you will need to configure the following environment variables:

#### Required Variables

- `GRPC_ENDPOINT` - Your Yellowstone gRPC endpoint URL
- `GRPC_X_TOKEN` - Your Yellowstone authentication token
- `` - Wallet address(es) to monitor for trades (comma-separated for multiple addresses)

#### Telegram Notifications

To enable Telegram notifications:

- `TELEGRAM_BOT_TOKEN` - Your Telegram bot token
- `TELEGRAM_CHAT_ID` - Your chat ID for receiving notifications

#### Optional Variables

- `PROTOCOL_PREFERENCE` - Preferred protocol to use (`pumpfun`, `pumpswap`, or `auto` for automatic detection)
- `COUNTER_LIMIT` - Maximum number of trades to execute

## New Token Tracking System

The bot now includes a comprehensive token tracking system that:

1. **Tracks Bought Tokens**: When the bot successfully buys a token, it's added to a tracking system
2. **Prevents Invalid Sells**: The bot will only attempt to sell tokens it actually owns
3. **Monitors Balances**: A background service checks token balances every 30 seconds
4. **Auto-Cleanup**: Tokens with zero or very low balances are automatically removed from tracking

### Commands

- `--check-tokens`: Display current token tracking status
- `--wrap`: Wrap SOL to WSOL
- `--unwrap`: Unwrap WSOL to SOL
- `--close`: Close all token accounts

## Usage

```bash
# Build the project
cargo build --release

# Run the bot
cargo run --release
```

Once started, the bot will:

1. Connect to the Yellowstone gRPC endpoint
2. Monitor transactions from the specified wallet address(es)
3. Automatically copy buy and sell transactions as they occur
4. Send notifications via Telegram for detected transactions and executed trades

## Recent Updates

- Added PumpSwap notification mode (can monitor without executing trades)
- Implemented concurrent transaction processing using tokio tasks
- Enhanced error handling and reporting
- Improved selling strategy implementation

## Contact

For questions or support, please contact the developer.
