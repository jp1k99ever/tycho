// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.26;

import {
    SafeERC20,
    IERC20
} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {IExecutor} from "@interfaces/IExecutor.sol";
import {TransferManager} from "../TransferManager.sol";

interface ICvammALM {
    /// @param stableIn True to sell the stable leg (token0).
    /// @param amountIn MAXIMUM input; the venue pulls only what the fill uses.
    /// @param minAmountOut Slippage floor on the output actually delivered.
    /// @param sqrtPriceLimitX96 Pool-frame bound; 0 means no limit.
    function swap(
        bool stableIn,
        uint256 amountIn,
        uint256 minAmountOut,
        uint160 sqrtPriceLimitX96,
        address to,
        uint256 deadline
    ) external returns (uint256 amountInUsed, uint256 amountOut);
}

error EverlongCvammExecutor__InvalidDataLength();

/// @notice Executor for the Everlong CVAMM venue: exact-input swaps directly
/// against the CvammALM (the ALM is the pool, the swap entrypoint and the
/// approval target at once — it pulls the input with transferFrom and settles
/// both legs in plain ERC-20). Partial fills are normal: the venue treats
/// `amountIn` as a maximum and pulls only `amountInUsed`; any unused input
/// simply stays with the router. The router's slippage floor guards the
/// output, so `minAmountOut` is 0 here.
contract EverlongCvammExecutor is IExecutor {
    using SafeERC20 for IERC20;

    uint256 internal constant DATA_LENGTH = 61;

    function fundsExpectedAddress(bytes calldata /* data */ )
        external
        view
        returns (address receiver)
    {
        return msg.sender;
    }

    // slither-disable-next-line locked-ether
    function swap(uint256 amountIn, bytes calldata data, address receiver)
        external
        payable
    {
        (address alm, address tokenIn,, bool stableIn) = _decodeData(data);

        // slither-disable-next-line unused-return
        ICvammALM(alm).swap(stableIn, amountIn, 0, 0, receiver, block.timestamp);

        // The Dispatcher approved the full `amountIn` before this call, and the ALM
        // pulls only what the fill used — so a partial fill, which is the normal case
        // here, leaves a standing allowance exactly equal to the stranded remainder.
        // Clear it rather than leave an upgradeable proxy able to take it later.
        IERC20(tokenIn).forceApprove(alm, 0);
    }

    function getTransferData(bytes calldata data)
        external
        pure
        returns (
            TransferManager.TransferType transferType,
            address receiver,
            address tokenIn,
            address tokenOut,
            bool outputToRouter
        )
    {
        address alm;
        (alm, tokenIn, tokenOut,) = _decodeData(data);
        transferType = TransferManager.TransferType.ProtocolWillDebit;
        receiver = alm;
        outputToRouter = false;
    }

    function _decodeData(bytes calldata data)
        internal
        pure
        returns (address alm, address tokenIn, address tokenOut, bool stableIn)
    {
        if (data.length != DATA_LENGTH) {
            revert EverlongCvammExecutor__InvalidDataLength();
        }
        alm = address(bytes20(data[0:20]));
        tokenIn = address(bytes20(data[20:40]));
        tokenOut = address(bytes20(data[40:60]));
        stableIn = data[60] != 0;
    }
}
