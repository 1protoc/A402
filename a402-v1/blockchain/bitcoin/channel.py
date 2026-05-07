
import hashlib
import json
from typing import Dict, Optional
from rpc_client import BitcoinRPCClient
from config import CHANNEL_DUST_LIMIT

class BitcoinChannel:

    def __init__(self, rpc_client: BitcoinRPCClient):
        self.rpc = rpc_client
        self.channels: Dict[str, Dict] = {}

    def create_channel(self, channel_id: str, u_tee_pubkey: str,
                      m_tee_pubkey: str, amount: int) -> Dict[str, Any]:
        """Create Channel"""
        if channel_id in self.channels:
            raise ValueError(f"Channel {channel_id} already exists")


        unspent = self.rpc.list_unspent()
        if not unspent:
            raise Exception("No unspent outputs available")


        total_input = 0
        inputs = []
        for utxo in unspent:
            if total_input >= amount + 10000:
                break
            inputs.append({
                "txid": utxo["txid"],
                "vout": utxo["vout"]
            })
            total_input += int(utxo["amount"] * 100000000)

        if total_input < amount + 10000:
            raise Exception("Insufficient balance")


        channel_address = self.rpc.get_new_address(f"channel_{channel_id}")

        outputs = {
            channel_address: amount / 100000000.0,
        }


        change = total_input - amount - 10000
        if change > CHANNEL_DUST_LIMIT:
            change_address = self.rpc.get_new_address()
            outputs[change_address] = change / 100000000.0


        raw_tx = self.rpc.create_raw_transaction(inputs, outputs)
        signed = self.rpc.sign_raw_transaction_with_wallet(raw_tx)

        if not signed.get("complete"):
            raise Exception(f"Transaction not fully signed: {signed.get('errors')}")


        txid = self.rpc.send_raw_transaction(signed["hex"])


        self.rpc.generate_blocks(1)


        channel_info = {
            "channel_id": channel_id,
            "u_tee_pubkey": u_tee_pubkey,
            "m_tee_pubkey": m_tee_pubkey,
            "amount": amount,
            "paid_amount": 0,
            "txid": txid,
            "address": channel_address,
            "state": "open"
        }

        self.channels[channel_id] = channel_info

        return channel_info

    def deposit(self, channel_id: str, amount: int) -> str:
        """Deposit to Channel"""
        if channel_id not in self.channels:
            raise ValueError(f"Channel {channel_id} not found")

        channel = self.channels[channel_id]
        channel_address = channel["address"]

        txid = self.rpc.send_to_address(channel_address, amount / 100000000.0,
                                        f"deposit_{channel_id}")

        channel["amount"] += amount
        self.rpc.generate_blocks(1)

        return txid

    def withdraw(self, channel_id: str, amount: int, to_address: str) -> str:
        """Withdraw from Channel"""
        if channel_id not in self.channels:
            raise ValueError(f"Channel {channel_id} not found")

        channel = self.channels[channel_id]

        if channel["amount"] - channel["paid_amount"] < amount:
            raise ValueError("Insufficient channel balance")

        unspent = self.rpc.list_unspent()
        channel_utxos = [utxo for utxo in unspent
                        if utxo.get("address") == channel["address"]]

        if not channel_utxos:
            raise Exception("No UTXOs found for channel")

        total_input = sum(int(utxo["amount"] * 100000000) for utxo in channel_utxos)

        inputs = [{"txid": utxo["txid"], "vout": utxo["vout"]}
                 for utxo in channel_utxos]

        outputs = {
            to_address: amount / 100000000.0,
        }

        change = total_input - amount - 10000
        if change > CHANNEL_DUST_LIMIT:
            outputs[channel["address"]] = change / 100000000.0

        raw_tx = self.rpc.create_raw_transaction(inputs, outputs)
        signed = self.rpc.sign_raw_transaction_with_wallet(raw_tx)

        if not signed.get("complete"):
            raise Exception(f"Transaction not fully signed: {signed.get('errors')}")

        txid = self.rpc.send_raw_transaction(signed["hex"])

        channel["amount"] -= amount
        channel["paid_amount"] += amount

        self.rpc.generate_blocks(1)

        return txid

    def close_channel(self, channel_id: str, refund_address: str) -> str:
        """Close Channel"""
        if channel_id not in self.channels:
            raise ValueError(f"Channel {channel_id} not found")

        channel = self.channels[channel_id]

        unspent = self.rpc.list_unspent()
        channel_utxos = [utxo for utxo in unspent
                        if utxo.get("address") == channel["address"]]

        if not channel_utxos:
            raise Exception("No UTXOs found for channel")

        total_amount = sum(int(utxo["amount"] * 100000000) for utxo in channel_utxos)
        remaining = total_amount - channel["paid_amount"]

        if remaining <= 0:
            raise Exception("No remaining balance to refund")

        inputs = [{"txid": utxo["txid"], "vout": utxo["vout"]}
                 for utxo in channel_utxos]

        outputs = {
            refund_address: remaining / 100000000.0,
        }

        raw_tx = self.rpc.create_raw_transaction(inputs, outputs)
        signed = self.rpc.sign_raw_transaction_with_wallet(raw_tx)

        if not signed.get("complete"):
            raise Exception(f"Transaction not fully signed: {signed.get('errors')}")

        txid = self.rpc.send_raw_transaction(signed["hex"])

        channel["state"] = "closed"
        self.rpc.generate_blocks(1)

        return txid

    def get_channel_info(self, channel_id: str) -> Optional[Dict]:
        """Get Channel Information"""
        return self.channels.get(channel_id)
