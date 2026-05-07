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

async function main() {
  const uTeeAddress = process.argv[2];
  const amount = process.argv[3];
  const userCAddress = process.env.ETH_FROM_ADDRESS || "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
  
  const rpcUrl = process.env.ETH_RPC_URL || "http://127.0.0.1:8545";
  const provider = new ethers.JsonRpcProvider(rpcUrl);
  
  
  const privateKeys = {
    "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266": "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
    "0x70997970C51812dc3A010C7d01b50e0d17dc79C8": "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d",
    "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC": "0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a"
  };
  
  const userCAddressLower = userCAddress.toLowerCase();
  const privateKey = privateKeys[userCAddressLower] || privateKeys["0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"];
  const wallet = new ethers.Wallet(privateKey, provider);
  
  
  if (wallet.address.toLowerCase() !== userCAddressLower) {
    console.error(JSON.stringify({error: `Address mismatch: expected ${userCAddress}, got ${wallet.address}`}));
    process.exit(1);
  }
  
  const deployment = loadDeployment();
  const abi = [
    "function deposit(address uTeeAddress) payable"
  ];
  const contract = new ethers.Contract(deployment.address, abi, wallet);
  
  try {
    
    
    const value = BigInt(amount);
    
    
    const data = contract.interface.encodeFunctionData('deposit', [uTeeAddress]);
    const tx = await wallet.sendTransaction({
      to: deployment.address,
      data: data,
      value: value,
      gasLimit: 100000 
    });
    
    console.log(JSON.stringify({txid: tx.hash}));
    const receipt = await tx.wait();
    
    
    console.log(JSON.stringify({gasUsed: receipt.gasUsed.toString()}));
  } catch (error) {
    console.error(JSON.stringify({error: error.message}));
    process.exit(1);
  }
}

main();
