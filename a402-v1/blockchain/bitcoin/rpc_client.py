


import requests
import json
from typing import Dict, Any, Optional
from config import BITCOIN_RPC_URL

class BitcoinRPCClient:

    def __init__(self, rpc_url: str = BITCOIN_RPC_URL):
        self.rpc_url = rpc_url
        self.session = requests.Session()
        self.request_id = 0

    def _call(self, method: str, params: list = None) -> Dict[str, Any]:
        if params is None:
            params = []

        self.request_id += 1
        payload = {
            "jsonrpc": "2.0",
            "id": self.request_id,
            "method": method,
            "params": params
        }

        try:
            response = self.session.post(
                self.rpc_url,
                json=payload,
                headers={"Content-Type": "application/json"},
                timeout=10
            )
            response.raise_for_status()
            result = response.json()

            if "error" in result:
                raise Exception(f"RPC Error: {result['error']}")

            return result.get("result")
        except requests.exceptions.RequestException as e:
            raise Exception(f"RPC Connection Error: {e}")

    def get_new_address(self, label: str = "") -> str:
        """Get new address"""
        return self._call("getnewaddress", [label])

    def get_balance(self, minconf: int = 0) -> float:
        """Get balance"""
        return self._call("getbalance", ["*", minconf])

    def send_to_address(self, address: str, amount: float, comment: str = "") -> str:
        """Send to address"""
        return self._call("sendtoaddress", [address, amount, comment])

    def get_transaction(self, txid: str) -> Dict[str, Any]:
        """Get transaction"""
        return self._call("gettransaction", [txid])

    def send_raw_transaction(self, hexstring: str) -> str:
        """Send raw transaction"""
        return self._call("sendrawtransaction", [hexstring])

    def create_raw_transaction(self, inputs: list, outputs: Dict[str, float]) -> str:
        """Create raw transaction"""
        return self._call("createrawtransaction", [inputs, outputs])

    def sign_raw_transaction_with_wallet(self, hexstring: str) -> Dict[str, Any]:
        """Sign raw transaction with wallet"""
        return self._call("signrawtransactionwithwallet", [hexstring])

    def generate_blocks(self, nblocks: int, address: str = None) -> list:
        if address is None:
            address = self.get_new_address()
        return self._call("generatetoaddress", [nblocks, address])

    def get_blockchain_info(self) -> Dict[str, Any]:
        """Get blockchain information"""
        return self._call("getblockchaininfo")

    def list_unspent(self, minconf: int = 1, maxconf: int = 9999999) -> list:
        """List unspent outputs"""
        return self._call("listunspent", [minconf, maxconf])

    def test_connection(self) -> bool:
        """Test connection"""
        try:
            self.get_blockchain_info()
            return True
        except:
            return False
