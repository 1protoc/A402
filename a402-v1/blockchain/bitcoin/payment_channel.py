
"""
Bitcoin Payment Channel Implementation
Uses P2TR (SegWit v1) and BIP340 Schnorr signatures
Supports createChannel, closeChannel, deposit, withdraw
"""

import hashlib
import json
import struct
import binascii
from typing import Dict, List, Optional, Tuple
from rpc_client import BitcoinRPCClient
from scripts.payment_channel import PaymentChannelScript
from config import CHANNEL_DUST_LIMIT, NETWORK

class BitcoinPaymentChannel:
    """Bitcoin Taproot Payment Channel Manager (using P2TR)"""

    def __init__(self, rpc_client: BitcoinRPCClient):
        self.rpc = rpc_client
        self.channels: Dict[str, Dict] = {}

    def _calculate_tx_size(self, inputs: List[Dict], outputs: Dict[str, float],
                           witness_stacks: Optional[List[List[bytes]]] = None,
                           is_segwit: bool = True) -> int:
        """
        Estimate Taproot transaction size (in bytes)

        Taproot transactions use SegWit, witness data is not counted in base transaction size

        Args:
            inputs: List of inputs
            outputs: Dictionary of outputs
            witness_stacks: List of witness stacks (optional)
            is_segwit: Whether to use SegWit

        Returns:
            Transaction size (in bytes)
        """
        # Base transaction size (excluding witness)
        size = 4  # version (4 bytes)
        size += 1  # marker (0x00 for SegWit)
        size += 1  # flag (0x01 for SegWit)
        size += 1  # input count (varint)

        # Input size (excluding witness)
        for inp in inputs:
            size += 32  # txid (32 bytes)
            size += 4   # vout (4 bytes)
            size += 1   # script length (0 for SegWit)
            size += 4   # sequence (4 bytes)

        size += 1  # output count (varint)

        # Output size
        for address, amount in outputs.items():
            size += 8  # amount (8 bytes)
            size += 1  # script length (varint)
            # P2TR output script: OP_1 <32 bytes> (33 bytes)
            size += 1 + 32  # 33 bytes

        # Witness data (not counted in base size, but included in total size)
        witness_size = 0
        if witness_stacks:
            witness_size += 1  # witness element count (varint)
            for witness_stack in witness_stacks:
                witness_size += 1  # stack element count (varint)
                for item in witness_stack:
                    witness_size += len(item) + 1  # item length + item

        # Total size = base size + witness size
        total_size = size + witness_size

        # For SegWit transactions, virtual size = (base size * 3 + witness size) / 4
        # But actual transaction size is base size + witness size
        return total_size

    def create_channel(self, channel_id: str, u_tee_pubkey: str,
                      m_tee_pubkey: str, user_c_pubkey: str,
                      amount: int, challenge_period: int = 144) -> Dict:
        """
        Create Taproot payment channel

        Args:
            channel_id: Channel ID
            u_tee_pubkey: U-TEE public key (hex string, 32 bytes BIP340 format)
            m_tee_pubkey: M-TEE public key (hex string, 32 bytes BIP340 format)
            user_c_pubkey: User C public key (hex string, 32 bytes BIP340 format)
            amount: Channel amount (satoshis)
            challenge_period: Challenge period (number of blocks)

        Returns:
            Channel information dictionary
        """
        if channel_id in self.channels:
            raise ValueError(f"Channel {channel_id} already exists")

        # Convert public keys
        u_tee_pubkey_bytes = binascii.unhexlify(u_tee_pubkey)
        m_tee_pubkey_bytes = binascii.unhexlify(m_tee_pubkey)
        user_c_pubkey_bytes = binascii.unhexlify(user_c_pubkey)

        # Create Tapscript
        tapscript = PaymentChannelScript.create_tapscript(
            u_tee_pubkey_bytes, m_tee_pubkey_bytes, user_c_pubkey_bytes, challenge_period
        )

        # Create P2TR address
        p2tr_address, taproot_output_key = PaymentChannelScript.create_p2tr_address(
            tapscript, None, NETWORK
        )

        # Get UTXOs for creating channel
        unspent = self.rpc.list_unspent()
        if not unspent:
            raise Exception("No unspent outputs available")

        # Select sufficient UTXOs
        total_input = 0
        inputs = []
        for utxo in unspent:
            if total_input >= amount + 10000:  # Include transaction fee
                break
            inputs.append({
                "txid": utxo["txid"],
                "vout": utxo["vout"]
            })
            total_input += int(utxo["amount"] * 100000000)

        if total_input < amount + 10000:
            raise Exception("Insufficient balance")

        # Create outputs
        outputs = {
            p2tr_address: amount / 100000000.0,
        }

        # Change output
        change = total_input - amount - 10000
        if change > CHANNEL_DUST_LIMIT:
            change_address = self.rpc.get_new_address()
            outputs[change_address] = change / 100000000.0

        # Create and sign transaction
        raw_tx = self.rpc.create_raw_transaction(inputs, outputs)
        signed = self.rpc.sign_raw_transaction_with_wallet(raw_tx)

        if not signed.get("complete"):
            raise Exception(f"Transaction not fully signed: {signed.get('errors')}")

        # Estimate transaction size
        tx_size = self._calculate_tx_size(inputs, outputs)

        # Send transaction
        txid = self.rpc.send_raw_transaction(signed["hex"])

        # Wait for confirmation
        self.rpc.generate_blocks(1)

        # Store channel information
        channel_info = {
            "channel_id": channel_id,
            "u_tee_pubkey": u_tee_pubkey,
            "m_tee_pubkey": m_tee_pubkey,
            "user_c_pubkey": user_c_pubkey,
            "amount": amount,
            "paid_amount": 0,
            "txid": txid,
            "p2tr_address": p2tr_address,
            "p2sh_address": p2tr_address,  # Backward compatibility, use p2tr_address
            "tapscript": binascii.hexlify(tapscript).decode('ascii'),
            "redeem_script": binascii.hexlify(tapscript).decode('ascii'),  # Backward compatibility
            "taproot_output_key": binascii.hexlify(taproot_output_key).decode('ascii'),
            "script_hash": binascii.hexlify(taproot_output_key[:20]).decode('ascii'),  # Backward compatibility
            "challenge_period": challenge_period,
            "state": "open",
            "tx_size": tx_size
        }

        self.channels[channel_id] = channel_info

        return channel_info

    def deposit(self, channel_id: str, amount: int) -> Dict:
        """
        Deposit to channel

        Args:
            channel_id: Channel ID
            amount: Deposit amount (satoshis)

        Returns:
            Transaction information
        """
        if channel_id not in self.channels:
            raise ValueError(f"Channel {channel_id} not found")

        channel = self.channels[channel_id]
        p2tr_address = channel["p2tr_address"]

        # Send to P2TR address
        txid = self.rpc.send_to_address(p2tr_address, amount / 100000000.0,
                                        f"deposit_{channel_id}")

        # Get transaction information to calculate size
        tx_info = self.rpc.get_transaction(txid)
        tx_hex = tx_info.get("hex", "")
        tx_size = len(binascii.unhexlify(tx_hex)) if tx_hex else 0

        channel["amount"] += amount
        self.rpc.generate_blocks(1)

        return {
            "txid": txid,
            "amount": amount,
            "tx_size": tx_size
        }

    def withdraw(self, channel_id: str, amount: int, to_address: str,
                condition: str = "u_tee", signatures: Optional[List[str]] = None) -> Dict:
        """
        Withdraw from channel

        Args:
            channel_id: Channel ID
            amount: Withdrawal amount (satoshis)
            to_address: Withdrawal target address
            condition: Unlock condition ("u_tee", "m_tee", "user_c")
            signatures: List of Schnorr signatures (hex string, 64 bytes BIP340 format)
                       For backward compatibility, parameter name remains signatures, but actual is Schnorr signature

        Returns:
            Transaction information
        """
        if channel_id not in self.channels:
            raise ValueError(f"Channel {channel_id} not found")

        channel = self.channels[channel_id]

        if channel["amount"] - channel["paid_amount"] < amount:
            raise ValueError("Insufficient channel balance")

        # Get channel UTXOs
        unspent = self.rpc.list_unspent()
        channel_utxos = [utxo for utxo in unspent
                        if utxo.get("address") == channel["p2tr_address"]]

        if not channel_utxos:
            raise Exception("No UTXOs found for channel")

        total_input = sum(int(utxo["amount"] * 100000000) for utxo in channel_utxos)

        inputs = [{"txid": utxo["txid"], "vout": utxo["vout"]}
                 for utxo in channel_utxos]

        outputs = {
            to_address: amount / 100000000.0,
        }

        # Change back to channel
        change = total_input - amount - 10000
        if change > CHANNEL_DUST_LIMIT:
            outputs[channel["p2tr_address"]] = change / 100000000.0

        # Create raw transaction
        raw_tx = self.rpc.create_raw_transaction(inputs, outputs)

        # Build witness stack
        tapscript = binascii.unhexlify(channel["tapscript"])
        if signatures:
            sig_bytes = [binascii.unhexlify(sig) for sig in signatures]
            witness_stack = PaymentChannelScript.create_witness_stack(
                condition, sig_bytes, tapscript
            )
        else:
            witness_stack = None

        # Estimate transaction size
        witness_stacks = [witness_stack] if witness_stack else None
        tx_size = self._calculate_tx_size(inputs, outputs, witness_stacks)

        # Sign transaction
        signed = self.rpc.sign_raw_transaction_with_wallet(raw_tx)

        if not signed.get("complete"):
            raise Exception(f"Transaction not fully signed: {signed.get('errors')}")

        txid = self.rpc.send_raw_transaction(signed["hex"])

        channel["amount"] -= amount
        channel["paid_amount"] += amount

        self.rpc.generate_blocks(1)

        return {
            "txid": txid,
            "amount": amount,
            "to_address": to_address,
            "tx_size": tx_size
        }

    def close_channel(self, channel_id: str, user_c_amount: int, m_tee_amount: int,
                     condition: str = "u_tee", signatures: Optional[List[str]] = None,
                     user_c_address: Optional[str] = None,
                     m_tee_address: Optional[str] = None) -> Dict:
        """
        Close channel

        Args:
            channel_id: Channel ID
            user_c_amount: Amount for user C (satoshis)
            m_tee_amount: Amount for M-TEE (satoshis)
            condition: Unlock condition ("u_tee", "m_tee", "user_c")
            signatures: List of Schnorr signatures (hex string, 64 bytes BIP340 format)
                       For backward compatibility, parameter name remains signatures, but actual is Schnorr signature
            user_c_address: User C address (optional)
            m_tee_address: M-TEE address (optional)

        Returns:
            Transaction information
        """
        if channel_id not in self.channels:
            raise ValueError(f"Channel {channel_id} not found")

        channel = self.channels[channel_id]

        if user_c_amount + m_tee_amount > channel["amount"]:
            raise ValueError("Total amount exceeds channel balance")

        # Get channel UTXOs
        unspent = self.rpc.list_unspent()
        channel_utxos = [utxo for utxo in unspent
                        if utxo.get("address") == channel["p2tr_address"]]

        if not channel_utxos:
            raise Exception("No UTXOs found for channel")

        total_amount = sum(int(utxo["amount"] * 100000000) for utxo in channel_utxos)

        inputs = [{"txid": utxo["txid"], "vout": utxo["vout"]}
                 for utxo in channel_utxos]

        # Create outputs
        outputs = {}
        if user_c_amount > 0:
            if not user_c_address:
                user_c_address = self.rpc.get_new_address("user_c")
            outputs[user_c_address] = user_c_amount / 100000000.0

        if m_tee_amount > 0:
            if not m_tee_address:
                m_tee_address = self.rpc.get_new_address("m_tee")
            outputs[m_tee_address] = m_tee_amount / 100000000.0

        # Change output (if any remaining)
        change = total_amount - user_c_amount - m_tee_amount - 10000
        if change > CHANNEL_DUST_LIMIT:
            change_address = self.rpc.get_new_address()
            outputs[change_address] = change / 100000000.0

        # Create raw transaction
        raw_tx = self.rpc.create_raw_transaction(inputs, outputs)

        # Build witness stack
        tapscript = binascii.unhexlify(channel["tapscript"])
        if signatures:
            sig_bytes = [binascii.unhexlify(sig) for sig in signatures]
            witness_stack = PaymentChannelScript.create_witness_stack(
                condition, sig_bytes, tapscript
            )
        else:
            witness_stack = None

        # Estimate transaction size
        witness_stacks = [witness_stack] if witness_stack else None
        tx_size = self._calculate_tx_size(inputs, outputs, witness_stacks)

        # Sign transaction
        signed = self.rpc.sign_raw_transaction_with_wallet(raw_tx)

        if not signed.get("complete"):
            raise Exception(f"Transaction not fully signed: {signed.get('errors')}")

        txid = self.rpc.send_raw_transaction(signed["hex"])

        channel["state"] = "closed"
        self.rpc.generate_blocks(1)

        return {
            "txid": txid,
            "user_c_amount": user_c_amount,
            "m_tee_amount": m_tee_amount,
            "tx_size": tx_size
        }

    def get_channel_info(self, channel_id: str) -> Optional[Dict]:
        """Get channel information"""
        return self.channels.get(channel_id)

    def estimate_tx_size(self, channel_id: str, operation: str,
                        amount: Optional[int] = None) -> int:
        """
        Estimate transaction size

        Args:
            channel_id: Channel ID
            operation: Operation type ("create", "deposit", "withdraw", "close")
            amount: Amount (optional)

        Returns:
            Transaction size (bytes)
        """
        if channel_id not in self.channels:
            raise ValueError(f"Channel {channel_id} not found")

        channel = self.channels[channel_id]

        if operation == "create":
            # Create channel: 1 input, 1-2 outputs
            inputs = [{"txid": "0" * 64, "vout": 0}]
            outputs = {channel["p2tr_address"]: channel["amount"] / 100000000.0}
            return self._calculate_tx_size(inputs, outputs)

        elif operation == "deposit":
            # Deposit: 1 input, 1 output
            inputs = [{"txid": "0" * 64, "vout": 0}]
            outputs = {channel["p2tr_address"]: (amount or 100000) / 100000000.0}
            return self._calculate_tx_size(inputs, outputs)

        elif operation == "withdraw":
            # Withdraw: 1 input, 1-2 outputs, requires witness
            inputs = [{"txid": "0" * 64, "vout": 0}]
            outputs = {"1" * 34: (amount or 100000) / 100000000.0}
            witness_size = PaymentChannelScript.estimate_witness_size("u_tee", 1)
            return self._calculate_tx_size(inputs, outputs) + witness_size

        elif operation == "close":
            # Close: 1 input, 2-3 outputs, requires witness
            inputs = [{"txid": "0" * 64, "vout": 0}]
            outputs = {
                "1" * 34: channel["amount"] / 2 / 100000000.0,
                "2" * 34: channel["amount"] / 2 / 100000000.0
            }
            witness_size = PaymentChannelScript.estimate_witness_size("u_tee", 1)
            return self._calculate_tx_size(inputs, outputs) + witness_size

        else:
            raise ValueError(f"Invalid operation: {operation}")

# Backward compatibility: provide alias
BitcoinTaprootPaymentChannel = BitcoinPaymentChannel
