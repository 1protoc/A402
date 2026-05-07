#!/usr/bin/env node


const { ethers } = require("hardhat");
const fs = require("fs");

async function main() {
  console.log("=== DeployPaymentChannelV2和TestERC20 ===\n");
  
  const [deployer, userC, uTee, mTee] = await ethers.getSigners();
  
  console.log("Deploy账户:", deployer.address);
  console.log("账户Balance:", ethers.formatEther(await ethers.provider.getBalance(deployer.address)), "ETH\n");
  
  
  console.log("1. DeployTestERC20...");
  const TestERC20 = await ethers.getContractFactory("TestERC20");
  const initialSupply = ethers.parseEther("1000000"); 
  const testToken = await TestERC20.deploy(
    "Test Token",
    "TEST",
    18,
    initialSupply
  );
  await testToken.waitForDeployment();
  const tokenAddress = await testToken.getAddress();
  console.log("   TestERC20Address:", tokenAddress);
  
  
  console.log("\n2. DeployPaymentChannelV2...");
  const PaymentChannelV2 = await ethers.getContractFactory("PaymentChannelV2");
  const paymentChannel = await PaymentChannelV2.deploy();
  await paymentChannel.waitForDeployment();
  const contractAddress = await paymentChannel.getAddress();
  console.log("   PaymentChannelV2Address:", contractAddress);
  
  
  console.log("\n3. 分配代币给各个账户...");
  const distributeAmount = ethers.parseEther("100000"); 
  await testToken.transfer(userC.address, distributeAmount);
  await testToken.transfer(uTee.address, distributeAmount);
  await testToken.transfer(mTee.address, distributeAmount);
  console.log("   已分配代币给 userC, uTee, mTee");
  
  
  const network = await ethers.provider.getNetwork();
  const deployment = {
    address: contractAddress,
    tokenAddress: tokenAddress,
    deployer: deployer.address,
    userC: userC.address,
    uTee: uTee.address,
    mTee: mTee.address,
    network: network.name,
    chainId: network.chainId.toString(),
    timestamp: new Date().toISOString()
  };
  
  fs.writeFileSync(
    "./deployment_v2.json",
    JSON.stringify(deployment, null, 2)
  );
  
  console.log("\n=== Deploy完成 ===");
  console.log("DeployInformation已保存到: deployment_v2.json");
  console.log("\n账户Information:");
  console.log("  userC:", userC.address);
  console.log("  uTee:", uTee.address);
  console.log("  mTee:", mTee.address);
  console.log("\n代币Address:", tokenAddress);
  console.log("ContractAddress:", contractAddress);
}

main()
  .then(() => process.exit(0))
  .catch((error) => {
    console.error(error);
    process.exit(1);
  });
