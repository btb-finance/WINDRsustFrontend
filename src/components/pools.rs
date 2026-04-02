use alloy_primitives::{Address, U256};
use leptos::*;
use alloy_sol_types::SolCall;

use crate::{
    blockchain::{
        apply_slippage_min, deadline_secs, fetch_user_positions,
        fmt_units, get_lp_balance, get_pool_address,
        get_erc20_allowance, parse_token_amount,
        quote_add_liquidity, quote_remove_liquidity, IRouter,
        PoolPosition,
    },
    components::token_selector::TokenSelectorModal,
    config::{Token, TOKENS, V2_ROUTER, WETH},
    state::{use_app_state, ToastKind},
    wallet::{approve_max, send_transaction, wait_for_receipt},
};

// ─── Page ─────────────────────────────────────────────────────────────────────

#[component]
pub fn PoolsPage() -> impl IntoView {
    let (tab, set_tab) = create_signal(0u8); // 0=Add 1=Remove 2=Positions

    view! {
        <main class="max-w-2xl mx-auto px-4 py-10 space-y-6">
            // ── Header ────────────────────────────────────────────────────
            <div class="flex items-center justify-between">
                <div>
                    <h1 class="text-2xl font-bold text-white">"Liquidity Pools"</h1>
                    <p class="text-sm text-gray-400 mt-1">"Provide liquidity and earn trading fees"</p>
                </div>
                <span class="text-3xl">"🌊"</span>
            </div>

            // ── Tabs ──────────────────────────────────────────────────────
            <div class="flex gap-1 bg-gray-800/60 rounded-2xl p-1">
                {[("➕ Add", 0u8), ("➖ Remove", 1u8), ("📊 Positions", 2u8)].into_iter().map(|(label, idx)| {
                    view! {
                        <button
                            class=move || format!(
                                "flex-1 py-2 px-3 rounded-xl text-sm font-medium transition-all {}",
                                if tab.get() == idx {
                                    "bg-wind-500 text-white shadow-lg"
                                } else {
                                    "text-gray-400 hover:text-white"
                                }
                            )
                            on:click=move |_| set_tab.set(idx)
                        >{label}</button>
                    }
                }).collect_view()}
            </div>

            // ── Tab content ───────────────────────────────────────────────
            {move || match tab.get() {
                0 => view! { <AddLiquidityPanel/> }.into_view(),
                1 => view! { <RemoveLiquidityPanel/> }.into_view(),
                _ => view! { <PositionsPanel/> }.into_view(),
            }}
        </main>
    }
}

// ─── Add Liquidity ────────────────────────────────────────────────────────────

#[component]
fn AddLiquidityPanel() -> impl IntoView {
    let state = use_app_state();

    let (token_a,    set_token_a)    = create_signal(TOKENS[0].clone()); // ETH
    let (token_b,    set_token_b)    = create_signal(TOKENS[3].clone()); // USDC
    let (amount_a,   set_amount_a)   = create_signal(String::new());
    let (amount_b,   set_amount_b)   = create_signal(String::new());
    let (stable,     set_stable)     = create_signal(false);
    let (slippage,   _)              = create_signal(0.5_f64);
    let (modal_a,    set_modal_a)    = create_signal(false);
    let (modal_b,    set_modal_b)    = create_signal(false);
    let (quoting,    set_quoting)    = create_signal(false);
    let (busy,       set_busy)       = create_signal(false);
    let (pool_exists,set_pool_exists)= create_signal(true);
    let (lp_preview, set_lp_preview) = create_signal(String::new());
    let (need_app_a, set_need_app_a) = create_signal(false);
    let (need_app_b, set_need_app_b) = create_signal(false);

    // When amount_a changes: call quoteAddLiquidity to fill amount_b
    let on_amount_a_input = move |e: web_sys::Event| {
        let val = event_target_value(&e);
        set_amount_a.set(val.clone());
        if val.is_empty() {
            set_amount_b.set(String::new());
            set_lp_preview.set(String::new());
            return;
        }
        let ta = token_a.get_untracked();
        let tb = token_b.get_untracked();
        let is_stable = stable.get_untracked();
        // Use WETH address for ETH
        let addr_a = if ta.is_native { WETH } else { ta.address };
        let addr_b = if tb.is_native { WETH } else { tb.address };
        let raw_a = parse_token_amount(&val, ta.decimals);

        if raw_a.is_zero() { return; }
        set_quoting.set(true);

        spawn_local(async move {
            // Use a large desired_b so router tells us the optimal b for this a
            let big_b = U256::from(10u64).pow(U256::from(36u64));
            match quote_add_liquidity(addr_a, addr_b, is_stable, raw_a, big_b).await {
                Some((_, opt_b, lp)) => {
                    let b_fmt = fmt_units(opt_b, tb.decimals);
                    // trim trailing zeros
                    let b_trimmed = trim_fmt(&b_fmt);
                    set_amount_b.set(b_trimmed);
                    set_lp_preview.set(fmt_units(lp, 18));
                    set_pool_exists.set(true);
                }
                None => {
                    // New pool — no quote available
                    set_pool_exists.set(false);
                    set_lp_preview.set(String::new());
                }
            }
            set_quoting.set(false);
        });
    };

    let on_amount_b_input = move |e: web_sys::Event| {
        let val = event_target_value(&e);
        set_amount_b.set(val.clone());
        if val.is_empty() { return; }
        let ta = token_a.get_untracked();
        let tb = token_b.get_untracked();
        let is_stable = stable.get_untracked();
        let addr_a = if ta.is_native { WETH } else { ta.address };
        let addr_b = if tb.is_native { WETH } else { tb.address };
        let raw_b = parse_token_amount(&val, tb.decimals);
        if raw_b.is_zero() { return; }
        set_quoting.set(true);
        spawn_local(async move {
            let big_a = U256::from(10u64).pow(U256::from(36u64));
            match quote_add_liquidity(addr_a, addr_b, is_stable, big_a, raw_b).await {
                Some((opt_a, _, lp)) => {
                    let a_fmt = fmt_units(opt_a, ta.decimals);
                    set_amount_a.set(trim_fmt(&a_fmt));
                    set_lp_preview.set(fmt_units(lp, 18));
                    set_pool_exists.set(true);
                }
                None => { set_pool_exists.set(false); }
            }
            set_quoting.set(false);
        });
    };

    // Check allowances on input change
    let check_allowances = move || {
        let wallet = state.wallet_address.get_untracked();
        let ta = token_a.get_untracked();
        let tb = token_b.get_untracked();
        let amt_a = amount_a.get_untracked();
        let amt_b = amount_b.get_untracked();
        if let Some(addr) = wallet {
            let from: Address = addr.parse().unwrap_or(Address::ZERO);
            let raw_a = parse_token_amount(&amt_a, ta.decimals);
            let raw_b = parse_token_amount(&amt_b, tb.decimals);
            let addr_a = ta.address;
            let addr_b = tb.address;
            let is_native_a = ta.is_native;
            let is_native_b = tb.is_native;
            spawn_local(async move {
                if !is_native_a && !raw_a.is_zero() {
                    let allow = get_erc20_allowance(addr_a, from, V2_ROUTER).await.unwrap_or(U256::ZERO);
                    set_need_app_a.set(allow < raw_a);
                }
                if !is_native_b && !raw_b.is_zero() {
                    let allow = get_erc20_allowance(addr_b, from, V2_ROUTER).await.unwrap_or(U256::ZERO);
                    set_need_app_b.set(allow < raw_b);
                }
            });
        }
    };

    let do_add = move |_| {
        let ta = token_a.get_untracked();
        let tb = token_b.get_untracked();
        let amt_a = amount_a.get_untracked();
        let amt_b = amount_b.get_untracked();
        let slip  = slippage.get_untracked();
        let is_stable = stable.get_untracked();

        let wallet = match state.wallet_address.get_untracked() {
            Some(a) => a,
            None => { state.toast(ToastKind::Error, "Wallet not connected", 3000); return; }
        };

        let raw_a = parse_token_amount(&amt_a, ta.decimals);
        let raw_b = parse_token_amount(&amt_b, tb.decimals);
        if raw_a.is_zero() || raw_b.is_zero() {
            state.toast(ToastKind::Error, "Enter amounts", 3000);
            return;
        }

        set_busy.set(true);

        spawn_local(async move {
            let from: Address = wallet.parse().unwrap_or(Address::ZERO);
            let router_hex = format!("{:#x}", V2_ROUTER);
            let min_a = apply_slippage_min(raw_a, slip);
            let min_b = apply_slippage_min(raw_b, slip);
            let dl = U256::from(deadline_secs(30));

            // Approvals
            let router_str = format!("{:#x}", V2_ROUTER);
            if !ta.is_native && need_app_a.get_untracked() {
                state.toast(ToastKind::Info, format!("Approving {}…", ta.symbol), 3000);
                if let Err(e) = approve_max(&wallet, &format!("{:#x}", ta.address), &router_str).await {
                    state.toast(ToastKind::Error, e, 5000);
                    set_busy.set(false);
                    return;
                }
                set_need_app_a.set(false);
            }
            if !tb.is_native && need_app_b.get_untracked() {
                state.toast(ToastKind::Info, format!("Approving {}…", tb.symbol), 3000);
                if let Err(e) = approve_max(&wallet, &format!("{:#x}", tb.address), &router_str).await {
                    state.toast(ToastKind::Error, e, 5000);
                    set_busy.set(false);
                    return;
                }
                set_need_app_b.set(false);
            }

            let to = from;
            let addr_a = if ta.is_native { WETH } else { ta.address };
            let addr_b = if tb.is_native { WETH } else { tb.address };

            let result = if ta.is_native {
                let call = IRouter::addLiquidityETHCall {
                    token:              addr_b,
                    stable:             is_stable,
                    amountTokenDesired: raw_b,
                    amountTokenMin:     min_b,
                    amountETHMin:       min_a,
                    to,
                    deadline:           dl,
                };
                let value_hex = format!("0x{:x}", raw_a);
                send_transaction(&wallet, &router_hex, &call.abi_encode(), &value_hex).await
            } else if tb.is_native {
                let call = IRouter::addLiquidityETHCall {
                    token:              addr_a,
                    stable:             is_stable,
                    amountTokenDesired: raw_a,
                    amountTokenMin:     min_a,
                    amountETHMin:       min_b,
                    to,
                    deadline:           dl,
                };
                let value_hex = format!("0x{:x}", raw_b);
                send_transaction(&wallet, &router_hex, &call.abi_encode(), &value_hex).await
            } else {
                let call = IRouter::addLiquidityCall {
                    tokenA:         addr_a,
                    tokenB:         addr_b,
                    stable:         is_stable,
                    amountADesired: raw_a,
                    amountBDesired: raw_b,
                    amountAMin:     min_a,
                    amountBMin:     min_b,
                    to,
                    deadline:       dl,
                };
                send_transaction(&wallet, &router_hex, &call.abi_encode(), "0x0").await
            };

            match result {
                Ok(hash) => {
                    state.toast(ToastKind::Info, format!("Tx submitted: {}", &hash[..10]), 4000);
                    let _ = wait_for_receipt(&hash, 60).await;
                    state.toast(ToastKind::Success, "✓ Liquidity added!", 5000);
                    set_amount_a.set(String::new());
                    set_amount_b.set(String::new());
                    set_lp_preview.set(String::new());
                }
                Err(e) => state.toast(ToastKind::Error, e, 7000),
            }
            set_busy.set(false);
        });
    };

    view! {
        <div class="space-y-3">
            // ── Stable / Volatile toggle ──────────────────────────────────
            <div class="flex items-center justify-between bg-gray-800/60 rounded-2xl px-4 py-3">
                <div class="text-sm text-gray-300">"Pool type"</div>
                <div class="flex items-center gap-3 text-sm">
                    <span class=move || format!("font-medium {}", if !stable.get() { "text-wind-400" } else { "text-gray-500" })>"Volatile"</span>
                    <button
                        class=move || format!(
                            "relative w-11 h-6 rounded-full transition-colors {}",
                            if stable.get() { "bg-wind-500" } else { "bg-gray-600" }
                        )
                        on:click=move |_| set_stable.update(|v| *v = !*v)
                    >
                        <span class=move || format!(
                            "absolute top-1 w-4 h-4 bg-white rounded-full shadow transition-transform {}",
                            if stable.get() { "left-6" } else { "left-1" }
                        )></span>
                    </button>
                    <span class=move || format!("font-medium {}", if stable.get() { "text-wind-400" } else { "text-gray-500" })>"Stable"</span>
                </div>
            </div>

            // ── Token A ───────────────────────────────────────────────────
            <TokenAmountInput
                label="Token A"
                token=Signal::from(token_a)
                amount=Signal::from(amount_a)
                on_token_click=move || set_modal_a.set(true)
                on_input=on_amount_a_input
                on_input_done=move || check_allowances()
            />

            <div class="flex items-center justify-center text-gray-600 text-xl font-thin select-none">"+"</div>

            // ── Token B ───────────────────────────────────────────────────
            <TokenAmountInput
                label="Token B"
                token=Signal::from(token_b)
                amount=Signal::from(amount_b)
                on_token_click=move || set_modal_b.set(true)
                on_input=on_amount_b_input
                on_input_done=move || check_allowances()
            />

            // ── Pool info ─────────────────────────────────────────────────
            {move || {
                let lp = lp_preview.get();
                let exists = pool_exists.get();
                let q = quoting.get();
                if q {
                    view! {
                        <div class="bg-gray-800/40 rounded-xl px-4 py-3 text-sm text-gray-400 text-center animate-pulse">
                            "Calculating optimal amounts…"
                        </div>
                    }.into_view()
                } else if !exists {
                    view! {
                        <div class="bg-blue-900/30 border border-blue-700/40 rounded-xl px-4 py-3 text-sm text-blue-300">
                            "🆕 This pool does not exist yet — you will create it and set the initial price."
                        </div>
                    }.into_view()
                } else if !lp.is_empty() {
                    let lp_trimmed = trim_fmt(&lp);
                    view! {
                        <div class="bg-gray-800/40 rounded-xl px-4 py-3 flex justify-between text-sm">
                            <span class="text-gray-400">"LP tokens to receive"</span>
                            <span class="text-white font-medium">{lp_trimmed}</span>
                        </div>
                    }.into_view()
                } else {
                    ().into_view()
                }
            }}

            // ── Approval warnings ─────────────────────────────────────────
            {move || {
                let sym_a = token_a.get().symbol;
                let sym_b = token_b.get().symbol;
                let na = need_app_a.get();
                let nb = need_app_b.get();
                if na || nb {
                    let txt = match (na, nb) {
                        (true, true)  => format!("Approve {sym_a} and {sym_b} before adding"),
                        (true, false) => format!("Approve {sym_a} before adding"),
                        _             => format!("Approve {sym_b} before adding"),
                    };
                    view! {
                        <div class="bg-yellow-900/30 border border-yellow-700/40 rounded-xl px-4 py-2 text-sm text-yellow-300">
                            "⚠ " {txt}
                        </div>
                    }.into_view()
                } else { ().into_view() }
            }}

            // ── Submit ────────────────────────────────────────────────────
            <button
                class="btn-primary w-full mt-1"
                prop:disabled=move || busy.get() || quoting.get()
                on:click=do_add
            >
                {move || {
                    let b = busy.get();
                    let ta = token_a.get().symbol;
                    let tb = token_b.get().symbol;
                    if b { "Adding Liquidity…".to_string() }
                    else { format!("Add {ta} / {tb} Liquidity") }
                }}
            </button>
        </div>

        // ── Token selector modals ─────────────────────────────────────────
        {move || modal_a.get().then(|| view! {
            <TokenSelectorModal
                selected=Signal::from(token_a)
                on_select=move |t| { set_token_a.set(t); set_modal_a.set(false); }
                on_close=move || set_modal_a.set(false)
            />
        })}
        {move || modal_b.get().then(|| view! {
            <TokenSelectorModal
                selected=Signal::from(token_b)
                on_select=move |t| { set_token_b.set(t); set_modal_b.set(false); }
                on_close=move || set_modal_b.set(false)
            />
        })}
    }
}

// ─── Remove Liquidity ─────────────────────────────────────────────────────────

#[component]
fn RemoveLiquidityPanel() -> impl IntoView {
    let state = use_app_state();

    let (token_a,    set_token_a)    = create_signal(TOKENS[0].clone());
    let (token_b,    set_token_b)    = create_signal(TOKENS[3].clone());
    let (stable,     set_stable)     = create_signal(false);
    let (pct,        set_pct)        = create_signal(50u8);
    let (modal_a,    set_modal_a)    = create_signal(false);
    let (modal_b,    set_modal_b)    = create_signal(false);
    let (lp_balance, set_lp_balance) = create_signal(U256::ZERO);
    let (lp_fmt,     set_lp_fmt)     = create_signal(String::new());
    let (recv_a,     set_recv_a)     = create_signal(String::new());
    let (recv_b,     set_recv_b)     = create_signal(String::new());
    let (loading,    set_loading)    = create_signal(false);
    let (busy,       set_busy)       = create_signal(false);
    let (slippage,   _)              = create_signal(0.5_f64);

    // Fetch LP balance whenever token or stable selection changes
    let fetch_lp = move || {
        let wallet = state.wallet_address.get_untracked();
        let ta = token_a.get_untracked();
        let tb = token_b.get_untracked();
        let is_stable = stable.get_untracked();
        let addr_a = if ta.is_native { WETH } else { ta.address };
        let addr_b = if tb.is_native { WETH } else { tb.address };
        set_recv_a.set(String::new());
        set_recv_b.set(String::new());

        if let Some(addr) = wallet {
            let from: Address = addr.parse().unwrap_or(Address::ZERO);
            set_loading.set(true);
            spawn_local(async move {
                if let Some(pool) = get_pool_address(addr_a, addr_b, is_stable).await {
                    let bal = get_lp_balance(pool, from).await;
                    set_lp_balance.set(bal);
                    set_lp_fmt.set(trim_fmt(&fmt_units(bal, 18)));
                } else {
                    set_lp_balance.set(U256::ZERO);
                    set_lp_fmt.set("0".into());
                }
                set_loading.set(false);
            });
        }
    };

    // Recompute receive preview when pct or LP balance changes
    let update_preview = move || {
        let bal = lp_balance.get_untracked();
        if bal.is_zero() { return; }
        let p = pct.get_untracked();
        let lp_amt = bal * U256::from(p as u64) / U256::from(100u64);
        let ta = token_a.get_untracked();
        let tb = token_b.get_untracked();
        let is_stable = stable.get_untracked();
        let addr_a = if ta.is_native { WETH } else { ta.address };
        let addr_b = if tb.is_native { WETH } else { tb.address };
        spawn_local(async move {
            if let Some((out_a, out_b)) = quote_remove_liquidity(addr_a, addr_b, is_stable, lp_amt).await {
                set_recv_a.set(trim_fmt(&fmt_units(out_a, ta.decimals)));
                set_recv_b.set(trim_fmt(&fmt_units(out_b, tb.decimals)));
            }
        });
    };

    // Initial load
    fetch_lp();

    let do_remove = move |_| {
        let bal = lp_balance.get_untracked();
        if bal.is_zero() { state.toast(ToastKind::Error, "No LP balance", 3000); return; }
        let p = pct.get_untracked();
        let lp_amt = bal * U256::from(p as u64) / U256::from(100u64);
        let ta = token_a.get_untracked();
        let tb = token_b.get_untracked();
        let is_stable = stable.get_untracked();
        let slip = slippage.get_untracked();
        let addr_a = if ta.is_native { WETH } else { ta.address };
        let addr_b = if tb.is_native { WETH } else { tb.address };

        let wallet = match state.wallet_address.get_untracked() {
            Some(a) => a,
            None => { state.toast(ToastKind::Error, "Wallet not connected", 3000); return; }
        };
        set_busy.set(true);

        spawn_local(async move {
            let from: Address = wallet.parse().unwrap_or(Address::ZERO);
            let router_hex = format!("{:#x}", V2_ROUTER);
            let dl = U256::from(deadline_secs(30));

            // Approve LP token (pool address) to router
            let router_str = format!("{:#x}", V2_ROUTER);
            if let Some(pool) = get_pool_address(addr_a, addr_b, is_stable).await {
                let allow = get_erc20_allowance(pool, from, V2_ROUTER).await.unwrap_or(U256::ZERO);
                if allow < lp_amt {
                    state.toast(ToastKind::Info, "Approving LP token…", 3000);
                    if let Err(e) = approve_max(&wallet, &format!("{:#x}", pool), &router_str).await {
                        state.toast(ToastKind::Error, e, 5000);
                        set_busy.set(false);
                        return;
                    }
                }
            }

            // Estimate receive amounts for slippage
            let (min_a, min_b) = match quote_remove_liquidity(addr_a, addr_b, is_stable, lp_amt).await {
                Some((a, b)) => (apply_slippage_min(a, slip), apply_slippage_min(b, slip)),
                None => (U256::ZERO, U256::ZERO),
            };

            let result = if ta.is_native || tb.is_native {
                let (token, min_tok, min_eth) = if ta.is_native {
                    (addr_b, min_b, min_a)
                } else {
                    (addr_a, min_a, min_b)
                };
                let call = IRouter::removeLiquidityETHCall {
                    token,
                    stable: is_stable,
                    liquidity: lp_amt,
                    amountTokenMin: min_tok,
                    amountETHMin:   min_eth,
                    to: from,
                    deadline: dl,
                };
                send_transaction(&wallet, &router_hex, &call.abi_encode(), "0x0").await
            } else {
                let call = IRouter::removeLiquidityCall {
                    tokenA:     addr_a,
                    tokenB:     addr_b,
                    stable:     is_stable,
                    liquidity:  lp_amt,
                    amountAMin: min_a,
                    amountBMin: min_b,
                    to:         from,
                    deadline:   dl,
                };
                send_transaction(&wallet, &router_hex, &call.abi_encode(), "0x0").await
            };

            match result {
                Ok(hash) => {
                    state.toast(ToastKind::Info, format!("Tx submitted: {}", &hash[..10]), 4000);
                    let _ = wait_for_receipt(&hash, 60).await;
                    state.toast(ToastKind::Success, "✓ Liquidity removed!", 5000);
                    fetch_lp();
                }
                Err(e) => state.toast(ToastKind::Error, e, 7000),
            }
            set_busy.set(false);
        });
    };

    view! {
        <div class="space-y-3">
            // ── Stable / Volatile toggle ──────────────────────────────────
            <div class="flex items-center justify-between bg-gray-800/60 rounded-2xl px-4 py-3">
                <div class="text-sm text-gray-300">"Pool type"</div>
                <div class="flex items-center gap-3 text-sm">
                    <span class=move || format!("font-medium {}", if !stable.get() { "text-wind-400" } else { "text-gray-500" })>"Volatile"</span>
                    <button
                        class=move || format!("relative w-11 h-6 rounded-full transition-colors {}", if stable.get() { "bg-wind-500" } else { "bg-gray-600" })
                        on:click=move |_| { set_stable.update(|v| *v = !*v); fetch_lp(); }
                    >
                        <span class=move || format!(
                            "absolute top-1 w-4 h-4 bg-white rounded-full shadow transition-transform {}",
                            if stable.get() { "left-6" } else { "left-1" }
                        )></span>
                    </button>
                    <span class=move || format!("font-medium {}", if stable.get() { "text-wind-400" } else { "text-gray-500" })>"Stable"</span>
                </div>
            </div>

            // ── Token selectors ───────────────────────────────────────────
            <div class="grid grid-cols-2 gap-3">
                <button
                    class="flex items-center gap-2 bg-gray-800 hover:bg-gray-700 rounded-2xl px-4 py-3 transition-colors"
                    on:click=move |_| set_modal_a.set(true)
                >
                    <span class="text-2xl">{move || token_a.get().logo}</span>
                    <div class="text-left">
                        <div class="font-semibold text-sm">{move || token_a.get().symbol}</div>
                        <div class="text-xs text-gray-400">"Token A"</div>
                    </div>
                    <span class="ml-auto text-gray-400 text-xs">"▾"</span>
                </button>
                <button
                    class="flex items-center gap-2 bg-gray-800 hover:bg-gray-700 rounded-2xl px-4 py-3 transition-colors"
                    on:click=move |_| set_modal_b.set(true)
                >
                    <span class="text-2xl">{move || token_b.get().logo}</span>
                    <div class="text-left">
                        <div class="font-semibold text-sm">{move || token_b.get().symbol}</div>
                        <div class="text-xs text-gray-400">"Token B"</div>
                    </div>
                    <span class="ml-auto text-gray-400 text-xs">"▾"</span>
                </button>
            </div>

            // ── LP balance ────────────────────────────────────────────────
            <div class="bg-gray-800/60 rounded-2xl px-4 py-3 flex justify-between text-sm">
                <span class="text-gray-400">"Your LP balance"</span>
                {move || if loading.get() {
                    view! { <span class="text-gray-400 animate-pulse">"…"</span> }.into_view()
                } else {
                    let b = lp_fmt.get();
                    view! { <span class="text-white font-medium">{b}</span> }.into_view()
                }}
            </div>

            // ── Percentage selector ───────────────────────────────────────
            <div class="bg-gray-800/60 rounded-2xl p-4 space-y-3">
                <div class="flex justify-between text-sm">
                    <span class="text-gray-400">"Amount to remove"</span>
                    <span class="text-wind-400 font-bold text-lg">{move || format!("{}%", pct.get())}</span>
                </div>
                <input type="range" min="1" max="100"
                    class="w-full accent-sky-500 cursor-pointer"
                    prop:value=move || pct.get().to_string()
                    on:input=move |e| {
                        let v: u8 = event_target_value(&e).parse().unwrap_or(50);
                        set_pct.set(v);
                        update_preview();
                    }
                />
                <div class="flex gap-2">
                    {[25u8, 50, 75, 100].into_iter().map(|p| view! {
                        <button
                            class=move || format!(
                                "flex-1 py-1.5 rounded-xl text-xs font-semibold transition-colors {}",
                                if pct.get() == p { "bg-wind-500 text-white" } else { "bg-gray-700 text-gray-300 hover:bg-gray-600" }
                            )
                            on:click=move |_| { set_pct.set(p); update_preview(); }
                        >
                            {if p == 100 { "MAX".to_string() } else { format!("{}%", p) }}
                        </button>
                    }).collect_view()}
                </div>
            </div>

            // ── Receive preview ───────────────────────────────────────────
            {move || {
                let ra = recv_a.get();
                let rb = recv_b.get();
                let sym_a = token_a.get().symbol;
                let sym_b = token_b.get().symbol;
                if !ra.is_empty() {
                    view! {
                        <div class="bg-gray-800/40 rounded-2xl p-4 space-y-2">
                            <div class="text-xs text-gray-400 font-medium uppercase tracking-wide">"You will receive (estimated)"</div>
                            <div class="flex justify-between">
                                <span class="text-gray-300">{sym_a}</span>
                                <span class="text-white font-semibold">{ra}</span>
                            </div>
                            <div class="flex justify-between">
                                <span class="text-gray-300">{sym_b}</span>
                                <span class="text-white font-semibold">{rb}</span>
                            </div>
                        </div>
                    }.into_view()
                } else { ().into_view() }
            }}

            <button
                class="btn-primary w-full"
                prop:disabled=move || busy.get() || lp_balance.get().is_zero()
                on:click=do_remove
            >
                {move || if busy.get() { "Removing…" } else { "Remove Liquidity" }}
            </button>
        </div>

        {move || modal_a.get().then(|| view! {
            <TokenSelectorModal
                selected=Signal::from(token_a)
                on_select=move |t| { set_token_a.set(t); set_modal_a.set(false); fetch_lp(); }
                on_close=move || set_modal_a.set(false)
            />
        })}
        {move || modal_b.get().then(|| view! {
            <TokenSelectorModal
                selected=Signal::from(token_b)
                on_select=move |t| { set_token_b.set(t); set_modal_b.set(false); fetch_lp(); }
                on_close=move || set_modal_b.set(false)
            />
        })}
    }
}

// ─── Positions ────────────────────────────────────────────────────────────────

#[component]
fn PositionsPanel() -> impl IntoView {
    let state = use_app_state();

    let positions: RwSignal<Vec<PoolPosition>> = create_rw_signal(vec![]);
    let loading = create_rw_signal(true);

    create_effect(move |_| {
        if let Some(addr) = state.wallet_address.get() {
            let from: Address = addr.parse().unwrap_or(Address::ZERO);
            loading.set(true);
            spawn_local(async move {
                let pos = fetch_user_positions(from).await;
                positions.set(pos);
                loading.set(false);
            });
        } else {
            positions.set(vec![]);
            loading.set(false);
        }
    });

    view! {
        <div class="space-y-4">
            {move || {
                if loading.get() {
                    view! {
                        <div class="flex flex-col items-center py-16 gap-4">
                            <div class="w-8 h-8 border-2 border-wind-500 border-t-transparent rounded-full animate-spin"></div>
                            <p class="text-gray-400 text-sm">"Scanning your LP positions…"</p>
                        </div>
                    }.into_view()
                } else if state.wallet_address.get().is_none() {
                    view! {
                        <div class="text-center py-16">
                            <div class="text-5xl mb-4">"🔗"</div>
                            <p class="text-gray-400">"Connect your wallet to see positions"</p>
                        </div>
                    }.into_view()
                } else {
                    let pos = positions.get();
                    if pos.is_empty() {
                        view! {
                            <div class="text-center py-16">
                                <div class="text-5xl mb-4">"🌊"</div>
                                <p class="text-white font-semibold">"No LP positions found"</p>
                                <p class="text-gray-400 text-sm mt-1">"Add liquidity to start earning fees"</p>
                            </div>
                        }.into_view()
                    } else {
                        pos.into_iter().map(|p| {
                            let sym_a = p.token_a_sym;
                            let sym_b = p.token_b_sym;
                            let stable_label = if p.stable { "Stable" } else { "Volatile" };
                            let badge_cls = if p.stable {
                                "bg-purple-900/40 text-purple-300 border border-purple-700/40"
                            } else {
                                "bg-blue-900/40 text-blue-300 border border-blue-700/40"
                            };
                            let share = format!("{:.4}%", p.share_pct);
                            let lp = p.lp_fmt.clone();
                            view! {
                                <div class="bg-gray-800/60 hover:bg-gray-800 rounded-2xl p-4 transition-colors border border-gray-700/40 space-y-3">
                                    <div class="flex items-center justify-between">
                                        <div class="flex items-center gap-3">
                                            <div class="flex -space-x-2">
                                                <div class="w-8 h-8 rounded-full bg-gray-700 flex items-center justify-center text-base ring-2 ring-gray-800">
                                                    {crate::config::TOKENS.iter().find(|t| t.symbol == sym_a).map(|t| t.logo).unwrap_or("🪙")}
                                                </div>
                                                <div class="w-8 h-8 rounded-full bg-gray-700 flex items-center justify-center text-base ring-2 ring-gray-800">
                                                    {crate::config::TOKENS.iter().find(|t| t.symbol == sym_b).map(|t| t.logo).unwrap_or("🪙")}
                                                </div>
                                            </div>
                                            <div>
                                                <div class="font-semibold text-white">{format!("{sym_a} / {sym_b}")}</div>
                                                <span class=format!("text-xs px-2 py-0.5 rounded-full {badge_cls}")>{stable_label}</span>
                                            </div>
                                        </div>
                                    </div>
                                    <div class="grid grid-cols-2 gap-2 text-sm">
                                        <div class="bg-gray-900/50 rounded-xl px-3 py-2">
                                            <div class="text-gray-400 text-xs">"LP Tokens"</div>
                                            <div class="text-white font-medium">{lp}</div>
                                        </div>
                                        <div class="bg-gray-900/50 rounded-xl px-3 py-2">
                                            <div class="text-gray-400 text-xs">"Pool Share"</div>
                                            <div class="text-wind-400 font-medium">{share}</div>
                                        </div>
                                    </div>
                                </div>
                            }
                        }).collect_view().into_view()
                    }
                }
            }}
        </div>
    }
}

// ─── Shared sub-components ────────────────────────────────────────────────────

#[component]
fn TokenAmountInput(
    label: &'static str,
    token: Signal<Token>,
    amount: Signal<String>,
    on_token_click: impl Fn() + 'static,
    on_input: impl Fn(web_sys::Event) + 'static,
    on_input_done: impl Fn() + 'static,
) -> impl IntoView {
    view! {
        <div class="bg-gray-800/60 rounded-2xl p-4 space-y-2 border border-gray-700/30 hover:border-gray-600/50 transition-colors">
            <div class="flex items-center justify-between text-xs text-gray-400">
                <span>{label}</span>
            </div>
            <div class="flex items-center gap-3">
                <button
                    class="flex items-center gap-2 bg-gray-700 hover:bg-gray-600 rounded-xl px-3 py-2.5 shrink-0 transition-colors"
                    on:click=move |_| on_token_click()
                >
                    <span class="text-xl">{move || token.get().logo}</span>
                    <span class="font-semibold text-sm text-white">{move || token.get().symbol}</span>
                    <span class="text-xs text-gray-400">"▾"</span>
                </button>
                <input
                    type="number"
                    placeholder="0.0"
                    class="flex-1 bg-transparent text-right text-xl font-medium text-white placeholder-gray-600 outline-none [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none"
                    prop:value=amount
                    on:input=on_input
                    on:change=move |_| on_input_done()
                />
            </div>
        </div>
    }
}

// ─── Utility ──────────────────────────────────────────────────────────────────

fn trim_fmt(s: &str) -> String {
    if s.contains('.') {
        let trimmed = s.trim_end_matches('0').trim_end_matches('.');
        if trimmed.is_empty() { "0".to_string() } else { trimmed.to_string() }
    } else {
        s.to_string()
    }
}
