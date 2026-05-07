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


function getContractABI() {
  try {
    const artifact = fs.readFileSync("./artifacts/contracts/PaymentChannel.sol/PaymentChannel.json", "utf8");
    return JSON.parse(artifact).abi;
  } catch (error) {
    return [
      "function createChannel(bytes32 channelId, address userC, address mTee, uint256 challengePeriod) payable"
    ];
  }
}

async function main() {
  const channelId = process.argv[2];
  const userC = process.argv[3];
  const uTee = process.argv[4];  
  const mTee = process.argv[5];
  const challengePeriod = parseInt(process.argv[6]);
  const amount = process.argv[7];
  
  const rpcUrl = process.env.ETH_RPC_URL || "http://127.0.0.1:8545";
  const provider = new ethers.JsonRpcProvider(rpcUrl);
  
  
  
  
  const privateKey = process.env.ETH_U_TEE_PRIVATE_KEY || "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";
  const wallet = new ethers.Wallet(privateKey, provider);
  
  
  if (wallet.address.toLowerCase() !== uTee.toLowerCase()) {
    console.error(JSON.stringify({error: `U-TEE address mismatch: expected ${uTee}, got ${wallet.address}`}));
    process.exit(1);
  }
  
  const deployment = loadDeployment();
  const abi = getContractABI();
  const contract = new ethers.Contract(deployment.address, abi, wallet);
  
  try {
    const tx = await contract.createChannel(channelId, userC, mTee, challengePeriod, {
      value: amount
    });
    
    console.log(JSON.stringify({txid: tx.hash}));
    
    
    const receipt = await tx.wait();
    console.log(JSON.stringify({blockNumber: receipt.blockNumber.toString()}));
  } catch (error) {
    console.error(JSON.stringify({error: error.message}));
    process.exit(1);
  }
}

main();
