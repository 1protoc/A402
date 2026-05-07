#!/usr/bin/env python3

import sys
import json
import os
from rpc_client import BitcoinRPCClient
from payment_channel import BitcoinPaymentChannel

def main():
    if len(sys.argv) < 2:
        print(json.dumps({"error": "Missing command"}))
        sys.exit(1)
    
    command = sys.argv[1]
    rpc_url = os.environ.get("BITCOIN_RPC_URL", "http://127.0.0.1:18332")
    
    try:
        rpc_client = BitcoinRPCClient(rpc_url)
        channel_manager = BitcoinPaymentChannel(rpc_client)
        
        if command == "createChannel":
            if len(sys.argv) < 7:
                print(json.dumps({"error": "Usage: createChannel <channel_id> <u_tee_pubkey> <m_tee_pubkey> <user_c_pubkey> <amount_satoshis> [challenge_period]"}))
                sys.exit(1)
            
            channel_id = sys.argv[2]
            u_tee_pubkey = sys.argv[3]
            m_tee_pubkey = sys.argv[4]
            user_c_pubkey = sys.argv[5]
            amount = int(sys.argv[6])
            challenge_period = int(sys.argv[7]) if len(sys.argv) > 7 else 144
            
            result = channel_manager.create_channel(
                channel_id, u_tee_pubkey, m_tee_pubkey, user_c_pubkey,
                amount, challenge_period
            )
            print(json.dumps({"txid": result["txid"], "p2tr_address": result["p2tr_address"]}))
        
        elif command == "deposit":
            if len(sys.argv) < 4:
                print(json.dumps({"error": "Usage: deposit <channel_id> <amount_satoshis>"}))
                sys.exit(1)
            
            channel_id = sys.argv[2]
            amount = int(sys.argv[3])
            
            result = channel_manager.deposit(channel_id, amount)
            print(json.dumps({"txid": result["txid"]}))
        
        elif command == "withdraw":
            if len(sys.argv) < 5:
                print(json.dumps({"error": "Usage: withdraw <channel_id> <amount_satoshis> <to_address>"}))
                sys.exit(1)
            
            channel_id = sys.argv[2]
            amount = int(sys.argv[3])
            to_address = sys.argv[4]
            
            result = channel_manager.withdraw(channel_id, amount, to_address)
            print(json.dumps({"txid": result["txid"]}))
        
        elif command == "closeChannel":
            if len(sys.argv) < 5:
                print(json.dumps({"error": "Usage: closeChannel <channel_id> <user_c_amount> <m_tee_amount> [condition] [signatures...]"}))
                sys.exit(1)
            
            channel_id = sys.argv[2]
            user_c_amount = int(sys.argv[3])
            m_tee_amount = int(sys.argv[4])
            condition = sys.argv[5] if len(sys.argv) > 5 else "u_tee"
            signatures = sys.argv[6:] if len(sys.argv) > 6 else None
            
            result = channel_manager.close_channel(
                channel_id, user_c_amount, m_tee_amount,
                condition, signatures
            )
            print(json.dumps({"txid": result["txid"]}))
        
        elif command == "getChannelInfo":
            if len(sys.argv) < 3:
                print(json.dumps({"error": "Usage: getChannelInfo <channel_id>"}))
                sys.exit(1)
            
            channel_id = sys.argv[2]
            info = channel_manager.get_channel_info(channel_id)
            
            if info:
                print(json.dumps({
                    "channel_id": channel_id,
                    "p2tr_address": info.get("p2tr_address", ""),
                    "amount": info.get("amount", 0),
                    "state": info.get("state", "unknown")
                }))
            else:
                print(json.dumps({"error": "Channel not found"}))
                sys.exit(1)
        
        elif command == "sendRaw":
            if len(sys.argv) < 3:
                print(json.dumps({"error": "Usage: sendRaw <hex_transaction>"}))
                sys.exit(1)
            
            hex_tx = sys.argv[2]
            txid = rpc_client.send_raw_transaction(hex_tx)
            print(json.dumps({"txid": txid}))
        
        elif command == "getTx":
            if len(sys.argv) < 3:
                print(json.dumps({"error": "Usage: getTx <txid>"}))
                sys.exit(1)
            
            txid = sys.argv[2]
            tx_info = rpc_client.get_transaction(txid)
            
            if tx_info:
                print(json.dumps({
                    "txid": tx_info.get("txid", txid),
                    "hex": tx_info.get("hex", ""),
                    "confirmations": tx_info.get("confirmations", 0)
                }))
            else:
                print(json.dumps({"error": "Transaction not found"}))
                sys.exit(1)
        
        else:
            print(json.dumps({"error": f"Unknown command: {command}"}))
            sys.exit(1)
    
    except Exception as e:
        print(json.dumps({"error": str(e)}))
        sys.exit(1)

if __name__ == "__main__":
    main()
