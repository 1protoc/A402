const hre = require("hardhat");

async function main() {
  console.log("Deploying PaymentChannel contract...");

  const PaymentChannel = await hre.ethers.getContractFactory("PaymentChannel");
  const paymentChannel = await PaymentChannel.deploy();

  await paymentChannel.waitForDeployment();

  const address = await paymentChannel.getAddress();
  console.log("PaymentChannel deployed to:", address);

  const fs = require("fs");
  const deploymentInfo = {
    address: address,
    network: hre.network.name,
    timestamp: new Date().toISOString()
  };
  
  fs.writeFileSync(
    "./deployment.json",
    JSON.stringify(deploymentInfo, null, 2)
  );
  
  console.log("Deployment info saved to deployment.json");
}

main()
  .then(() => process.exit(0))
  .catch((error) => {
    console.error(error);
    process.exit(1);
  });
