// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.26;

import {IExecutor} from "@interfaces/IExecutor.sol";
import {TransferManager} from "../TransferManager.sol";

interface ICollateralRebalancerSwapper {
    struct StableForVolatileResult {
        uint256 netStableIn;
        uint256 stableRefund;
        uint256 volatileOut;
        uint256 collVaultSharesRedeemed;
        uint256 almSharesRedeemed;
        uint256 physicalStableOut;
        uint256 flashStable;
    }

    struct VolatileForStableResult {
        uint256 volatileIn;
        uint256 volatileRefund;
        uint256 netStableOut;
        uint256 grossStableOut;
        uint256 collVaultSharesMinted;
        uint256 almSharesUsed;
        uint256 physicalStableIn;
        uint256 flashStable;
    }

    function swapStableForVolatile(
        uint256 stableDebtIn,
        uint256 maxNetStableIn,
        uint256 minVolatileOut,
        address receiver
    ) external returns (StableForVolatileResult memory result);

    function swapVolatileForStable(
        uint256 collVaultSharesIn,
        uint256 maxStableIn,
        uint256 maxVolatileIn,
        uint256 minNetStableOut,
        address receiver
    ) external returns (VolatileForStableResult memory result);

    function previewTokenAmounts(uint256 collVaultShares, bool mint)
        external
        view
        returns (uint256 stableAmount, uint256 volatileAmount);

    function core() external view returns (address);
}

interface ICollateralRebalancer {
    struct ExchangeState {
        uint256 collVaultShares;
        uint256 debt;
        uint256 reservationValueWad;
        uint256 spreadPpm;
    }

    function exchangeState()
        external
        view
        returns (ExchangeState memory state);
}

/// @notice The deployed CR-math library the venue prices with. `deleverageQuote` is a
/// view, so a fill can evaluate the exact gross->shares map the swap itself will use.
interface ICollRebalancerMath {
    function deleverageQuote(
        uint256 collVaultShares,
        uint256 debt,
        uint256 reservationValueWad,
        uint256 leverageRatioWad,
        uint256 spreadPpm,
        uint256 stableIn
    )
        external
        view
        returns (uint256 collateralOut, uint256 newColl, uint256 newDebt);
}

error EverlongRebalancerExecutor__InvalidDataLength();
error EverlongRebalancerExecutor__NothingToFill();

/// @notice Executor for the Everlong CollateralRebalancer settlement venue
/// (CollateralRebalancerSwapper): NECT<->WBTC settled against a leveraged CDP
/// position priced by the CollateralRebalancer's CR bonding curve.
///
/// The swapper's arguments are share/gross-debt denominated while the executor
/// receives a runtime token `amountIn`, so each direction re-derives its
/// argument on-chain:
/// - LEVERAGE (volatile -> stable) inverts the exact share count by bisecting
///   the swapper's own monotone `previewTokenAmounts` (seeded by the
///   encode-time share hint to keep gas down);
/// - DELEVERAGE (stable -> volatile) passes the encode-time GROSS debt hint,
///   scaled down proportionally when the runtime amount is below the quoted
///   net; with no hints the gross falls back to `amountIn`, which always fits
///   (net <= gross) but forgoes the recycled-leg amplification.
///
/// Both directions pull their input cap up front and refund the unused part to
/// the router within the same call; the refund stays with the router exactly
/// like any partial fill. The stable leg of a leverage fill is flash-financed
/// by the swapper itself — only the volatile leg is debited from the router.
contract EverlongRebalancerExecutor is IExecutor {
    uint256 internal constant DATA_LENGTH = 177;
    /// @dev Cap on exact net evaluations when re-deriving the deleverage gross. A seeded
    /// step converges in one or two; the hint-free path starts at `budget` — low by the
    /// whole recycled leg — and small lots need the most, since each ratio step moves
    /// them proportionally less.
    uint256 internal constant NET_STEPS = 10;
    /// @dev Stop once the budget left unspent is within this fraction of it (0.01 bps).
    /// Measuring the SHORTFALL rather than the step size is what makes the exit an
    /// accuracy guarantee instead of a guess: a step can move very little and still be
    /// far from the boundary, which leaves small lots several bps short.
    uint256 internal constant NET_ACCURACY = 1e6;
    /// @dev Shares live at the CollVault's own scale; the venue's book is far
    /// below this.
    uint256 internal constant MAX_SHARES = type(uint128).max;

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
        (address swapper,,, bool isDeleverage, uint256 hintA, uint256 hintB) =
            _decodeData(data);

        if (isDeleverage) {
            _deleverage(
                ICollateralRebalancerSwapper(swapper),
                amountIn,
                hintA,
                hintB,
                data,
                receiver
            );
        } else {
            _leverage(
                ICollateralRebalancerSwapper(swapper), amountIn, hintA, receiver
            );
        }
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
        address swapper;
        (swapper, tokenIn, tokenOut,,,) = _decodeData(data);
        transferType = TransferManager.TransferType.ProtocolWillDebit;
        receiver = swapper;
        outputToRouter = false;
    }

    /// @dev stable -> volatile. `grossHint`/`netHint` are the quote-time gross debt and
    /// net stable spend, so when the runtime amount covers the quoted net the quote
    /// holds exactly. Below it the gross is re-derived against the venue's own math
    /// rather than rescaled: gross<->net bends with the bonding curve and the recycled
    /// stable leg, so a linear step both underfills and can overshoot into a revert.
    function _deleverage(
        ICollateralRebalancerSwapper swapper,
        uint256 amountIn,
        uint256 grossHint,
        uint256 netHint,
        bytes calldata data,
        address receiver
    ) internal {
        (ICollRebalancerMath math, uint256 leverageRatioWad) = _decodeMath(data);

        uint256 gross;
        if (grossHint != 0 && netHint != 0 && amountIn >= netHint) {
            // The quote holds only if the position has not moved since. It is
            // keeper-managed, so verify the hint still fits the budget rather than
            // assuming: a hint whose net has drifted above `amountIn` reverts inside the
            // swapper, killing the route, and exact-net funding has no headroom to
            // absorb it.
            gross = grossHint;
            if (
                _netFor(
                    swapper,
                    ICollateralRebalancer(swapper.core()).exchangeState(),
                    gross,
                    math,
                    leverageRatioWad
                ) > amountIn
            ) {
                gross = 0; // fall through to the exact re-derivation below
            }
        }
        if (gross == 0) {
            gross = _grossForNet(
                swapper, amountIn, grossHint, netHint, math, leverageRatioWad
            );
        }
        if (gross == 0) revert EverlongRebalancerExecutor__NothingToFill();

        // slither-disable-next-line unused-return
        swapper.swapStableForVolatile(gross, amountIn, 0, receiver);
    }

    /// @dev Largest gross debt whose NET stable spend still fits `budget`.
    /// `gross = budget` is always feasible (net <= gross), so it seeds the answer and the
    /// loop only ever improves on it — the call can never be sized into a revert. Each
    /// step evaluates the venue's own deleverageQuote composed with previewTokenAmounts,
    /// then takes the exact ratio step; `net` is monotone in `gross`, the same property
    /// the leverage bisection relies on.
    function _grossForNet(
        ICollateralRebalancerSwapper swapper,
        uint256 budget,
        uint256 grossHint,
        uint256 netHint,
        ICollRebalancerMath math,
        uint256 leverageRatioWad
    ) internal view returns (uint256) {
        ICollateralRebalancer.ExchangeState memory st =
            ICollateralRebalancer(swapper.core()).exchangeState();

        uint256 best = budget;
        uint256 cand = netHint == 0 || grossHint == 0
            ? budget
            : grossHint * budget / netHint;

        for (uint256 i; i < NET_STEPS; i++) {
            if (cand == 0) break;
            uint256 net = _netFor(swapper, st, cand, math, leverageRatioWad);
            if (net == 0) break;
            if (net <= budget) {
                if (cand > best) best = cand;
                if (budget - net <= budget / NET_ACCURACY) break;
            }
            uint256 next = cand * budget / net;
            if (next == cand) break;
            cand = next;
        }
        return best;
    }

    /// @dev Net stable the caller spends for `gross` debt retired: the gross minus the
    /// stable leg the released shares recycle.
    function _netFor(
        ICollateralRebalancerSwapper swapper,
        ICollateralRebalancer.ExchangeState memory st,
        uint256 gross,
        ICollRebalancerMath math,
        uint256 leverageRatioWad
    ) internal view returns (uint256) {
        (uint256 shares,,) = math.deleverageQuote(
            st.collVaultShares,
            st.debt,
            st.reservationValueWad,
            leverageRatioWad,
            st.spreadPpm,
            gross
        );
        if (shares == 0) return 0;
        (uint256 stableLeg,) = swapper.previewTokenAmounts(shares, false);
        return gross > stableLeg ? gross - stableLeg : 0;
    }

    function _leverage(
        ICollateralRebalancerSwapper swapper,
        uint256 amountIn,
        uint256 sharesHint,
        address receiver
    ) internal {
        uint256 shares = _sharesForVolatileIn(swapper, amountIn, sharesHint);
        if (shares == 0) revert EverlongRebalancerExecutor__NothingToFill();

        (uint256 stableRequired,) = swapper.previewTokenAmounts(shares, true);

        // slither-disable-next-line unused-return
        swapper.swapVolatileForStable(
            shares, stableRequired, amountIn, 0, receiver
        );
    }

    /// @dev Largest `shares` with previewTokenAmounts(shares, true).volatile
    /// <= amountIn. The hint (encode-time share count) usually lands within a
    /// halving of the answer, so the doubling bracket is short; without it the
    /// bracket grows from 1.
    function _sharesForVolatileIn(
        ICollateralRebalancerSwapper swapper,
        uint256 amountIn,
        uint256 sharesHint
    ) internal view returns (uint256) {
        uint256 lo;
        uint256 hi;
        if (
            sharesHint != 0 && sharesHint < MAX_SHARES
                && _volatileFor(swapper, sharesHint) <= amountIn
        ) {
            lo = sharesHint;
            hi = sharesHint << 1;
        } else {
            lo = 0;
            hi = sharesHint != 0 && sharesHint < MAX_SHARES ? sharesHint : 1;
        }
        while (_volatileFor(swapper, hi) <= amountIn) {
            lo = hi;
            hi <<= 1;
            if (hi > MAX_SHARES) {
                hi = MAX_SHARES;
                break;
            }
        }
        // invariant: volatileFor(lo) <= amountIn < volatileFor(hi), or hi hit
        // the cap
        while (hi - lo > 1) {
            uint256 mid = (lo + hi) >> 1;
            if (_volatileFor(swapper, mid) <= amountIn) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        return lo;
    }

    function _volatileFor(ICollateralRebalancerSwapper swapper, uint256 shares)
        internal
        view
        returns (uint256 volatileAmount)
    {
        (, volatileAmount) = swapper.previewTokenAmounts(shares, true);
    }

    function _decodeData(bytes calldata data)
        internal
        pure
        returns (
            address swapper,
            address tokenIn,
            address tokenOut,
            bool isDeleverage,
            uint256 hintA,
            uint256 hintB
        )
    {
        if (data.length != DATA_LENGTH) {
            revert EverlongRebalancerExecutor__InvalidDataLength();
        }
        swapper = address(bytes20(data[0:20]));
        tokenIn = address(bytes20(data[20:40]));
        tokenOut = address(bytes20(data[40:60]));
        isDeleverage = data[60] != 0;
        hintA = uint256(bytes32(data[61:93]));
        hintB = uint256(bytes32(data[93:125]));
    }

    /// @dev The CR-math library and the leverage ratio it validates against. Both are
    /// configuration carried on the component, not caller input: the library has no
    /// getter on the rebalancer, so a fill cannot resolve it on-chain.
    function _decodeMath(bytes calldata data)
        internal
        pure
        returns (ICollRebalancerMath math, uint256 leverageRatioWad)
    {
        math = ICollRebalancerMath(address(bytes20(data[125:145])));
        leverageRatioWad = uint256(bytes32(data[145:177]));
    }
}
