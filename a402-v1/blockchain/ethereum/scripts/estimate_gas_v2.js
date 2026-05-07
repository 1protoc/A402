#!/usr/bin/env node


const { ethers } = require("ethers");
const fs = require("fs");


function loadDeployment() {
  try {
    const data = fs.readFileSync("./deployment_v2.json", "utf8");
    return JSON.parse(data);
  } catch (error) {
    console.error(JSON.stringify({error: "Contract not deployed. Please deploy PaymentChannelV2 first."}));
    process.exit(1);
  }
}


function getContractABI() {
  try {
    const artifact = fs.readFileSync("./artifacts/contracts/PaymentChannelV2.sol/PaymentChannelV2.json", "utf8");
    return JSON.parse(artifact).abi;
  } catch (error) {
    console.error(JSON.stringify({error: "Contract ABI not found. Please compile the contract first."}));
    process.exit(1);
  }
}


function getERC20ABI() {
  return [
    "function transfer(address to, uint256 amount) external returns (bool)",
    "function transferFrom(address from, address to, uint256 amount) external returns (bool)",
    "function approve(address spender, uint256 amount) external returns (bool)",
    "function balanceOf(address account) external view returns (uint256)",
    "function allowance(address owner, address spender) external view returns (uint256)"
  ];
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
  
  
  const tokenAddress = deployment.tokenAddress || process.env.TOKEN_ADDRESS;
  if (!tokenAddress && command !== "estimateAll") {
    console.error(JSON.stringify({error: "Token address not found. Please set TOKEN_ADDRESS env var or add to deployment_v2.json"}));
    process.exit(1);
  }
  
  try {
    let tx, gasEstimate;
    
    switch (command) {
      case "estimateCreateChannel": {
        const channelId = process.argv[3];
        const userC = process.argv[4];
        const mTee = process.argv[5];
        const token = process.argv[6] || tokenAddress;
        const amount = process.argv[7];
        const challengePeriod = parseInt(process.argv[8]);
        
        
        const erc20ABI = getERC20ABI();
        const tokenContract = new ethers.Contract(token, erc20ABI, wallet);
        const approveTx = await tokenContract.approve.populateTransaction(deployment.address, amount);
        await provider.estimateGas({...approveTx, from: walletAddress});
        
        tx = await contract.createChannel.populateTransaction(
          channelId, userC, mTee, token, amount, challengePeriod
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
        const adapterSig = process.argv[8];
        const adapterPointT = process.argv[9];
        
        const assetState = {
          userCAmount: userCAmount,
          mTeeAmount: mTeeAmount,
          nonce: nonce,
          uTeeSignature: uTeeSig,
          adapterSignature: adapterSig,
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
        const userCAmount = process.argv[4];
        const mTeeAmount = process.argv[5];
        const nonce = process.argv[6];
        const uTeeSig = process.argv[7];
        const adapterSig = process.argv[8];
        const adapterPointT = process.argv[9];
        
        const assetState = {
          userCAmount: userCAmount,
          mTeeAmount: mTeeAmount,
          nonce: nonce,
          uTeeSignature: uTeeSig,
          adapterSignature: adapterSig,
          adapterPointT: adapterPointT
        };
        
        tx = await contract.challengeByMTee.populateTransaction(channelId, assetState);
        
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
        const token = process.argv[4] || tokenAddress;
        const amount = process.argv[5];
        
        
        const erc20ABI = getERC20ABI();
        const tokenContract = new ethers.Contract(token, erc20ABI, wallet);
        const approveTx = await tokenContract.approve.populateTransaction(deployment.address, amount);
        await provider.estimateGas({...approveTx, from: walletAddress});
        
        tx = await contract.deposit.populateTransaction(uTeeAddress, token, amount);
        
        gasEstimate = await provider.estimateGas({
          ...tx,
          from: walletAddress
        });
        
        console.log(JSON.stringify({
          gas_used: gasEstimate.toString(),
          operation: "deposit"
        }));
        break;
      }
      
      case "estimateWithdraw": {
        const uTeeAddress = process.argv[3];
        const token = process.argv[4] || tokenAddress;
        const amount = process.argv[5];
        const nonce = process.argv[6];
        const uTeeSig = process.argv[7];
        const challengePeriod = process.argv[8];
        
        tx = await contract.withdraw.populateTransaction(
          uTeeAddress, token, amount, nonce, uTeeSig, challengePeriod
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
        const token = process.argv[4] || tokenAddress;
        const amount = process.argv[5];
        
        tx = await contract.withdrawByUTee.populateTransaction(userCAddress, token, amount);
        
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
        console.error("Available commands:");
        console.error("  estimateCreateChannel");
        console.error("  estimateCloseByUTee");
        console.error("  estimateCloseByMTee");
        console.error("  estimateRequestCloseByUser");
        console.error("  estimateChallengeByMTee");
        console.error("  estimateFinalizeCloseByUser");
        console.error("  estimateDeposit");
        console.error("  estimateWithdraw");
        console.error("  estimateWithdrawByUTee");
        console.error("  estimateFinalizeWithdraw");
        process.exit(1);
    }
  } catch (error) {
    console.error(JSON.stringify({error: error.message}));
    process.exit(1);
  }
}

main();
