//! MetaMask / injected-wallet bridge via wasm-bindgen.
//!
//! All calls go through `window.ethereum.request({method, params})` which
//! returns a JS Promise. We convert it to a Rust Future with `JsFuture`.

use js_sys::{Array, Function, Object, Promise, Reflect};
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

// ─── Internal helpers ─────────────────────────────────────────────────────────

fn get_ethereum() -> Result<JsValue, String> {
    let window = web_sys::window().ok_or("No window object")?;
    let eth = Reflect::get(&window, &JsValue::from_str("ethereum"))
        .map_err(|_| "Cannot read window.ethereum")?;
    if eth.is_undefined() || eth.is_null() {
        Err("No injected wallet detected. Please install MetaMask.".into())
    } else {
        Ok(eth)
    }
}

/// Call `window.ethereum.request(requestObj)` and await the Promise.
async fn ethereum_request(method: &str, params: JsValue) -> Result<JsValue, String> {
    let eth = get_ethereum()?;

    let request_fn = Reflect::get(&eth, &JsValue::from_str("request"))
        .map_err(|_| "window.ethereum has no request() method")?;
    let request_fn = Function::from(request_fn);

    // Build { method, params }
    let req_obj = Object::new();
    Reflect::set(&req_obj, &JsValue::from_str("method"), &JsValue::from_str(method)).ok();
    Reflect::set(&req_obj, &JsValue::from_str("params"), &params).ok();

    let args = Array::new();
    args.push(&req_obj);

    let promise_val = request_fn
        .apply(&eth, &args)
        .map_err(|e| format!("ethereum.request threw: {e:?}"))?;

    JsFuture::from(Promise::from(promise_val))
        .await
        .map_err(|e| {
            // Distinguish user rejections from real errors
            let msg = format!("{e:?}");
            if msg.contains("4001") || msg.contains("User rejected") {
                "Transaction rejected by user".to_string()
            } else {
                format!("Wallet error: {msg}")
            }
        })
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Return true if `window.ethereum` is present.
pub fn has_ethereum() -> bool {
    get_ethereum().is_ok()
}

/// Request the list of connected accounts (triggers MetaMask popup if needed).
/// Returns the first account address.
pub async fn request_accounts() -> Result<String, String> {
    let result = ethereum_request("eth_requestAccounts", Array::new().into()).await?;
    let accounts = Array::from(&result);
    accounts
        .get(0)
        .as_string()
        .ok_or_else(|| "No account returned".to_string())
}

/// Get the currently selected chain ID as a decimal u64.
pub async fn get_chain_id() -> Result<u64, String> {
    let result = ethereum_request("eth_chainId", Array::new().into()).await?;
    let hex = result.as_string().ok_or("chainId not a string")?;
    let hex = hex.trim_start_matches("0x");
    u64::from_str_radix(hex, 16).map_err(|e| e.to_string())
}

/// Ask MetaMask to switch to Base (chain 8453 = 0x2105).
pub async fn switch_to_base() -> Result<(), String> {
    let params = Array::new();
    let obj = Object::new();
    Reflect::set(&obj, &JsValue::from_str("chainId"), &JsValue::from_str("0x2105")).ok();
    params.push(&obj);

    ethereum_request("wallet_switchEthereumChain", params.into())
        .await
        .map(|_| ())
}

/// Send an EVM transaction via the wallet. Returns the tx hash.
///
/// `value_hex` – `"0x0"` for ERC-20 calls; hex-encoded ETH value for payable calls.
pub async fn send_transaction(
    from:      &str,
    to:        &str,
    calldata:  &[u8],
    value_hex: &str,
) -> Result<String, String> {
    let data_hex = format!("0x{}", hex::encode(calldata));

    let tx = Object::new();
    Reflect::set(&tx, &JsValue::from_str("from"),  &JsValue::from_str(from)).ok();
    Reflect::set(&tx, &JsValue::from_str("to"),    &JsValue::from_str(to)).ok();
    Reflect::set(&tx, &JsValue::from_str("data"),  &JsValue::from_str(&data_hex)).ok();
    Reflect::set(&tx, &JsValue::from_str("value"), &JsValue::from_str(value_hex)).ok();

    let params = Array::new();
    params.push(&tx);

    let result = ethereum_request("eth_sendTransaction", params.into()).await?;
    result
        .as_string()
        .ok_or_else(|| "Invalid tx hash returned".to_string())
}

/// ERC-20 `approve(spender, type(uint256).max)` via the wallet.
pub async fn approve_max(
    from:    &str,
    token:   &str,
    spender: &str,
) -> Result<String, String> {
    use alloy_sol_types::SolCall;
    use crate::blockchain::IERC20;
    use alloy_primitives::Address;

    let spender_addr: Address = spender.parse().map_err(|e: alloy_primitives::hex::FromHexError| e.to_string())?;
    let call = IERC20::approveCall {
        spender: spender_addr,
        amount: alloy_primitives::U256::MAX,
    };
    let calldata = call.abi_encode();

    send_transaction(from, token, &calldata, "0x0").await
}

/// Wait for a tx receipt by polling `eth_getTransactionReceipt` every 2 s.
/// Resolves once the receipt is non-null (tx confirmed) or after `max_polls`.
pub async fn wait_for_receipt(tx_hash: &str, max_polls: u32) -> Result<(), String> {
    for _ in 0..max_polls {
        gloo_timers::future::TimeoutFuture::new(2_000).await;

        let params = Array::new();
        params.push(&JsValue::from_str(tx_hash));

        let result = ethereum_request("eth_getTransactionReceipt", params.into()).await?;
        if !result.is_null() && !result.is_undefined() {
            return Ok(());
        }
    }
    Err("Transaction not confirmed in time".into())
}
