use alloy_primitives::{address, Address};

// ─── Chain ────────────────────────────────────────────────────────────────────
pub const CHAIN_ID: u64 = 8453; // Base

pub const FALLBACK_RPCS: &[&str] = &[
    "https://base-rpc.publicnode.com",
    "https://base.meowrpc.com",
    "https://rpc.ankr.com/base",
    "https://1rpc.io/base",
];

// ─── V2 Contracts ─────────────────────────────────────────────────────────────
pub const WIND_TOKEN:     Address = address!("888a4F89aF7dD0Be836cA367C9FF5490c0F6e888");
pub const V2_ROUTER:      Address = address!("88883154C9F8eb3bd34fb760bda1EB7556a20e14");
pub const V2_FACTORY:     Address = address!("88880e3dA8676C879c3D019EDE0b5a74586813be");
pub const VOTING_ESCROW:  Address = address!("88889C4Be508cA88eba6ad802340C0563891D426");
pub const VOTER:          Address = address!("88881EB4b5dD3461fC0CFBc44606E3b401197E38");

// ─── V3 / CL Contracts ────────────────────────────────────────────────────────
pub const CL_SWAP_ROUTER: Address = address!("8888EEA5C97AF36f764259557d2D4CA23e6b19Ff");
pub const CL_FACTORY:     Address = address!("8888A3D87EF6aBC5F50572661E4729A45b255cF6");
pub const CL_QUOTER_V2:   Address = address!("888831E6a70C71009765bAa1C3d86031539d6B15");
pub const CL_NFT_PM:      Address = address!("8888bB79b80e6B48014493819656Ffc1444d7687");

// ─── Common ───────────────────────────────────────────────────────────────────
pub const WETH:           Address = address!("4200000000000000000000000000000000000006");
pub const USDC:           Address = address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913");
pub const USDT:           Address = address!("fde4C96c8593536E31F229EA8f37b2ADa2699bb2");

// V3 tick spacings to try when auto-routing
pub const CL_TICK_SPACINGS: &[i32] = &[1, 10, 50, 100, 200, 500, 1000, 2000];

// ─── Token list ───────────────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub address: Address,   // zero address = native ETH
    pub symbol: &'static str,
    pub name: &'static str,
    pub decimals: u8,
    pub logo: &'static str, // emoji or URL
    pub is_native: bool,
}

impl Token {
    pub const fn new(
        address: Address,
        symbol: &'static str,
        name: &'static str,
        decimals: u8,
        logo: &'static str,
    ) -> Self {
        Self { address, symbol, name, decimals, logo, is_native: false }
    }
    pub const fn native() -> Self {
        Self {
            address: Address::ZERO,
            symbol: "ETH",
            name: "Ether",
            decimals: 18,
            logo: "⟠",
            is_native: true,
        }
    }
}

pub static TOKENS: &[Token] = &[
    Token::native(),
    Token::new(WETH,  "WETH",  "Wrapped Ether",           18, "⟠"),
    Token::new(WIND_TOKEN, "WIND", "Wind",                 18, "💨"),
    Token::new(USDC,  "USDC",  "USD Coin",                  6, "💲"),
    Token::new(USDT,  "USDT",  "Tether USD",                6, "💵"),
    Token::new(address!("50c5725949A6F0c72E6C4a641F24049A917DB0Cb"), "DAI",  "Dai Stablecoin", 18, "🔷"),
    Token::new(address!("0555E30da8f98308EdB960aa94C0Db47230d2B9c"), "WBTC", "Wrapped Bitcoin",  8, "₿"),
    Token::new(address!("88Fb150BDc53A65fe94Dea0c9BA0a6dAf8C6e196"), "LINK", "Chainlink",       18, "🔗"),
];

/// Look up a token from the default list by address (case-insensitive).
pub fn find_token(addr: &Address) -> Option<&'static Token> {
    TOKENS.iter().find(|t| &t.address == addr)
}
