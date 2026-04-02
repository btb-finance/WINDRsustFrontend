//! Subgraph service for WindSwap protocol data.
//! Uses The Graph gateway to fetch pools, gauges, veNFTs, votes, and bribes.

use alloy_primitives::U256;
use serde::Deserialize;
use gloo_net::http::Request;

const SUBGRAPH_URL: &str =
    "https://gateway.thegraph.com/api/subgraphs/id/4xkN7MDbfzm1p4MfHCSzyhNTpjYk4URUnA5ysyKRfij";
const API_KEY: &str = "d65849208eee868786c36b6acb3b1987";

// ─── Raw JSON helpers ─────────────────────────────────────────────────────────

fn u256_from_str(s: &str) -> U256 {
    if s.is_empty() {
        return U256::ZERO;
    }
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        U256::from_str_radix(hex, 16).unwrap_or(U256::ZERO)
    } else {
        // BigDecimal from subgraph — strip decimal point, parse integer part
        let clean: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
        if clean.is_empty() {
            U256::ZERO
        } else {
            U256::from_str_radix(&clean, 10).unwrap_or(U256::ZERO)
        }
    }
}

async fn query<T: serde::de::DeserializeOwned>(gql: &str) -> Result<T, String> {
    let body = serde_json::json!({ "query": gql });
    let resp = Request::post(SUBGRAPH_URL)
        .header("Content-Type", "application/json")
        .header("Authorization", &format!("Bearer {API_KEY}"))
        .body(serde_json::to_string(&body).unwrap())
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| format!("Subgraph request failed: {e}"))?;

    let val: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    if let Some(errs) = val.get("errors") {
        return Err(format!("Subgraph errors: {errs}"));
    }
    let data = val.get("data").ok_or("No data in subgraph response")?;
    serde_json::from_value(data.clone()).map_err(|e| format!("Deserialize: {e}"))
}

// ─── Protocol ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
pub struct ProtocolInfo {
    pub weekly_emissions: Option<f64>,
    pub active_period: Option<u64>,
    pub epoch_count: Option<u64>,
}

pub async fn fetch_protocol() -> Result<ProtocolInfo, String> {
    #[derive(Deserialize)]
    struct Resp {
        protocol: Option<Inner>,
    }
    #[derive(Deserialize)]
    struct Inner {
        weeklyEmissions: Option<String>,
        activePeriod: Option<String>,
        epochCount: Option<String>,
    }

    let r: Resp = query(
        r#"{ protocol(id: "windswap") { weeklyEmissions activePeriod epochCount } }"#,
    )
    .await?;

    let p = r.protocol.ok_or("No protocol data")?;
    Ok(ProtocolInfo {
        weekly_emissions: p.weeklyEmissions.and_then(|s| s.parse().ok()),
        active_period: p.activePeriod.and_then(|s| s.parse().ok()),
        epoch_count: p.epochCount.and_then(|s| s.parse().ok()),
    })
}

// ─── Pools ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct PoolInfo {
    pub id: String,
    pub token0_symbol: String,
    pub token1_symbol: String,
    pub token0_decimals: u8,
    pub token1_decimals: u8,
    pub tvl: f64,
    pub volume_24h: f64,
    pub pool_type: String,
    pub stable: bool,
    pub tick_spacing: Option<i32>,
}

pub async fn fetch_pools() -> Result<Vec<PoolInfo>, String> {
    #[derive(Deserialize)]
    struct Resp {
        pools: Vec<Raw>,
    }
    #[derive(Deserialize)]
    struct Raw {
        id: String,
        token0: TokenRef,
        token1: TokenRef,
        #[serde(default)]
        tvl: String,
        #[serde(default, rename = "volume24h")]
        volume_24h: Option<String>,
        #[serde(default, rename = "type")]
        pool_type: Option<String>,
        #[serde(default)]
        stable: Option<bool>,
        #[serde(default, rename = "tickSpacing")]
        tick_spacing: Option<i32>,
    }
    #[derive(Deserialize)]
    struct TokenRef {
        symbol: String,
        #[serde(default = "default_decimals")]
        decimals: i32,
    }
    fn default_decimals() -> i32 {
        18
    }

    let r: Resp = query(
        r#"{ pools(first: 200, orderBy: tvl, orderDirection: desc) {
            id token0 { symbol decimals } token1 { symbol decimals }
            tvl volume24h type: __typename stable tickSpacing
        } }"#,
    )
    .await?;

    Ok(r.pools
        .into_iter()
        .map(|p| PoolInfo {
            id: p.id,
            token0_symbol: p.token0.symbol,
            token1_symbol: p.token1.symbol,
            token0_decimals: p.token0.decimals as u8,
            token1_decimals: p.token1.decimals as u8,
            tvl: p.tvl.parse().unwrap_or(0.0),
            volume_24h: p.volume_24h.unwrap_or_default().parse().unwrap_or(0.0),
            pool_type: p.pool_type.unwrap_or_else(|| "V2".into()),
            stable: p.stable.unwrap_or(false),
            tick_spacing: p.tick_spacing,
        })
        .collect())
}

// ─── Gauges ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct GaugeInfo {
    pub id: String,
    pub pool_id: String,
    pub token0_symbol: String,
    pub token1_symbol: String,
    pub weight: f64,
    pub alive: bool,
    pub pool_type: String,
    pub is_stable: bool,
    pub bribe_reward: Option<String>,
    pub fee_reward: Option<String>,
}

pub async fn fetch_gauges() -> Result<Vec<GaugeInfo>, String> {
    #[derive(Deserialize)]
    struct Resp {
        gauges: Vec<Raw>,
    }
    #[derive(Deserialize)]
    struct Raw {
        id: String,
        pool: PoolRef,
        #[serde(default)]
        weight: String,
        #[serde(default)]
        alive: bool,
        #[serde(default, rename = "type")]
        pool_type: Option<String>,
        #[serde(default)]
        isStable: Option<bool>,
        #[serde(default)]
        bribeReward: Option<String>,
        #[serde(default)]
        feeReward: Option<String>,
    }
    #[derive(Deserialize)]
    struct PoolRef {
        id: String,
        #[serde(default)]
        token0: Option<TokenRef2>,
        #[serde(default)]
        token1: Option<TokenRef2>,
    }
    #[derive(Deserialize)]
    struct TokenRef2 {
        symbol: String,
    }

    let r: Resp = query(
        r#"{ gauges(first: 200, orderBy: weight, orderDirection: desc) {
            id pool { id token0 { symbol } token1 { symbol } }
            weight alive type: __typename isStable bribeReward feeReward
        } }"#,
    )
    .await?;

    Ok(r.gauges
        .into_iter()
        .map(|g| GaugeInfo {
            id: g.id,
            pool_id: g.pool.id,
            token0_symbol: g.pool.token0.map(|t| t.symbol).unwrap_or("?".into()),
            token1_symbol: g.pool.token1.map(|t| t.symbol).unwrap_or("?".into()),
            weight: g.weight.parse().unwrap_or(0.0),
            alive: g.alive,
            pool_type: g.pool_type.unwrap_or_else(|| "V2".into()),
            is_stable: g.isStable.unwrap_or(false),
            bribe_reward: g.bribeReward,
            fee_reward: g.feeReward,
        })
        .collect())
}

// ─── veNFTs ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct VeNFTInfo {
    pub id: String,
    pub locked_amount: U256,
    pub lock_end: U256,
    pub is_permanent: bool,
    pub voting_power: U256,
}

pub async fn fetch_venfts(owner: &str) -> Result<Vec<VeNFTInfo>, String> {
    #[derive(Deserialize)]
    struct Resp {
        #[serde(rename = "veNFTs")]
        venfts: Vec<Raw>,
    }
    #[derive(Deserialize)]
    struct Raw {
        id: String,
        lockedAmount: String,
        lockEnd: String,
        #[serde(default)]
        isPermanent: bool,
        votingPower: String,
    }

    let gql = format!(
        r#"{{ veNFTs(where: {{ owner: "{owner}", lockedAmount_gt: "0" }}, first: 50) {{
            id lockedAmount lockEnd isPermanent votingPower
        }} }}"#
    );
    let r: Resp = query(&gql).await?;

    Ok(r.venfts
        .into_iter()
        .map(|v| VeNFTInfo {
            id: v.id,
            locked_amount: u256_from_str(&v.lockedAmount),
            lock_end: u256_from_str(&v.lockEnd),
            is_permanent: v.isPermanent,
            voting_power: u256_from_str(&v.votingPower),
        })
        .collect())
}

// ─── Votes ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct VoteInfo {
    pub pool_id: String,
    pub weight: U256,
    pub epoch: String,
}

pub async fn fetch_user_votes(token_id: &str) -> Result<Vec<VoteInfo>, String> {
    #[derive(Deserialize)]
    struct Resp {
        #[serde(rename = "veVotes")]
        votes: Vec<Raw>,
    }
    #[derive(Deserialize)]
    struct Raw {
        pool: PoolId,
        weight: String,
        epoch: String,
    }
    #[derive(Deserialize)]
    struct PoolId {
        id: String,
    }

    let gql = format!(
        r#"{{ veVotes(where: {{ veNFT: "{token_id}", isActive: true }}, orderBy: epoch, orderDirection: desc, first: 500) {{
            pool {{ id }} weight epoch
        }} }}"#
    );
    let r: Resp = query(&gql).await?;

    Ok(r.votes
        .into_iter()
        .map(|v| VoteInfo {
            pool_id: v.pool.id,
            weight: u256_from_str(&v.weight),
            epoch: v.epoch,
        })
        .collect())
}

// ─── Epoch bribes ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct BribeInfo {
    pub gauge_id: String,
    pub token_symbol: String,
    pub token_decimals: u8,
    pub amount: f64,
    pub amount_usd: f64,
}

pub async fn fetch_epoch_bribes(epoch: &str) -> Result<Vec<BribeInfo>, String> {
    #[derive(Deserialize)]
    struct Resp {
        #[serde(rename = "gaugeEpochBribes")]
        bribes: Vec<Raw>,
    }
    #[derive(Deserialize)]
    struct Raw {
        gauge: GaugeId,
        token: TokenInfo,
        totalAmount: String,
        totalAmountUSD: String,
    }
    #[derive(Deserialize)]
    struct GaugeId {
        id: String,
    }
    #[derive(Deserialize)]
    struct TokenInfo {
        symbol: String,
        #[serde(default = "default_dec")]
        decimals: i32,
    }
    fn default_dec() -> i32 {
        18
    }

    let gql = format!(
        r#"{{ gaugeEpochBribes(where: {{ epoch: "{epoch}" }}, first: 100) {{
            gauge {{ id }} token {{ symbol decimals }} totalAmount totalAmountUSD
        }} }}"#
    );
    let r: Resp = query(&gql).await?;

    Ok(r.bribes
        .into_iter()
        .map(|b| BribeInfo {
            gauge_id: b.gauge.id,
            token_symbol: b.token.symbol,
            token_decimals: b.token.decimals as u8,
            amount: b.totalAmount.parse().unwrap_or(0.0),
            amount_usd: b.totalAmountUSD.parse().unwrap_or(0.0),
        })
        .collect())
}

// ─── User CL positions ────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct UserPosition {
    pub id: String,
    pub pool_id: String,
    pub token0_symbol: String,
    pub token1_symbol: String,
    pub liquidity: U256,
    pub amount_usd: f64,
}

pub async fn fetch_user_positions(owner: &str) -> Result<Vec<UserPosition>, String> {
    #[derive(Deserialize)]
    struct Resp {
        positions: Vec<Raw>,
    }
    #[derive(Deserialize)]
    struct Raw {
        id: String,
        pool: PoolRef,
        liquidity: String,
        amountUSD: String,
    }
    #[derive(Deserialize)]
    struct PoolRef {
        id: String,
        token0: SymbolRef,
        token1: SymbolRef,
    }
    #[derive(Deserialize)]
    struct SymbolRef {
        symbol: String,
    }

    let gql = format!(
        r#"{{ positions(where: {{ owner: "{owner}", liquidity_gt: "0" }}, first: 50) {{
            id pool {{ id token0 {{ symbol }} token1 {{ symbol }} }} liquidity amountUSD
        }} }}"#
    );
    let r: Resp = query(&gql).await?;

    Ok(r.positions
        .into_iter()
        .map(|p| UserPosition {
            id: p.id,
            pool_id: p.pool.id,
            token0_symbol: p.pool.token0.symbol,
            token1_symbol: p.pool.token1.symbol,
            liquidity: u256_from_str(&p.liquidity),
            amount_usd: p.amountUSD.parse().unwrap_or(0.0),
        })
        .collect())
}
