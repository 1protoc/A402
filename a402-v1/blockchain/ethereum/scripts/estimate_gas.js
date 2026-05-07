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
      "function createChannel(bytes32 channelId, address userC, address mTee, uint256 challengePeriod) payable",
      "function deposit(bytes32 channelId) payable",
      "function withdraw(bytes32 channelId, address payable to, uint256 amount)",
      "function closeChannelByUTee(bytes32 channelId, uint256 userCAmount, uint256 mTeeAmount)",
      "function closeChannelByMTee(bytes32 channelId, uint256 userCAmount, uint256 mTeeAmount, uint256 nonce, bytes memory uTeeSignature, uint8 adapterParity, bytes32 adapterPx, bytes32 adapterE, bytes32 adapterS, bytes32 adapterPointT)",
      "function requestCloseChannelByUser(bytes32 channelId)",
      "function challengeByMTee(bytes32 channelId, uint256 nonce, bytes memory uTeeSignature, uint8 adapterParity, bytes32 adapterPx, bytes32 adapterE, bytes32 adapterS, bytes32 adapterPointT)",
      "function finalizeCloseByUser(bytes32 channelId)",
      "function deposit(bytes32 channelId) payable",
      "function withdraw(bytes32 channelId, uint256 amount, uint256 nonce, bytes memory uTeeSignature)",
      "function withdrawByUTee(bytes32 channelId, uint256 amount)",
      "function finalizeWithdraw(bytes32 channelId)"
    ];
  }
}

async function main() {
  const command = process.argv[2];
  const rpcUrl = process.env.ETH_RPC_URL || "http://127.0.0.1:8545";
  
  
  const provider = new ethers.JsonRpcProvider(rpcUrl);
  
  
  const HARDHAT_ACCOUNTS = [
    "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266",
    "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
    "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"
  ];
  
  
  const accountsList = HARDHAT_ACCOUNTS.map(addr => addr.toLowerCase());
  const defaultAccount = accountsList[0];
  
  
  const fromAddressEnv = process.env.ETH_FROM_ADDRESS;
  const fromAddress = (fromAddressEnv && fromAddressEnv !== "undefined") ? fromAddressEnv.toLowerCase() : defaultAccount;
  const walletAddress = fromAddress;
  
  
  let wallet;
  if (fromAddress === defaultAccount) {
    wallet = provider.getSigner(0);
  } else {
    
    
    const privateKeys = [
      "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
      "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d",
      "0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a"
    ];
    const accountIndex = accountsList.findIndex(addr => addr === fromAddress);
    if (accountIndex >= 0 && accountIndex < privateKeys.length) {
      wallet = new ethers.Wallet(privateKeys[accountIndex], provider);
    } else {
      wallet = provider.getSigner(0); 
    }
  }
  const deployment = loadDeployment();
  const abi = getContractABI();
  const contract = new ethers.Contract(deployment.address, abi, wallet);
  
  try {
    let tx, gasEstimate;
    
    switch (command) {
      case "estimateCreateChannel": {
        const channelId = process.argv[3];
        const userC = process.argv[4];
        const mTee = process.argv[5];
        const challengePeriod = parseInt(process.argv[6]);
        const amount = process.argv[7];
        
        tx = await contract.createChannel.populateTransaction(
          channelId, userC, mTee, challengePeriod,
          { value: amount }
        );
        
        gasEstimate = await provider.estimateGas({
          ...tx,
          from: walletAddress
        });
        
        console.log(JSON.stringify({
          gas_used: gasEstimate.toString(),
          operation: "createChannel"
        }));
        break;
      }
      
      case "estimateCloseByUTee": {
        const channelId = process.argv[3];
        const userCAmount = process.argv[4];
        const mTeeAmount = process.argv[5];
        
        tx = await contract.closeChannelByUTee.populateTransaction(
          channelId, userCAmount, mTeeAmount
        );
        
        gasEstimate = await provider.estimateGas({
          ...tx,
          from: walletAddress
        });
        
        console.log(JSON.stringify({
          gas_used: gasEstimate.toString(),
          operation: "closeChannelByUTee"
        }));
        break;
      }
      
      case "estimateCloseByMTee": {
        const channelId = process.argv[3];
        const userCAmount = process.argv[4];
        const mTeeAmount = process.argv[5];
        const nonce = process.argv[6];
        const uTeeSig = process.argv[7];
        const adapterParity = parseInt(process.argv[8]);
        const adapterPx = process.argv[9];
        const adapterE = process.argv[10];
        const adapterS = process.argv[11];
        const adapterPointT = process.argv[12];
        
        const assetState = {
          userCAmount: userCAmount,
          mTeeAmount: mTeeAmount,
          nonce: nonce,
          uTeeSignature: uTeeSig,
          adapterParity: adapterParity,
          adapterPx: adapterPx,
          adapterE: adapterE,
          adapterS: adapterS,
          adapterPointT: adapterPointT
        };
        
        tx = await contract.closeChannelByMTee.populateTransaction(channelId, assetState);
        
        gasEstimate = await provider.estimateGas({
          ...tx,
          from: walletAddress
        });
        
        console.log(JSON.stringify({
          gas_used: gasEstimate.toString(),
          operation: "closeChannelByMTee"
        }));
        break;
      }
      
      case "estimateRequestCloseByUser": {
        const channelId = process.argv[3];
        
        tx = await contract.requestCloseChannelByUser.populateTransaction(channelId);
        
        gasEstimate = await provider.estimateGas({
          ...tx,
          from: walletAddress
        });
        
        console.log(JSON.stringify({
          gas_used: gasEstimate.toString(),
          operation: "requestCloseChannelByUser"
        }));
        break;
      }
      
      case "estimateChallengeByMTee": {
        const channelId = process.argv[3];
        const nonce = process.argv[4];
        const uTeeSig = process.argv[5];
        const adapterParity = parseInt(process.argv[6]);
        const adapterPx = process.argv[7];
        const adapterE = process.argv[8];
        const adapterS = process.argv[9];
        const adapterPointT = process.argv[10];
        
        tx = await contract.challengeByMTee.populateTransaction(
          channelId, nonce, uTeeSig, adapterParity,
          adapterPx, adapterE, adapterS, adapterPointT
        );
        
        gasEstimate = await provider.estimateGas({
          ...tx,
          from: walletAddress
        });
        
        console.log(JSON.stringify({
          gas_used: gasEstimate.toString(),
          operation: "challengeByMTee"
        }));
        break;
      }
      
      case "estimateFinalizeCloseByUser": {
        const channelId = process.argv[3];
        
        tx = await contract.finalizeCloseByUser.populateTransaction(channelId);
        
        gasEstimate = await provider.estimateGas({
          ...tx,
          from: walletAddress
        });
        
        console.log(JSON.stringify({
          gas_used: gasEstimate.toString(),
          operation: "finalizeCloseByUser"
        }));
        break;
      }
      
      case "estimateDeposit": {
        const uTeeAddress = process.argv[3];
        const amount = process.argv[4];
        
        tx = await contract.deposit.populateTransaction(uTeeAddress);
        
        
        const value = BigInt(amount);
        gasEstimate = await provider.estimateGas({
          ...tx,
          from: walletAddress,
          value: value
        });
        
        console.log(JSON.stringify({
          gas_used: gasEstimate.toString(),
          operation: "deposit"
        }));
        break;
      }
      
      case "estimateWithdraw": {
        const uTeeAddress = process.argv[3];
        const amount = process.argv[4];
        const nonce = process.argv[5];
        const uTeeSig = process.argv[6];
        const challengePeriod = process.argv[7];
        
        tx = await contract.withdraw.populateTransaction(
          uTeeAddress, amount, nonce, uTeeSig, challengePeriod
        );
        
        gasEstimate = await provider.estimateGas({
          ...tx,
          from: walletAddress
        });
        
        console.log(JSON.stringify({
          gas_used: gasEstimate.toString(),
          operation: "withdraw"
        }));
        break;
      }
      
      case "estimateWithdrawByUTee": {
        const userCAddress = process.argv[3];
        const amount = process.argv[4];
        
        tx = await contract.withdrawByUTee.populateTransaction(userCAddress, amount);
        
        gasEstimate = await provider.estimateGas({
          ...tx,
          from: walletAddress
        });
        
        console.log(JSON.stringify({
          gas_used: gasEstimate.toString(),
          operation: "withdrawByUTee"
        }));
        break;
      }
      
      case "estimateFinalizeWithdraw": {
        tx = await contract.finalizeWithdraw.populateTransaction();
        
        gasEstimate = await provider.estimateGas({
          ...tx,
          from: walletAddress
        });
        
        console.log(JSON.stringify({
          gas_used: gasEstimate.toString(),
          operation: "finalizeWithdraw"
        }));
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
