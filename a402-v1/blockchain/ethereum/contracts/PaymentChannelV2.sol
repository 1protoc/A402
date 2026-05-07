
pragma solidity ^0.8.20;


interface IERC20 {
    function transfer(address to, uint256 amount) external returns (bool);
    function transferFrom(address from, address to, uint256 amount) external returns (bool);
    function balanceOf(address account) external view returns (uint256);
    function allowance(address owner, address spender) external view returns (uint256);
}

contract PaymentChannelV2 {
    
    struct Channel {
        address userC;          
        address uTee;          
        address mTee;          
        address token;         
        uint256 totalAmount;   
        uint256 nonce;         
        bool isOpen;           
        uint256 challengePeriod; 
        uint256 challengeEndBlock; 
    }
    
    
    struct AssetState {
        uint256 userCAmount;   
        uint256 mTeeAmount;    
        uint256 nonce;         
        bytes uTeeSignature;  
        bytes adapterSignature; 
        bytes32 adapterPointT;  
    }
    
    
    mapping(bytes32 => Channel) public channels;
    
    
    mapping(bytes32 => bool) public usedAdapterPoints;  
    
    
    mapping(bytes32 => AssetState) public lastChallengeState;
    
    
    
    mapping(address => mapping(address => uint256)) public uTeeBalances;
    
    
    struct WithdrawRequest {
        address token;              
        uint256 amount;              
        uint256 nonce;               
        bytes uTeeSignature;         
        uint256 challengeEndBlock;   
    }
    
    
    mapping(address => WithdrawRequest) public withdrawRequests;
    
    
    mapping(address => mapping(address => uint256)) public userCNonces;
    
    
    event ChannelCreated(
        bytes32 indexed channelId, 
        address indexed userC, 
        address indexed uTee, 
        address mTee,
        address token,
        uint256 amount
    );
    
    event ChannelClosedByUTee(
        bytes32 indexed channelId,
        uint256 userCAmount,
        uint256 mTeeAmount
    );
    
    event ChannelClosedByMTee(
        bytes32 indexed channelId,
        uint256 userCAmount,
        uint256 mTeeAmount
    );
    
    event ChannelCloseRequestedByUser(
        bytes32 indexed channelId,
        uint256 challengeEndBlock
    );
    
    event ChallengeSubmitted(
        bytes32 indexed channelId,
        uint256 userCAmount,
        uint256 mTeeAmount
    );
    
    event ChannelClosedByUser(
        bytes32 indexed channelId,
        uint256 userCAmount,
        uint256 mTeeAmount
    );
    
    
    event Deposit(
        address indexed userC,
        address indexed uTee,
        address indexed token,
        uint256 amount
    );
    
    event WithdrawRequested(
        address indexed userC,
        address indexed uTee,
        address indexed token,
        uint256 amount,
        uint256 challengeEndBlock
    );
    
    event WithdrawCompleted(
        address indexed userC,
        address indexed uTee,
        address indexed token,
        uint256 amount
    );
    
    
    function createChannel(
        bytes32 channelId,
        address userC,
        address mTee,
        address token,
        uint256 amount,
        uint256 challengePeriod
    ) external {
        require(channels[channelId].userC == address(0), "Channel already exists");
        require(amount > 0, "Amount must be greater than 0");
        require(userC != address(0), "Invalid userC address");
        require(mTee != address(0), "Invalid mTee address");
        require(msg.sender != address(0), "Invalid uTee address");
        require(token != address(0), "Invalid token address");
        require(challengePeriod > 0, "Challenge period must be greater than 0");
        
        
        
        IERC20 tokenContract = IERC20(token);
        require(tokenContract.transferFrom(msg.sender, address(this), amount), "Token transfer failed");
        
        channels[channelId] = Channel({
            userC: userC,
            uTee: msg.sender,
            mTee: mTee,
            token: token,
            totalAmount: amount,
            nonce: 0,
            isOpen: true,
            challengePeriod: challengePeriod,
            challengeEndBlock: 0
        });
        
        emit ChannelCreated(channelId, userC, msg.sender, mTee, token, amount);
    }
    
    
    function closeChannelByUTee(
        bytes32 channelId,
        uint256 userCAmount,
        uint256 mTeeAmount
    ) external {
        Channel storage channel = channels[channelId];
        require(channel.isOpen, "Channel is not open");
        require(msg.sender == channel.uTee, "Only U-TEE can call this");
        require(userCAmount + mTeeAmount == channel.totalAmount, "Amount mismatch");
        require(userCAmount > 0 && mTeeAmount > 0, "Amounts must be positive");
        
        channel.isOpen = false;
        
        IERC20 tokenContract = IERC20(channel.token);
        
        
        if (userCAmount > 0) {
            require(tokenContract.transfer(channel.userC, userCAmount), "Transfer to userC failed");
        }
        
        if (mTeeAmount > 0) {
            require(tokenContract.transfer(channel.mTee, mTeeAmount), "Transfer to mTee failed");
        }
        
        emit ChannelClosedByUTee(channelId, userCAmount, mTeeAmount);
    }
    
    
    function closeChannelByMTee(
        bytes32 channelId,
        AssetState memory assetState
    ) external {
        Channel storage channel = channels[channelId];
        require(channel.isOpen, "Channel is not open");
        require(msg.sender == channel.mTee, "Only M-TEE can call this");
        require(assetState.userCAmount + assetState.mTeeAmount == channel.totalAmount, "Amount mismatch");
        require(assetState.nonce > channel.nonce, "Nonce must be greater than current nonce");
        
        
        require(assetState.adapterSignature.length >= 64, "Invalid adapter signature length");
        require(assetState.adapterPointT != bytes32(0), "Adapter point T required");
        require(verifyAdapterSignature(channelId, assetState, channel.mTee), "Invalid adapter signature");
        
        
        require(assetState.uTeeSignature.length >= 64, "Invalid U-TEE signature length");
        require(verifyUTeeSignature(channelId, assetState, channel.uTee), "Invalid U-TEE signature");
        
        
        require(!usedAdapterPoints[assetState.adapterPointT], "Adapter point already used");
        usedAdapterPoints[assetState.adapterPointT] = true;
        
        
        channel.nonce = assetState.nonce;
        channel.isOpen = false;
        
        IERC20 tokenContract = IERC20(channel.token);
        
        
        if (assetState.userCAmount > 0) {
            require(tokenContract.transfer(channel.userC, assetState.userCAmount), "Transfer to userC failed");
        }
        
        if (assetState.mTeeAmount > 0) {
            require(tokenContract.transfer(channel.mTee, assetState.mTeeAmount), "Transfer to mTee failed");
        }
        
        emit ChannelClosedByMTee(channelId, assetState.userCAmount, assetState.mTeeAmount);
    }
    
    
    function requestCloseChannelByUser(bytes32 channelId) external {
        Channel storage channel = channels[channelId];
        require(channel.isOpen, "Channel is not open");
        require(msg.sender == channel.userC, "Only userC can call this");
        require(channel.challengeEndBlock == 0, "Close already requested");
        
        channel.challengeEndBlock = block.number + channel.challengePeriod;
        
        emit ChannelCloseRequestedByUser(channelId, channel.challengeEndBlock);
    }
    
    
    function challengeByMTee(
        bytes32 channelId,
        AssetState memory assetState
    ) external {
        Channel storage channel = channels[channelId];
        require(channel.isOpen, "Channel is not open");
        require(msg.sender == channel.mTee, "Only M-TEE can call this");
        require(channel.challengeEndBlock > 0, "No close request pending");
        require(block.number < channel.challengeEndBlock, "Challenge period expired");
        require(assetState.userCAmount + assetState.mTeeAmount == channel.totalAmount, "Amount mismatch");
        require(assetState.nonce > channel.nonce, "Nonce must be greater than current nonce");
        
        
        require(assetState.adapterSignature.length >= 64, "Invalid adapter signature length");
        require(assetState.adapterPointT != bytes32(0), "Adapter point T required");
        require(assetState.uTeeSignature.length >= 64, "Invalid U-TEE signature length");
        require(verifyAdapterSignature(channelId, assetState, channel.mTee), "Invalid adapter signature");
        require(verifyUTeeSignature(channelId, assetState, channel.uTee), "Invalid U-TEE signature");
        
        
        require(!usedAdapterPoints[assetState.adapterPointT], "Adapter point already used");
        usedAdapterPoints[assetState.adapterPointT] = true;
        
        
        lastChallengeState[channelId] = assetState;
        
        
        channel.nonce = assetState.nonce;
        channel.challengeEndBlock = block.number + channel.challengePeriod; 
        
        emit ChallengeSubmitted(channelId, assetState.userCAmount, assetState.mTeeAmount);
    }
    
    
    function finalizeCloseByUser(bytes32 channelId) external {
        Channel storage channel = channels[channelId];
        require(channel.isOpen, "Channel is not open");
        require(msg.sender == channel.userC, "Only userC can call this");
        require(channel.challengeEndBlock > 0, "No close request pending");
        require(block.number >= channel.challengeEndBlock, "Challenge period not ended");
        
        channel.isOpen = false;
        
        
        
        AssetState memory lastState = lastChallengeState[channelId];
        
        uint256 userCAmount;
        uint256 mTeeAmount;
        
        if (lastState.nonce > 0 && lastState.userCAmount + lastState.mTeeAmount == channel.totalAmount) {
            
            userCAmount = lastState.userCAmount;
            mTeeAmount = lastState.mTeeAmount;
        } else {
            
            userCAmount = channel.totalAmount;
            mTeeAmount = 0;
        }
        
        IERC20 tokenContract = IERC20(channel.token);
        
        
        if (userCAmount > 0) {
            require(tokenContract.transfer(channel.userC, userCAmount), "Transfer to userC failed");
        }
        
        if (mTeeAmount > 0) {
            require(tokenContract.transfer(channel.mTee, mTeeAmount), "Transfer to mTee failed");
        }
        
        emit ChannelClosedByUser(channelId, userCAmount, mTeeAmount);
    }
    
    
    function getChannelInfo(bytes32 channelId) external view returns (
        address userC,
        address uTee,
        address mTee,
        address token,
        uint256 totalAmount,
        uint256 nonce,
        bool isOpen,
        uint256 challengeEndBlock
    ) {
        Channel storage channel = channels[channelId];
        return (
            channel.userC,
            channel.uTee,
            channel.mTee,
            channel.token,
            channel.totalAmount,
            channel.nonce,
            channel.isOpen,
            channel.challengeEndBlock
        );
    }
    
    
    function getContractBalance(address token) external view returns (uint256) {
        IERC20 tokenContract = IERC20(token);
        return tokenContract.balanceOf(address(this));
    }
    
    
    function deposit(
        address uTeeAddress,
        address token,
        uint256 amount
    ) external {
        require(token != address(0), "Invalid token address");
        require(amount > 0, "Amount must be greater than 0");
        
        
        IERC20 tokenContract = IERC20(token);
        require(tokenContract.transferFrom(msg.sender, address(this), amount), "Token transfer failed");
        
        
        uTeeBalances[msg.sender][token] += amount;
        
        emit Deposit(msg.sender, uTeeAddress, token, amount);
    }
    
    
    function withdraw(
        address uTeeAddress,
        address token,
        uint256 amount,
        uint256 nonce,
        bytes memory uTeeSignature,
        uint256 challengePeriod
    ) external {
        require(token != address(0), "Invalid token address");
        require(amount > 0, "Amount must be greater than 0");
        require(amount <= uTeeBalances[msg.sender][token], "Insufficient balance");
        require(uTeeSignature.length >= 64, "Invalid U-TEE signature length");
        require(challengePeriod > 0, "Challenge period must be greater than 0");
        
        
        uint256 remainingAmount = uTeeBalances[msg.sender][token] - amount;
        bytes memory message = abi.encodePacked(
            msg.sender,
            token,
            remainingAmount,
            amount,
            nonce
        );
        
        
        bytes32 messageHash = keccak256(message);
        bytes32 ethSignedMessageHash = keccak256(
            abi.encodePacked("\x19Ethereum Signed Message:\n32", messageHash)
        );
        
        
        bytes32 r;
        bytes32 s;
        uint8 v;
        
        assembly {
            r := mload(add(uTeeSignature, 32))
            s := mload(add(uTeeSignature, 64))
        }
        
        
        v = 27;
        address signer = ecrecover(ethSignedMessageHash, v, r, s);
        if (signer != uTeeAddress) {
            v = 28;
            signer = ecrecover(ethSignedMessageHash, v, r, s);
        }
        require(signer == uTeeAddress, "Invalid U-TEE signature");
        
        
        require(nonce > userCNonces[msg.sender][token], "Nonce must be greater than current nonce");
        
        
        withdrawRequests[msg.sender] = WithdrawRequest({
            token: token,
            amount: amount,
            nonce: nonce,
            uTeeSignature: uTeeSignature,
            challengeEndBlock: block.number + challengePeriod
        });
        
        
        userCNonces[msg.sender][token] = nonce;
        
        emit WithdrawRequested(msg.sender, uTeeAddress, token, amount, withdrawRequests[msg.sender].challengeEndBlock);
    }
    
    
    function withdrawByUTee(
        address userCAddress,
        address token,
        uint256 amount
    ) external {
        require(token != address(0), "Invalid token address");
        require(amount > 0, "Amount must be greater than 0");
        
        
        uint256 actualAmount = amount;
        if (amount > uTeeBalances[userCAddress][token]) {
            actualAmount = uTeeBalances[userCAddress][token];
        }
        
        if (actualAmount == 0) {
            return; 
        }
        
        
        uTeeBalances[userCAddress][token] -= actualAmount;
        
        
        IERC20 tokenContract = IERC20(token);
        require(tokenContract.transfer(userCAddress, actualAmount), "Transfer to userC failed");
        
        emit WithdrawCompleted(userCAddress, msg.sender, token, actualAmount);
    }
    
    
    function finalizeWithdraw() external {
        WithdrawRequest storage request = withdrawRequests[msg.sender];
        
        if (request.challengeEndBlock == 0) {
            return; 
        }
        
        if (block.number < request.challengeEndBlock) {
            return; 
        }
        
        address token = request.token;
        uint256 amount = request.amount;
        require(amount > 0, "Invalid withdraw amount");
        require(token != address(0), "Invalid token address");
        
        
        uint256 actualAmount = amount;
        if (amount > uTeeBalances[msg.sender][token]) {
            actualAmount = uTeeBalances[msg.sender][token];
        }
        
        if (actualAmount == 0) {
            delete withdrawRequests[msg.sender];
            return; 
        }
        
        
        delete withdrawRequests[msg.sender];
        
        
        uTeeBalances[msg.sender][token] -= actualAmount;
        
        
        IERC20 tokenContract = IERC20(token);
        require(tokenContract.transfer(msg.sender, actualAmount), "Transfer to userC failed");
        
        emit WithdrawCompleted(msg.sender, address(0), token, actualAmount);
    }
    
    
    function verifyAdapterSignature(
        bytes32 channelId,
        AssetState memory assetState,
        address mTeeAddress
    ) internal pure returns (bool) {
        
        bytes memory message = abi.encodePacked(
            channelId,
            assetState.userCAmount,
            assetState.mTeeAmount,
            assetState.nonce,
            assetState.adapterPointT
        );
        
        
        bytes32 messageHash = keccak256(message);
        bytes32 ethSignedMessageHash = keccak256(
            abi.encodePacked("\x19Ethereum Signed Message:\n32", messageHash)
        );
        
        
        
        if (assetState.adapterSignature.length < 64) {
            return false;
        }
        
        bytes32 r;
        bytes32 s;
        uint8 v;
        
        
        bytes memory sig = assetState.adapterSignature;
        assembly {
            r := mload(add(sig, 32))
            s := mload(add(sig, 64))
        }
        
        
        v = 27;
        address signer = ecrecover(ethSignedMessageHash, v, r, s);
        if (signer != mTeeAddress) {
            v = 28;
            signer = ecrecover(ethSignedMessageHash, v, r, s);
        }
        
        
        return signer == mTeeAddress;
    }
    
    
    function verifyUTeeSignature(
        bytes32 channelId,
        AssetState memory assetState,
        address uTeeAddress
    ) internal pure returns (bool) {
        
        
        bytes memory message = abi.encodePacked(
            channelId,
            assetState.userCAmount,
            assetState.mTeeAmount,
            assetState.nonce
        );
        
        
        bytes32 messageHash = keccak256(message);
        bytes32 ethSignedMessageHash = keccak256(
            abi.encodePacked("\x19Ethereum Signed Message:\n32", messageHash)
        );
        
        
        if (assetState.uTeeSignature.length < 64) {
            return false;
        }
        
        bytes32 r;
        bytes32 s;
        uint8 v;
        
        
        bytes memory sig = assetState.uTeeSignature;
        assembly {
            r := mload(add(sig, 32))
            s := mload(add(sig, 64))
        }
        
        
        v = 27;
        address signer = ecrecover(ethSignedMessageHash, v, r, s);
        if (signer != uTeeAddress) {
            v = 28;
            signer = ecrecover(ethSignedMessageHash, v, r, s);
        }
        
        
        return signer == uTeeAddress;
    }
}
