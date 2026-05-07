
"""
Bitcoin test chain configuration
"""

# Bitcoin RPC configuration
BITCOIN_RPC_HOST = "127.0.0.1"
BITCOIN_RPC_PORT = 18443
BITCOIN_RPC_USER = "user"
BITCOIN_RPC_PASSWORD = "password"
BITCOIN_RPC_URL = f"http://{BITCOIN_RPC_USER}:{BITCOIN_RPC_PASSWORD}@{BITCOIN_RPC_HOST}:{BITCOIN_RPC_PORT}"

# Network type
NETWORK = "regtest"  # regtest, testnet, mainnet

# Minimum confirmations
MIN_CONFIRMATIONS = 1

# Transaction fee rate (sat/vB)
FEE_RATE = 1

# Channel related configuration
CHANNEL_TIMELOCK = 144  # Timelock (number of blocks)
CHANNEL_DUST_LIMIT = 546  # Minimum output amount (satoshis)
