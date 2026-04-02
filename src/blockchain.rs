//! On-chain read logic + smart multi-source router.
//!
//! Routing strategy (all tried in parallel via futures::join_all):
//!   • V2 volatile direct
//!   • V2 stable direct
//!   • V3 CL — all known tick spacings
//!   • 2-hop via WETH  (V2 volatile-volatile)
//!   • 2-hop via USDC  (V2 volatile-volatile)
//! The candidate with the highest amountOut wins.
//!
//! Math uses U256 throughout — alloy_primitives::utils::format_units for display.
//! Zero JS floating-point involved.

use alloy_primitives::{
    utils::{format_units, parse_units},
    Address, U256,
};
use alloy_sol_types::{sol, SolCall};
use futures::future::join_all;
use serde_json::Value;

use crate::config::{CL_FACTORY, CL_QUOTER_V2, FALLBACK_RPCS, V2_FACTORY, V2_ROUTER, USDC, WETH};

// ─── ABI definitions ──────────────────────────────────────────────────────────

sol! {
    interface IERC20 {
        function balanceOf(address account) external view returns (uint256 balance);
        function allowance(address owner, address spender) external view returns (uint256 remaining);
        function approve(address spender, uint256 amount) external returns (bool success);
        function decimals() external view returns (uint8 decimals);
        function symbol() external view returns (string memory sym);
    }

    struct Route {
        address from;
        address to;
        bool stable;
        address factory;
    }

    interface IRouter {
        function getAmountsOut(
            uint256 amountIn,
            Route[] calldata routes
        ) external view returns (uint256[] memory amounts);

        function getAmountsIn(
            uint256 amountOut,
            Route[] calldata routes
        ) external view returns (uint256[] memory amounts);

        function swapExactTokensForTokens(
            uint256 amountIn,
            uint256 amountOutMin,
            Route[] calldata routes,
            address to,
            uint256 deadline
        ) external returns (uint256[] memory amounts);

        function swapExactETHForTokens(
            uint256 amountOutMin,
            Route[] calldata routes,
            address to,
            uint256 deadline
        ) external payable returns (uint256[] memory amounts);

        function swapTokensForExactTokens(
            uint256 amountOut,
            uint256 amountInMax,
            Route[] calldata routes,
            address to,
            uint256 deadline
        ) external returns (uint256[] memory amounts);

        function addLiquidity(
            address tokenA,
            address tokenB,
            bool stable,
            uint256 amountADesired,
            uint256 amountBDesired,
            uint256 amountAMin,
            uint256 amountBMin,
            address to,
            uint256 deadline
        ) external returns (uint256 amountA, uint256 amountB, uint256 liquidity);

        function addLiquidityETH(
            address token,
            bool stable,
            uint256 amountTokenDesired,
            uint256 amountTokenMin,
            uint256 amountETHMin,
            address to,
            uint256 deadline
        ) external payable returns (uint256 amountToken, uint256 amountETH, uint256 liquidity);

        function removeLiquidity(
            address tokenA,
            address tokenB,
            bool stable,
            uint256 liquidity,
            uint256 amountAMin,
            uint256 amountBMin,
            address to,
            uint256 deadline
        ) external returns (uint256 amountA, uint256 amountB);

        function removeLiquidityETH(
            address token,
            bool stable,
            uint256 liquidity,
            uint256 amountTokenMin,
            uint256 amountETHMin,
            address to,
            uint256 deadline
        ) external returns (uint256 amountToken, uint256 amountETH);

        function quoteAddLiquidity(
            address tokenA,
            address tokenB,
            bool stable,
            address _factory,
            uint256 amountADesired,
            uint256 amountBDesired
        ) external view returns (uint256 amountA, uint256 amountB, uint256 liquidity);

        function quoteRemoveLiquidity(
            address tokenA,
            address tokenB,
            bool stable,
            address _factory,
            uint256 liquidity
        ) external view returns (uint256 amountA, uint256 amountB);
    }

    interface IPoolFactory {
        function getPool(address tokenA, address tokenB, bool stable) external view returns (address pool);
        function allPools(uint256 index) external view returns (address pool);
        function allPoolsLength() external view returns (uint256 len);
    }

    interface IPool {
        function getReserves() external view
            returns (uint256 reserve0, uint256 reserve1, uint256 blockTimestampLast);
        function token0() external view returns (address t0);
        function token1() external view returns (address t1);
        function stable() external view returns (bool isStable);
        function totalSupply() external view returns (uint256 supply);
        function balanceOf(address account) external view returns (uint256 balance);
    }

    struct ExactInputSingleParams {
        address tokenIn;
        address tokenOut;
        int24 tickSpacing;
        address recipient;
        uint256 deadline;
        uint256 amountIn;
        uint256 amountOutMinimum;
        uint160 sqrtPriceLimitX96;
    }

    struct ExactOutputSingleParams {
        address tokenIn;
        address tokenOut;
        int24 tickSpacing;
        address recipient;
        uint256 deadline;
        uint256 amountOut;
        uint256 amountInMaximum;
        uint160 sqrtPriceLimitX96;
    }

    interface ISwapRouter {
        function exactInputSingle(ExactInputSingleParams calldata params)
            external payable returns (uint256 amountOut);
        function exactOutputSingle(ExactOutputSingleParams calldata params)
            external payable returns (uint256 amountIn);
        function multicall(bytes[] calldata data)
            external payable returns (bytes[] memory results);
        function unwrapWETH9(uint256 amountMinimum, address recipient) external payable;
    }

    interface ICLFactory {
        function getPool(address tokenA, address tokenB, int24 tickSpacing)
            external view returns (address pool);
    }

    struct QuoteExactInputSingleParams {
        address tokenIn;
        address tokenOut;
        uint256 amountIn;
        int24   tickSpacing;
        uint160 sqrtPriceLimitX96;
    }

    interface IQuoterV2 {
        function quoteExactInputSingle(QuoteExactInputSingleParams calldata params)
            external returns (
                uint256 amountOut,
                uint160 sqrtPriceX96After,
                uint32  gasEstimate,
                int24   tickAfter
            );
    }
}

// ─── JSON-RPC transport ───────────────────────────────────────────────────────

pub async fn eth_call(to: Address, calldata: &[u8]) -> Result<Vec<u8>, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_call",
        "params": [{ "to": format!("{to:#x}"), "data": format!("0x{}", hex::encode(calldata)) }, "latest"],
        "id": 1
    });
    let mut last = String::from("no RPC");
    for rpc in FALLBACK_RPCS {
        match try_rpc_call(rpc, &body).await {
            Ok(b)  => return Ok(b),
            Err(e) => last = e,
        }
    }
    Err(format!("All RPCs failed: {last}"))
}

async fn try_rpc_call(url: &str, body: &Value) -> Result<Vec<u8>, String> {
    use gloo_net::http::Request;
    let resp = Request::post(url)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(body).unwrap())
        .map_err(|e| e.to_string())?
        .send().await
        .map_err(|e| e.to_string())?;
    let json: Value = resp.json().await.map_err(|e| e.to_string())?;
    if let Some(e) = json.get("error") { return Err(e.to_string()); }
    let hex_str = json["result"].as_str().ok_or("missing result")?.trim_start_matches("0x");
    hex::decode(hex_str).map_err(|e| e.to_string())
}

// ─── ERC-20 / balance helpers ─────────────────────────────────────────────────

// Alloy 1.0: abi_decode_returns takes no validate bool; single-return fns → bare type.
pub async fn get_erc20_balance(token: Address, account: Address) -> Result<U256, String> {
    let raw = eth_call(token, &IERC20::balanceOfCall { account }.abi_encode()).await?;
    IERC20::balanceOfCall::abi_decode_returns(&raw).map_err(|e| e.to_string())
}

pub async fn get_erc20_allowance(token: Address, owner: Address, spender: Address) -> Result<U256, String> {
    let raw = eth_call(token, &IERC20::allowanceCall { owner, spender }.abi_encode()).await?;
    IERC20::allowanceCall::abi_decode_returns(&raw).map_err(|e| e.to_string())
}

pub async fn get_eth_balance(account: Address) -> Result<U256, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0", "method": "eth_getBalance",
        "params": [format!("{account:#x}"), "latest"], "id": 1
    });
    for rpc in FALLBACK_RPCS {
        if let Ok(req) = gloo_net::http::Request::post(rpc)
            .header("Content-Type", "application/json")
            .body(serde_json::to_string(&body).unwrap())
        {
            if let Ok(resp) = req.send().await {
                if let Ok(json) = resp.json::<Value>().await {
                    let hex = json["result"].as_str().unwrap_or("0x0").trim_start_matches("0x");
                    if let Ok(v) = U256::from_str_radix(hex, 16) { return Ok(v); }
                }
            }
        }
    }
    Err("eth_getBalance failed".into())
}

// ─── Smart Router ─────────────────────────────────────────────────────────────

/// Which source provided this quote.
#[derive(Clone, Debug, PartialEq)]
pub enum RouteKind {
    V2Volatile,
    V2Stable,
    V3 { tick_spacing: i32 },
    MultiHopWeth,
    MultiHopUsdc,
}

impl RouteKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::V2Volatile     => "V2 · Volatile AMM",
            Self::V2Stable       => "V2 · Stable AMM",
            Self::V3 { tick_spacing: 1    } => "V3 · 0.01% fee",
            Self::V3 { tick_spacing: 10   } => "V3 · 0.05% fee",
            Self::V3 { tick_spacing: 50   } => "V3 · 0.05% fee",
            Self::V3 { tick_spacing: 100  } => "V3 · 0.3% fee",
            Self::V3 { tick_spacing: 200  } => "V3 · 0.3% fee",
            Self::V3 { tick_spacing: 500  } => "V3 · 1% fee",
            Self::V3 { tick_spacing: 1000 } => "V3 · 1% fee",
            Self::V3 { tick_spacing: 2000 } => "V3 · 2% fee",
            Self::V3 { .. }                 => "V3 · CL",
            Self::MultiHopWeth              => "2-hop · via WETH",
            Self::MultiHopUsdc              => "2-hop · via USDC",
        }
    }
    /// Short badge text for inline display.
    pub fn badge(&self) -> &'static str {
        match self {
            Self::V2Volatile  => "V2",
            Self::V2Stable    => "V2S",
            Self::V3 { .. }   => "V3",
            Self::MultiHopWeth | Self::MultiHopUsdc => "2-hop",
        }
    }
}

/// Full result from the smart router.
#[derive(Clone, Debug)]
pub struct SmartRoute {
    pub kind:             RouteKind,
    pub amount_out:       U256,
    pub amount_out_fmt:   String,
    pub min_received:     U256,
    pub min_received_fmt: String,
    /// Basis-point integer, e.g. 42 = 0.42 %
    pub price_impact_bps: u64,
    pub gas_estimate:     u32,
    /// e.g. ["ETH", "→", "USDC"] or ["ETH", "→", "WETH", "→", "WIND"]
    pub path_labels:      Vec<&'static str>,
}

impl SmartRoute {
    pub fn price_impact_pct(&self) -> f64 {
        self.price_impact_bps as f64 / 100.0
    }

    /// Traffic-light colour class for price impact.
    pub fn impact_color(&self) -> &'static str {
        match self.price_impact_bps {
            0..=99          => "text-green-400",
            100..=499       => "text-yellow-400",
            _               => "text-red-400",
        }
    }

    pub fn impact_bg(&self) -> &'static str {
        match self.price_impact_bps {
            0..=99    => "bg-green-900/40 text-green-300",
            100..=499 => "bg-yellow-900/40 text-yellow-300",
            _         => "bg-red-900/40 text-red-300",
        }
    }
}

/// Main entry-point: find the best route for `amount_in` of `token_in` → `token_out`.
/// Tries all sources **in parallel** (browser fetch API is concurrent even in WASM).
pub async fn find_best_route(
    token_in:     Address,
    token_out:    Address,
    amount_in:    U256,
    slippage_pct: f64,
    in_sym:       &'static str,
    out_sym:      &'static str,
    out_dec:      u8,
) -> Result<SmartRoute, String> {
    // Normalise ETH → WETH for on-chain routing
    let in_addr  = if token_in  == Address::ZERO { WETH } else { token_in };
    let out_addr = if token_out == Address::ZERO { WETH } else { token_out };

    // ── Build all candidate futures ───────────────────────────────────────────
    // WASM futures are not Send — use LocalBoxFuture (no Send bound)
    use futures::future::LocalBoxFuture;
    use futures::FutureExt;
    type BoxFut = LocalBoxFuture<'static, Option<(U256, RouteKind, u32)>>;

    let mut futs: Vec<BoxFut> = vec![
        async move { try_v2_direct(in_addr, out_addr, amount_in, false).await.map(|o| (o, RouteKind::V2Volatile, 120_000u32)) }.boxed_local(),
        async move { try_v2_direct(in_addr, out_addr, amount_in, true ).await.map(|o| (o, RouteKind::V2Stable,   120_000u32)) }.boxed_local(),
    ];

    // V3 all tick spacings
    for &ts in crate::config::CL_TICK_SPACINGS {
        futs.push(async move {
            try_v3_single(in_addr, out_addr, amount_in, ts).await
                .map(|(o, gas)| (o, RouteKind::V3 { tick_spacing: ts }, gas))
        }.boxed_local());
    }

    // Multi-hop via WETH
    if in_addr != WETH && out_addr != WETH {
        futs.push(async move {
            try_v2_multihop(in_addr, WETH, out_addr, amount_in).await
                .map(|o| (o, RouteKind::MultiHopWeth, 180_000u32))
        }.boxed_local());
    }

    // Multi-hop via USDC
    if in_addr != USDC && out_addr != USDC {
        futs.push(async move {
            try_v2_multihop(in_addr, USDC, out_addr, amount_in).await
                .map(|o| (o, RouteKind::MultiHopUsdc, 180_000u32))
        }.boxed_local());
    }

    // ── Run all in parallel ───────────────────────────────────────────────────
    let results = join_all(futs).await;
    let (best_out, best_kind, gas) = results
        .into_iter()
        .flatten()
        .max_by_key(|(out, _, _)| *out)
        .ok_or("No liquidity found for this pair")?;

    // ── Compute price impact (U256 arithmetic, no floats) ─────────────────────
    let impact_bps = compute_price_impact_bps(in_addr, out_addr, amount_in, best_out).await;

    // ── Format outputs ────────────────────────────────────────────────────────
    let amount_out_fmt   = fmt_units(best_out, out_dec);
    let min_received     = apply_slippage_min(best_out, slippage_pct);
    let min_received_fmt = fmt_units(min_received, out_dec);

    // ── Build path labels ─────────────────────────────────────────────────────
    let path_labels = match &best_kind {
        RouteKind::MultiHopWeth => vec![in_sym, "→", "WETH", "→", out_sym],
        RouteKind::MultiHopUsdc => vec![in_sym, "→", "USDC", "→", out_sym],
        _                       => vec![in_sym, "→", out_sym],
    };

    Ok(SmartRoute {
        kind:             best_kind,
        amount_out:       best_out,
        amount_out_fmt,
        min_received,
        min_received_fmt,
        price_impact_bps: impact_bps,
        gas_estimate:     gas,
        path_labels,
    })
}

// ─── Internal routing helpers ─────────────────────────────────────────────────

async fn try_v2_direct(
    in_addr:  Address,
    out_addr: Address,
    amount_in: U256,
    stable:   bool,
) -> Option<U256> {
    let call = IRouter::getAmountsOutCall {
        amountIn: amount_in,
        routes:   vec![Route { from: in_addr, to: out_addr, stable, factory: V2_FACTORY }],
    };
    let raw = eth_call(V2_ROUTER, &call.abi_encode()).await.ok()?;
    // Alloy 1.0: getAmountsOut returns Vec<U256> directly
    let amounts: Vec<U256> = IRouter::getAmountsOutCall::abi_decode_returns(&raw).ok()?;
    let out = *amounts.last()?;
    if out.is_zero() { None } else { Some(out) }
}

async fn try_v3_single(
    in_addr:      Address,
    out_addr:     Address,
    amount_in:    U256,
    tick_spacing: i32,
) -> Option<(U256, u32)> {
    let ts24 = alloy_primitives::Signed::<24, 1>::try_from(tick_spacing as i128).ok()?;
    let params = QuoteExactInputSingleParams {
        tokenIn:           in_addr,
        tokenOut:          out_addr,
        amountIn:          amount_in,
        tickSpacing:       ts24,
        sqrtPriceLimitX96: alloy_primitives::Uint::ZERO,
    };
    let raw = eth_call(CL_QUOTER_V2, &IQuoterV2::quoteExactInputSingleCall { params }.abi_encode()).await.ok()?;
    // Alloy 1.0: multi-return → tuple (amountOut, sqrtPriceX96After, gasEstimate, tickAfter)
    let (amount_out, _, gas_estimate, _): (U256, alloy_primitives::Uint<160, 3>, u32, alloy_primitives::Signed<24, 1>)
        = IQuoterV2::quoteExactInputSingleCall::abi_decode_returns(&raw).ok()?.into();
    if amount_out.is_zero() { None } else { Some((amount_out, gas_estimate)) }
}

async fn try_v2_multihop(
    in_addr:   Address,
    via:       Address,
    out_addr:  Address,
    amount_in: U256,
) -> Option<U256> {
    let call = IRouter::getAmountsOutCall {
        amountIn: amount_in,
        routes:   vec![
            Route { from: in_addr, to: via,      stable: false, factory: V2_FACTORY },
            Route { from: via,     to: out_addr, stable: false, factory: V2_FACTORY },
        ],
    };
    let raw    = eth_call(V2_ROUTER, &call.abi_encode()).await.ok()?;
    let amounts: Vec<U256> = IRouter::getAmountsOutCall::abi_decode_returns(&raw).ok()?;
    let out    = *amounts.last()?;
    if out.is_zero() { None } else { Some(out) }
}

// ─── Price impact (pure U256 math) ───────────────────────────────────────────

/// Returns price impact in basis-points (u64). 100 bps = 1%.
/// Formula: idealOut = amountIn * reserveOut / reserveIn
///          impact   = (idealOut - actualOut) / idealOut * 10_000
async fn compute_price_impact_bps(
    in_addr:    Address,
    out_addr:   Address,
    amount_in:  U256,
    amount_out: U256,
) -> u64 {
    // Try to get V2 pool reserves for the direct pair
    if let Some((r0, r1, is_token0)) = get_v2_reserves_ordered(in_addr, out_addr).await {
        let (reserve_in, reserve_out) = if is_token0 { (r0, r1) } else { (r1, r0) };
        if !reserve_in.is_zero() && !reserve_out.is_zero() {
            // idealOut = amountIn * reserveOut / reserveIn  (ignores fee)
            let ideal_out = amount_in.saturating_mul(reserve_out) / reserve_in;
            if ideal_out > amount_out {
                let diff   = ideal_out - amount_out;
                let bps = diff.saturating_mul(U256::from(10_000u64)) / ideal_out;
                // Alloy 1.0: use saturating_to or clamp to u64::MAX
                let capped = bps.min(U256::from(u64::MAX));
                return capped.to::<u64>();
            }
        }
    }
    0
}

async fn get_v2_reserves_ordered(
    token_a: Address,
    token_b: Address,
) -> Option<(U256, U256, bool)> {
    // Pool address — Alloy 1.0: single return → bare Address
    let call = IPoolFactory::getPoolCall { tokenA: token_a, tokenB: token_b, stable: false };
    let raw  = eth_call(crate::config::V2_FACTORY, &call.abi_encode()).await.ok()?;
    let pool: Address = IPoolFactory::getPoolCall::abi_decode_returns(&raw).ok()?;
    if pool == Address::ZERO { return None; }

    // token0 ordering
    let t0_raw  = eth_call(pool, &IPool::token0Call {}.abi_encode()).await.ok()?;
    let token0: Address = IPool::token0Call::abi_decode_returns(&t0_raw).ok()?;
    let is_t0   = token_a == token0;

    // Reserves — multi-return → tuple (reserve0, reserve1, blockTimestampLast)
    let res_raw = eth_call(pool, &IPool::getReservesCall {}.abi_encode()).await.ok()?;
    let (reserve0, reserve1, _): (U256, U256, U256) =
        IPool::getReservesCall::abi_decode_returns(&res_raw).ok()?.into();
    Some((reserve0, reserve1, is_t0))
}

// ─── USD price (WETH/USDC pool) ───────────────────────────────────────────────

/// Approximate ETH price in USD from the WETH/USDC V2 pool.
pub async fn get_eth_usd_price() -> Option<f64> {
    // 1 WETH worth in USDC (USDC has 6 decimals)
    let one_eth = U256::from(10u64).pow(U256::from(18u64));
    let out = try_v2_direct(WETH, USDC, one_eth, false).await?;
    // out is USDC (6 decimals). Cap at u64::MAX before cast.
    let out_capped = out.min(U256::from(u64::MAX));
    let price = out_capped.to::<u64>() as f64 / 1_000_000.0;
    if price > 1.0 { Some(price) } else { None }
}

/// Format a token amount as a USD string if we can derive the price.
pub async fn token_usd_value(
    amount:     U256,
    token_addr: Address,
    decimals:   u8,
) -> Option<String> {
    if amount.is_zero() { return None; }

    // USDC/USDT → value = amount / 1e6
    if token_addr == USDC {
        let v = format_units(amount, 6u8).ok()?;
        return Some(format!("${}", trim_decimals(&v, 2)));
    }

    let eth_price = get_eth_usd_price().await?;

    // WETH/ETH → straightforward
    if token_addr == WETH || token_addr == Address::ZERO {
        let eth_amt: f64 = format_units(amount, decimals).ok()?.parse().ok()?;
        return Some(format!("${:.2}", eth_amt * eth_price));
    }

    // Other tokens: quote token → WETH, then multiply by eth_price
    let weth_out = try_v2_direct(token_addr, WETH, amount, false).await?;
    let weth_f: f64 = format_units(weth_out, 18u8).ok()?.parse().ok()?;
    let usd = weth_f * eth_price;
    if usd < 0.001 { return None; }
    Some(format!("${:.2}", usd))
}

fn trim_decimals(s: &str, places: usize) -> String {
    match s.find('.') {
        None    => s.to_string(),
        Some(i) => {
            let end = (i + 1 + places).min(s.len());
            s[..end].trim_end_matches('0').trim_end_matches('.').to_string()
        }
    }
}

// ─── Exchange rate string ─────────────────────────────────────────────────────

/// "1 ETH = 2,161.05 USDC" from a SmartRoute.
pub fn format_rate(
    amount_in:   U256,
    amount_out:  U256,
    in_sym:      &str,
    out_sym:     &str,
    in_dec:      u8,
    out_dec:     u8,
    inverted:    bool,
) -> String {
    if amount_in.is_zero() || amount_out.is_zero() {
        return format!("1 {in_sym} = — {out_sym}");
    }
    if inverted {
        // 1 out_sym = X in_sym
        let rate = amount_in.saturating_mul(U256::from(10u64).pow(U256::from(out_dec as u64)))
            / amount_out;
        let rate_str = fmt_units(rate, in_dec);
        format!("1 {out_sym} = {} {in_sym}", trim_decimals(&rate_str, 6))
    } else {
        // 1 in_sym = X out_sym
        let rate = amount_out.saturating_mul(U256::from(10u64).pow(U256::from(in_dec as u64)))
            / amount_in;
        let rate_str = fmt_units(rate, out_dec);
        format!("1 {in_sym} = {} {out_sym}", trim_decimals(&rate_str, 6))
    }
}

// ─── Formatting helpers ───────────────────────────────────────────────────────

pub fn fmt_units(raw: U256, decimals: u8) -> String {
    format_units(raw, decimals).unwrap_or_else(|_| "—".to_string())
}

pub fn parse_token_amount(s: &str, decimals: u8) -> U256 {
    parse_units(s, decimals).map(|v| v.into()).unwrap_or(U256::ZERO)
}

pub fn apply_slippage_min(amount: U256, slippage_pct: f64) -> U256 {
    let bps = (slippage_pct * 100.0) as u64;
    amount * U256::from(10_000u64 - bps) / U256::from(10_000u64)
}

pub fn deadline_secs(minutes: u64) -> u64 {
    (js_sys::Date::now() as u64) / 1000 + minutes * 60
}

pub async fn get_erc20_balance_fmt(token: Address, account: Address, decimals: u8) -> String {
    get_erc20_balance(token, account).await
        .map(|v| fmt_units(v, decimals))
        .unwrap_or_default()
}

// ─── Pool helpers ─────────────────────────────────────────────────────────────

pub async fn get_pool_address(token_a: Address, token_b: Address, stable: bool) -> Option<Address> {
    let call = IPoolFactory::getPoolCall { tokenA: token_a, tokenB: token_b, stable };
    let raw  = eth_call(V2_FACTORY, &call.abi_encode()).await.ok()?;
    let pool: Address = IPoolFactory::getPoolCall::abi_decode_returns(&raw).ok()?;
    if pool == Address::ZERO { None } else { Some(pool) }
}

pub async fn get_lp_balance(pool: Address, account: Address) -> U256 {
    let call = IPool::balanceOfCall { account };
    let raw  = eth_call(pool, &call.abi_encode()).await.unwrap_or_default();
    IPool::balanceOfCall::abi_decode_returns(&raw).unwrap_or(U256::ZERO)
}

pub async fn get_pool_total_supply(pool: Address) -> U256 {
    let raw = eth_call(pool, &IPool::totalSupplyCall {}.abi_encode()).await.unwrap_or_default();
    IPool::totalSupplyCall::abi_decode_returns(&raw).unwrap_or(U256::ZERO)
}

pub async fn get_pool_reserves(pool: Address) -> Option<(U256, U256)> {
    let raw = eth_call(pool, &IPool::getReservesCall {}.abi_encode()).await.ok()?;
    let (r0, r1, _): (U256, U256, U256) = IPool::getReservesCall::abi_decode_returns(&raw).ok()?.into();
    Some((r0, r1))
}

pub async fn get_pool_tokens(pool: Address) -> Option<(Address, Address)> {
    let r0 = eth_call(pool, &IPool::token0Call {}.abi_encode()).await.ok()?;
    let r1 = eth_call(pool, &IPool::token1Call {}.abi_encode()).await.ok()?;
    let t0: Address = IPool::token0Call::abi_decode_returns(&r0).ok()?;
    let t1: Address = IPool::token1Call::abi_decode_returns(&r1).ok()?;
    Some((t0, t1))
}

/// Returns (amountA, amountB, liquidity) — the amounts that will actually be deposited.
pub async fn quote_add_liquidity(
    token_a: Address,
    token_b: Address,
    stable:  bool,
    amount_a: U256,
    amount_b: U256,
) -> Option<(U256, U256, U256)> {
    let call = IRouter::quoteAddLiquidityCall {
        tokenA:         token_a,
        tokenB:         token_b,
        stable,
        _factory:       V2_FACTORY,
        amountADesired: amount_a,
        amountBDesired: amount_b,
    };
    let raw = eth_call(V2_ROUTER, &call.abi_encode()).await.ok()?;
    let (a, b, lp): (U256, U256, U256) =
        IRouter::quoteAddLiquidityCall::abi_decode_returns(&raw).ok()?.into();
    Some((a, b, lp))
}

/// Returns (amountA, amountB) you'd receive burning `liquidity` LP tokens.
pub async fn quote_remove_liquidity(
    token_a:   Address,
    token_b:   Address,
    stable:    bool,
    liquidity: U256,
) -> Option<(U256, U256)> {
    let call = IRouter::quoteRemoveLiquidityCall {
        tokenA:    token_a,
        tokenB:    token_b,
        stable,
        _factory:  V2_FACTORY,
        liquidity,
    };
    let raw = eth_call(V2_ROUTER, &call.abi_encode()).await.ok()?;
    let (a, b): (U256, U256) =
        IRouter::quoteRemoveLiquidityCall::abi_decode_returns(&raw).ok()?.into();
    Some((a, b))
}

/// Information about a pool the user has a position in.
#[derive(Clone, Debug)]
pub struct PoolPosition {
    pub pool:        Address,
    pub token_a_sym: &'static str,
    pub token_b_sym: &'static str,
    pub stable:      bool,
    pub lp_balance:  U256,
    pub lp_fmt:      String,
    pub share_pct:   f64,
}

/// Scan known token pairs for LP positions belonging to `account`.
pub async fn fetch_user_positions(account: Address) -> Vec<PoolPosition> {
    use crate::config::TOKENS;
    use futures::future::LocalBoxFuture;
    use futures::FutureExt;

    let tokens: Vec<_> = TOKENS.iter()
        .filter(|t| !t.is_native)   // skip ETH pseudo-token
        .collect();

    let mut futs: Vec<LocalBoxFuture<'static, Option<PoolPosition>>> = vec![];

    for i in 0..tokens.len() {
        for j in (i + 1)..tokens.len() {
            let ta = tokens[i];
            let tb = tokens[j];
            let ta_addr = ta.address;
            let tb_addr = tb.address;
            let ta_sym  = ta.symbol;
            let tb_sym  = tb.symbol;

            for &stable in &[false, true] {
                futs.push(async move {
                    let pool = get_pool_address(ta_addr, tb_addr, stable).await?;
                    let lp   = get_lp_balance(pool, account).await;
                    if lp.is_zero() { return None; }
                    let supply = get_pool_total_supply(pool).await;
                    let share_pct = if supply.is_zero() { 0.0 } else {
                        let s_capped = supply.min(U256::from(u128::MAX));
                        let l_capped = lp.min(U256::from(u128::MAX));
                        (l_capped.to::<u128>() as f64 / s_capped.to::<u128>() as f64) * 100.0
                    };
                    let lp_fmt = fmt_units(lp, 18);
                    Some(PoolPosition { pool, token_a_sym: ta_sym, token_b_sym: tb_sym, stable, lp_balance: lp, lp_fmt, share_pct })
                }.boxed_local());
            }
        }
    }

    join_all(futs).await.into_iter().flatten().collect()
}
