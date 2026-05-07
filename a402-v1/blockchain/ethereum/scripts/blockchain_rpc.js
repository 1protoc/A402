#!/usr/bin/env node


const { ethers } = require("ethers");
const fs = require("fs");


function loadDeployment() {
  try {
    const data = fs.readFileSync("./deployment.json", "utf8");
    return JSON.parse(data);
  } catch (error) {
    console.error(JSON.stringify({error: "Contract not deployed"}));
    process.exit(1);
  }
}


async function getContract(provider, wallet) {
  const deployment = loadDeployment();
  const abi = [
    "function createChannel(bytes32 channelId, address mTee) payable",
    "function deposit(bytes32 channelId) payable",
    "function withdraw(bytes32 channelId, address payable to, uint256 amount)",
    "function closeChannel(bytes32 channelId)",
    "function processPayment(bytes32 channelId, uint256 amount, bytes32 adapterPointT, bytes memory signature)",
    "function getChannelInfo(bytes32 channelId) view returns (address uTee, address mTee, uint256 totalAmount, uint256 paidAmount, uint256 nonce, bool isOpen)"
  ];
  
  return new ethers.Contract(deployment.address, abi, wallet);
}

async function main() {
  const command = process.argv[2];
  const rpcUrl = process.env.ETH_RPC_URL || "http://127.0.0.1:8545";
  
  const provider = new ethers.JsonRpcProvider(rpcUrl);
  const wallet = provider.getSigner(0);
  const contract = await getContract(provider, wallet);
  
  try {
    switch (command) {
      case "createChannel": {
        const channelId = process.argv[3];
        const mTee = process.argv[4];
        const amount = process.argv[5];
        
        const tx = await contract.createChannel(channelId, mTee, {value: amount});
        console.log(JSON.stringify({txid: tx.hash}));
        await tx.wait();
        break;
      }
      
      case "deposit": {
        const channelId = process.argv[3];
        const amount = process.argv[4];
        
        const tx = await contract.deposit(channelId, {value: amount});
        console.log(JSON.stringify({txid: tx.hash}));
        await tx.wait();
        break;
      }
      
      case "withdraw": {
        const channelId = process.argv[3];
        const to = process.argv[4];
        const amount = process.argv[5];
        
        const tx = await contract.withdraw(channelId, to, amount);
        console.log(JSON.stringify({txid: tx.hash}));
        await tx.wait();
        break;
      }
      
      case "closeChannel": {
        const channelId = process.argv[3];
        
        const tx = await contract.closeChannel(channelId);
        console.log(JSON.stringify({txid: tx.hash}));
        await tx.wait();
        break;
      }
      
      case "processPayment": {
        const channelId = process.argv[3];
        const amount = process.argv[4];
        const adapterPointT = process.argv[5];
        const signature = process.argv[6];
        
        const tx = await contract.processPayment(channelId, amount, adapterPointT, signature);
        console.log(JSON.stringify({txid: tx.hash}));
        await tx.wait();
        break;
      }
      
      case "sendRaw": {
        const to = process.argv[3];
        const data = process.argv[4];
        const value = process.argv[5] || "0";
        
        const tx = await wallet.sendTransaction({
          to: to,
          data: data,
          value: value
        });
        console.log(JSON.stringify({txid: tx.hash}));
        await tx.wait();
        break;
      }
      
      case "getTx": {
        const txHash = process.argv[3];
        const tx = await provider.getTransaction(txHash);
        if (tx) {
          console.log(tx.data);
        } else {
          console.log("ERROR: Transaction not found");
        }
        break;
      }
      
      case "getReceipt": {
        const txHash = process.argv[3];
        const receipt = await provider.getTransactionReceipt(txHash);
        if (receipt) {
          console.log(JSON.stringify({
            gasUsed: receipt.gasUsed.toString(),
            status: receipt.status,
            blockNumber: receipt.blockNumber.toString(),
            effectiveGasPrice: receipt.gasPrice ? receipt.gasPrice.toString() : "0"
          }));
        } else {
          console.log(JSON.stringify({error: "Transaction receipt not found"}));
        }
        break;
      }
      
      default:
        console.error(JSON.stringify({error: `Unknown command: ${command}`}));
        process.exit(1);
    }
  } catch (error) {
    console.error(JSON.stringify({error: error.message}));
    process.exit(1);
  }
}

main();
