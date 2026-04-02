//! Premium swap interface — KyberSwap/1inch quality.
//!
//! Features:
//!  • Smart multi-source router (V2 volatile/stable, V3 all tick spacings, 2-hop)
//!  • Parallel quote fetching (all sources race, best wins)
//!  • Price impact badge (green / yellow / red)
//!  • Exchange rate with invert button
//!  • MAX button
//!  • USD value estimates
//!  • Auto-refresh countdown (every 15 s)
//!  • Expandable "Swap Details" panel
//!  • Transaction confirmation with explorer link
//!
//! Leptos 0.6, stable Rust: all WriteSignal calls use .set() / .update()

use alloy_primitives::Address;
use leptos::*;

use crate::{
    blockchain::{
        apply_slippage_min, deadline_secs, find_best_route,
        fmt_units, format_rate, get_erc20_allowance,
        get_erc20_balance, get_eth_balance, get_eth_usd_price,
        parse_token_amount, token_usd_value, SmartRoute,
        IRouter, ISwapRouter, ExactInputSingleParams, Route,
    },
    components::token_selector::TokenSelectorModal,
    config::{Token, TOKENS, V2_ROUTER, V2_FACTORY, CL_SWAP_ROUTER, WETH},
    state::{use_app_state, ToastKind},
    wallet::{approve_max, send_transaction},
};
use alloy_sol_types::SolCall;

// ─── Page shell ───────────────────────────────────────────────────────────────

#[component]
pub fn SwapPage() -> impl IntoView {
    view! {
        <main class="max-w-lg mx-auto px-4 py-10">
            <SwapInterface/>
        </main>
    }
}

// ─── Main swap component ──────────────────────────────────────────────────────

#[component]
pub fn SwapInterface() -> impl IntoView {
    let state = use_app_state();

    // ── Core state ────────────────────────────────────────────────────────────
    let (token_in,  set_token_in)  = create_signal(TOKENS[0].clone()); // ETH
    let (token_out, set_token_out) = create_signal(TOKENS[3].clone()); // USDC
    let (amount_in_str, set_amount_in_str) = create_signal(String::new());

    // ── Settings ──────────────────────────────────────────────────────────────
    let (slippage,     set_slippage)     = create_signal(0.5_f64);
    let (deadline_min, set_deadline_min) = create_signal(30_u64);
    let (show_settings, set_show_settings) = create_signal(false);
    let (show_details, set_show_details)   = create_signal(false);

    // ── UI state ──────────────────────────────────────────────────────────────
    let (modal_for_in,  set_modal_for_in)  = create_signal(false);
    let (modal_for_out, set_modal_for_out) = create_signal(false);
    let (is_quoting,    set_is_quoting)    = create_signal(false);
    let (is_approving,  set_is_approving)  = create_signal(false);
    let (is_swapping,   set_is_swapping)   = create_signal(false);
    let (rate_inverted, set_rate_inverted) = create_signal(false);
    let (needs_approval, set_needs_approval) = create_signal(false);

    // ── Quote + balances ──────────────────────────────────────────────────────
    let (route,       set_route)      = create_signal(Option::<SmartRoute>::None);
    let (balance_in,  set_balance_in) = create_signal(String::new());
    let (balance_out, set_balance_out)= create_signal(String::new());
    let (usd_in,      set_usd_in)     = create_signal(Option::<String>::None);
    let (usd_out,     set_usd_out)    = create_signal(Option::<String>::None);

    // ── Auto-refresh countdown ────────────────────────────────────────────────
    // Counts down 15 → 0; when it hits 0 the quote effect runs again.
    let (countdown,    set_countdown)    = create_signal(15u32);
    let (refresh_tick, set_refresh_tick) = create_signal(0u32); // bumped on expiry

    // Start 1-second interval ticker
    {
        let interval = gloo_timers::callback::Interval::new(1_000, move || {
            set_countdown.update(|v| {
                if *v == 0 {
                    set_refresh_tick.update(|t| *t += 1);
                    *v = 15;
                } else {
                    *v -= 1;
                }
            });
        });
        interval.forget();
    }

    // ── Balance refresh ───────────────────────────────────────────────────────
    let refresh_balances = move || {
        if let Some(addr_str) = state.wallet_address.get_untracked() {
            if let Ok(addr) = addr_str.parse::<Address>() {
                let ti  = token_in.get_untracked();
                let to_ = token_out.get_untracked();
                spawn_local(async move {
                    let b_in = if ti.is_native {
                        get_eth_balance(addr).await.map(|v| fmt_units(v, 18)).unwrap_or_default()
                    } else {
                        get_erc20_balance(ti.address, addr).await
                            .map(|v| fmt_units(v, ti.decimals)).unwrap_or_default()
                    };
                    let b_out = if to_.is_native {
                        get_eth_balance(addr).await.map(|v| fmt_units(v, 18)).unwrap_or_default()
                    } else {
                        get_erc20_balance(to_.address, addr).await
                            .map(|v| fmt_units(v, to_.decimals)).unwrap_or_default()
                    };
                    set_balance_in.set(b_in);
                    set_balance_out.set(b_out);
                });
            }
        }
    };

    create_effect(move |_| {
        let _ = state.wallet_address.get();
        let _ = token_in.get();
        let _ = token_out.get();
        refresh_balances();
    });

    // ── Smart quote effect (debounced + auto-refresh) ─────────────────────────
    create_effect(move |_| {
        let amt   = amount_in_str.get();
        let ti    = token_in.get();
        let to_   = token_out.get();
        let slip  = slippage.get();
        let _tick = refresh_tick.get(); // subscribe so we re-run on auto-refresh

        if amt.is_empty() || amt == "0" {
            set_route.set(None);
            set_usd_in.set(None);
            set_usd_out.set(None);
            return;
        }

        set_is_quoting.set(true);

        spawn_local(async move {
            // 400 ms debounce
            gloo_timers::future::TimeoutFuture::new(400).await;
            if amount_in_str.get_untracked() != amt { set_is_quoting.set(false); return; }

            let amount_raw = parse_token_amount(&amt, ti.decimals);
            if amount_raw.is_zero() { set_is_quoting.set(false); return; }

            let in_addr  = if ti.is_native  { WETH } else { ti.address };
            let out_addr = if to_.is_native { WETH } else { to_.address };

            // ── USD value for input ───────────────────────────────────────────
            let usd_in_val = token_usd_value(amount_raw, in_addr, ti.decimals).await;
            set_usd_in.set(usd_in_val);

            // ── Run smart router (all sources in parallel) ────────────────────
            match find_best_route(
                in_addr, out_addr, amount_raw, slip,
                ti.symbol, to_.symbol, to_.decimals,
            ).await {
                Ok(best) => {
                    // USD value for output
                    let usd_out_val = token_usd_value(best.amount_out, out_addr, to_.decimals).await;
                    set_usd_out.set(usd_out_val);
                    set_route.set(Some(best));
                }
                Err(e) => {
                    log::warn!("Router: {e}");
                    set_route.set(None);
                    set_usd_out.set(None);
                }
            }
            set_is_quoting.set(false);

            // ── Allowance check ───────────────────────────────────────────────
            if !ti.is_native {
                if let Some(wallet) = state.wallet_address.get_untracked() {
                    if let Ok(addr) = wallet.parse::<Address>() {
                        let spender = if matches!(route.get_untracked(), Some(ref r) if matches!(r.kind, crate::blockchain::RouteKind::V3 { .. })) {
                            CL_SWAP_ROUTER
                        } else { V2_ROUTER };
                        if let Ok(allow) = get_erc20_allowance(ti.address, addr, spender).await {
                            set_needs_approval.set(allow < amount_raw);
                        }
                    }
                }
            } else {
                set_needs_approval.set(false);
            }
        });
    });

    // ── Flip tokens ───────────────────────────────────────────────────────────
    let flip = move |_| {
        let old_in  = token_in.get_untracked();
        let old_out = token_out.get_untracked();
        set_token_in.set(old_out);
        set_token_out.set(old_in);
        set_amount_in_str.set(String::new());
        set_route.set(None);
        set_usd_in.set(None);
        set_usd_out.set(None);
    };

    // ── MAX button ────────────────────────────────────────────────────────────
    let click_max = move |_| {
        let b = balance_in.get_untracked();
        if !b.is_empty() {
            // Leave a tiny buffer for gas if native ETH
            let val = if token_in.get_untracked().is_native {
                let b_f: f64 = b.parse().unwrap_or(0.0);
                let safe = (b_f - 0.002).max(0.0);
                format!("{:.6}", safe)
            } else { b };
            set_amount_in_str.set(val);
        }
    };

    // ── Approve ───────────────────────────────────────────────────────────────
    let do_approve = move |_| {
        let ti = token_in.get_untracked();
        let spender = if matches!(route.get_untracked(), Some(ref r) if matches!(r.kind, crate::blockchain::RouteKind::V3 { .. })) {
            CL_SWAP_ROUTER
        } else { V2_ROUTER };

        if let Some(wallet) = state.wallet_address.get_untracked() {
            set_is_approving.set(true);
            let state = state;
            spawn_local(async move {
                match approve_max(&wallet, &format!("{:#x}", ti.address), &format!("{:#x}", spender)).await {
                    Ok(hash) => {
                        state.toast(ToastKind::Info, format!("Approval sent: {}…", &hash[..10]), 4000);
                        let _ = crate::wallet::wait_for_receipt(&hash, 30).await;
                        set_needs_approval.set(false);
                        state.toast(ToastKind::Success, "Token approved!", 3000);
                    }
                    Err(e) => state.toast(ToastKind::Error, e, 5000),
                }
                set_is_approving.set(false);
            });
        }
    };

    // ── Execute Swap ──────────────────────────────────────────────────────────
    let do_swap = move |_| {
        let ti      = token_in.get_untracked();
        let to_     = token_out.get_untracked();
        let amt_str = amount_in_str.get_untracked();
        let r       = match route.get_untracked() { Some(r) => r, None => return };
        let dl_min  = deadline_min.get_untracked();

        let wallet = match state.wallet_address.get_untracked() {
            Some(a) => a,
            None    => { state.toast(ToastKind::Error, "Wallet not connected", 3000); return }
        };

        set_is_swapping.set(true);
        let state = state;

        spawn_local(async move {
            let in_raw   = parse_token_amount(&amt_str, ti.decimals);
            let out_min  = r.min_received;
            let dl       = alloy_primitives::U256::from(deadline_secs(dl_min));
            let recipient: Address = wallet.parse().unwrap_or(Address::ZERO);

            let in_addr  = if ti.is_native  { WETH } else { ti.address };
            let out_addr = if to_.is_native { WETH } else { to_.address };

            let result = match &r.kind {
                crate::blockchain::RouteKind::V3 { tick_spacing } => {
                    let ts24 = alloy_primitives::Signed::<24, 1>::try_from(*tick_spacing as i128)
                        .unwrap_or(alloy_primitives::Signed::ZERO);
                    let params = ExactInputSingleParams {
                        tokenIn: in_addr, tokenOut: out_addr,
                        tickSpacing: ts24, recipient,
                        deadline: dl, amountIn: in_raw,
                        amountOutMinimum: out_min,
                        sqrtPriceLimitX96: alloy_primitives::Uint::ZERO,
                    };
                    let cd    = ISwapRouter::exactInputSingleCall { params }.abi_encode();
                    let value = if ti.is_native { format!("0x{:x}", in_raw) } else { "0x0".into() };
                    send_transaction(&wallet, &format!("{:#x}", CL_SWAP_ROUTER), &cd, &value).await
                }

                crate::blockchain::RouteKind::MultiHopWeth => {
                    // 2-hop via WETH: tokenIn → WETH → tokenOut
                    let routes = vec![
                        Route { from: in_addr, to: WETH,     stable: false, factory: V2_FACTORY },
                        Route { from: WETH,    to: out_addr, stable: false, factory: V2_FACTORY },
                    ];
                    exec_v2_swap(&wallet, ti.is_native, in_raw, out_min, routes, recipient, dl).await
                }

                crate::blockchain::RouteKind::MultiHopUsdc => {
                    let usdc = crate::config::USDC;
                    let routes = vec![
                        Route { from: in_addr, to: usdc,     stable: false, factory: V2_FACTORY },
                        Route { from: usdc,    to: out_addr, stable: false, factory: V2_FACTORY },
                    ];
                    exec_v2_swap(&wallet, ti.is_native, in_raw, out_min, routes, recipient, dl).await
                }

                kind => {
                    let stable = matches!(kind, crate::blockchain::RouteKind::V2Stable);
                    let routes = vec![Route { from: in_addr, to: out_addr, stable, factory: V2_FACTORY }];
                    exec_v2_swap(&wallet, ti.is_native, in_raw, out_min, routes, recipient, dl).await
                }
            };

            match result {
                Ok(hash) => {
                    let short = format!("{}…{}", &hash[..10], &hash[hash.len()-4..]);
                    state.toast(ToastKind::Info,
                        format!("Swap submitted ↗ basescan.org/tx/{short}"), 5000);
                    let _ = crate::wallet::wait_for_receipt(&hash, 30).await;
                    state.toast(ToastKind::Success, "✓ Swap confirmed!", 4000);
                    set_amount_in_str.set(String::new());
                    set_route.set(None);
                    set_usd_in.set(None);
                    set_usd_out.set(None);
                    refresh_balances();
                }
                Err(e) => state.toast(ToastKind::Error, e, 7000),
            }
            set_is_swapping.set(false);
        });
    };

    // ─────────────────────────────────────────────────────────────────────────
    // VIEW
    // ─────────────────────────────────────────────────────────────────────────
    view! {
        <div class="space-y-3">

        // ── Swap card ─────────────────────────────────────────────────────────
        <div class="card p-5 space-y-3">

            // Title row
            <div class="flex items-center justify-between">
                <h2 class="text-lg font-bold tracking-tight">"Swap"</h2>
                <button
                    on:click=move |_| set_show_settings.update(|v| *v = !*v)
                    class=move || format!(
                        "p-2 rounded-lg transition-colors {}",
                        if show_settings.get() { "bg-wind-500/20 text-wind-400" }
                        else { "text-gray-400 hover:text-white" }
                    )
                >"⚙"</button>
            </div>

            // Settings panel
            {move || show_settings.get().then(|| view! {
                <div class="bg-gray-800/80 rounded-2xl p-4 space-y-4 text-sm border border-gray-700/50">
                    <div>
                        <p class="text-gray-400 mb-2 font-medium">"Slippage tolerance"</p>
                        <div class="flex gap-2 flex-wrap">
                            {[0.1f64, 0.5, 1.0, 3.0, 5.0].iter().enumerate().map(|(_, &preset)| {
                                view! {
                                    <button
                                        class=move || format!(
                                            "px-3 py-1.5 rounded-xl text-xs font-semibold transition-all {}",
                                            if (slippage.get() - preset).abs() < 0.001
                                                { "bg-wind-500 text-white shadow-wind-500/30 shadow-md" }
                                            else
                                                { "bg-gray-700 hover:bg-gray-600 text-gray-300" }
                                        )
                                        on:click=move |_| set_slippage.set(preset)
                                    >
                                        {format!("{preset}%")}
                                    </button>
                                }
                            }).collect_view()}
                        </div>
                    </div>
                    <div class="flex items-center justify-between">
                        <p class="text-gray-400 font-medium">"Tx deadline"</p>
                        <div class="flex items-center gap-2">
                            <input type="number" min="1" max="1440"
                                class="w-16 bg-gray-700 rounded-lg px-3 py-1.5 text-right
                                       text-sm outline-none focus:ring-1 focus:ring-wind-500"
                                prop:value=deadline_min
                                on:input=move |e| {
                                    if let Ok(v) = event_target_value(&e).parse::<u64>() {
                                        set_deadline_min.set(v.clamp(1, 1440));
                                    }
                                }
                            />
                            <span class="text-gray-500">"min"</span>
                        </div>
                    </div>
                </div>
            })}

            // ── Token-In box ──────────────────────────────────────────────────
            <div class="bg-gray-800/60 rounded-2xl p-4 border border-gray-700/30
                        focus-within:border-wind-500/50 transition-colors">
                <div class="flex justify-between items-center mb-3">
                    <span class="text-xs font-medium text-gray-400">"You pay"</span>
                    <div class="flex items-center gap-2">
                        <span class="text-xs text-gray-500">
                            "Balance: " {move || {
                                let b = balance_in.get();
                                if b.is_empty() { "—".to_string() }
                                else { truncate_decimals(&b, 6) }
                            }}
                        </span>
                        <button
                            on:click=click_max
                            class="text-xs font-bold text-wind-400 hover:text-wind-300
                                   bg-wind-400/10 hover:bg-wind-400/20 px-2 py-0.5
                                   rounded-md transition-colors"
                        >"MAX"</button>
                    </div>
                </div>
                <div class="flex items-center gap-3">
                    <button
                        on:click=move |_| set_modal_for_in.set(true)
                        class="flex items-center gap-2 bg-gray-700 hover:bg-gray-600
                               rounded-xl px-3 py-2.5 shrink-0 transition-colors
                               border border-gray-600/50"
                    >
                        <span class="text-2xl leading-none">{move || token_in.get().logo}</span>
                        <span class="font-bold text-sm">{move || token_in.get().symbol}</span>
                        <span class="text-gray-400 text-xs ml-0.5">"▾"</span>
                    </button>
                    <input
                        type="number" placeholder="0.0"
                        class="bg-transparent text-3xl font-semibold outline-none w-full
                               text-right placeholder-gray-700"
                        prop:value=amount_in_str
                        on:input=move |e| set_amount_in_str.set(event_target_value(&e))
                    />
                </div>
                {move || usd_in.get().map(|v| view! {
                    <p class="text-xs text-gray-500 text-right mt-1">{v}</p>
                })}
            </div>

            // ── Flip button ───────────────────────────────────────────────────
            <div class="flex justify-center -my-1 relative z-10">
                <button
                    on:click=flip
                    class="bg-gray-800 hover:bg-gray-700 border-2 border-gray-900
                           p-2.5 rounded-xl text-gray-300 hover:text-wind-400
                           transition-all hover:rotate-180 duration-300"
                >"⇅"</button>
            </div>

            // ── Token-Out box ─────────────────────────────────────────────────
            <div class="bg-gray-800/60 rounded-2xl p-4 border border-gray-700/30">
                <div class="flex justify-between items-center mb-3">
                    <span class="text-xs font-medium text-gray-400">"You receive"</span>
                    <span class="text-xs text-gray-500">
                        "Balance: " {move || {
                            let b = balance_out.get();
                            if b.is_empty() { "—".to_string() }
                            else { truncate_decimals(&b, 6) }
                        }}
                    </span>
                </div>
                <div class="flex items-center gap-3">
                    <button
                        on:click=move |_| set_modal_for_out.set(true)
                        class="flex items-center gap-2 bg-gray-700 hover:bg-gray-600
                               rounded-xl px-3 py-2.5 shrink-0 transition-colors
                               border border-gray-600/50"
                    >
                        <span class="text-2xl leading-none">{move || token_out.get().logo}</span>
                        <span class="font-bold text-sm">{move || token_out.get().symbol}</span>
                        <span class="text-gray-400 text-xs ml-0.5">"▾"</span>
                    </button>
                    <div class="flex-1 text-right">
                        {move || {
                            if is_quoting.get() {
                                view! {
                                    <div class="flex justify-end items-center gap-1">
                                        <span class="w-2 h-2 bg-wind-400 rounded-full animate-pulse"></span>
                                        <span class="text-gray-500 text-sm">"Fetching…"</span>
                                    </div>
                                }.into_view()
                            } else if let Some(ref r) = route.get() {
                                view! {
                                    <span class="text-3xl font-semibold text-white">
                                        {r.amount_out_fmt.clone()}
                                    </span>
                                }.into_view()
                            } else {
                                view! {
                                    <span class="text-3xl font-semibold text-gray-700">"0.0"</span>
                                }.into_view()
                            }
                        }}
                        {move || usd_out.get().map(|v| view! {
                            <p class="text-xs text-gray-500 mt-1">{v}</p>
                        })}
                    </div>
                </div>
            </div>

            // ── Rate / route info bar ─────────────────────────────────────────
            {move || route.get().map(|r| {
                let in_sym  = token_in.get_untracked().symbol;
                let out_sym = token_out.get_untracked().symbol;
                let in_dec  = token_in.get_untracked().decimals;
                let out_dec = token_out.get_untracked().decimals;
                let amt_in  = parse_token_amount(&amount_in_str.get_untracked(), in_dec);
                let rate    = format_rate(amt_in, r.amount_out, in_sym, out_sym, in_dec, out_dec, rate_inverted.get());
                let badge   = r.kind.badge();
                let label   = r.kind.label();
                let impact_bg = r.impact_bg();
                let impact_pct = r.price_impact_pct();

                view! {
                    <div class="bg-gray-800/40 rounded-xl px-4 py-3 space-y-2 border border-gray-700/30">
                        // Rate row
                        <div class="flex items-center justify-between gap-2">
                            <div class="flex items-center gap-2">
                                <span class="text-sm text-gray-200 font-medium">{rate}</span>
                                <button
                                    on:click=move |_| set_rate_inverted.update(|v| *v = !*v)
                                    class="text-gray-500 hover:text-gray-300 text-xs transition-colors"
                                    title="Invert rate"
                                >"↔"</button>
                            </div>
                            // Price impact badge
                            <span class=format!("text-xs font-semibold px-2 py-0.5 rounded-full {impact_bg}")>
                                {format!("{:.2}% impact", impact_pct)}
                            </span>
                        </div>
                        // Route + countdown row
                        <div class="flex items-center justify-between">
                            <div class="flex items-center gap-2">
                                <span class="text-xs font-bold px-1.5 py-0.5 rounded bg-wind-500/20 text-wind-300">
                                    {badge}
                                </span>
                                <span class="text-xs text-gray-500">{label}</span>
                                // Path
                                <span class="text-xs text-gray-600">
                                    {r.path_labels.join(" ")}
                                </span>
                            </div>
                            // Countdown
                            <div class="flex items-center gap-1.5">
                                <span class="text-xs text-gray-600">
                                    {move || format!("{}s", countdown.get())}
                                </span>
                                // Progress dots
                                <div class="flex gap-0.5">
                                    {(0..15u32).map(|i| {
                                        view! {
                                            <span class=move || format!(
                                                "w-1 h-1 rounded-full {}",
                                                if i < countdown.get() { "bg-wind-500" } else { "bg-gray-700" }
                                            )></span>
                                        }
                                    }).collect_view()}
                                </div>
                            </div>
                        </div>
                    </div>
                }
            })}

            // ── Swap details (expandable) ─────────────────────────────────────
            {move || route.get().map(|r| {
                let slip = slippage.get();
                view! {
                    <div>
                        <button
                            on:click=move |_| set_show_details.update(|v| *v = !*v)
                            class="w-full flex items-center justify-between text-xs
                                   text-gray-500 hover:text-gray-300 transition-colors py-1"
                        >
                            <span>"Swap details"</span>
                            <span>{move || if show_details.get() { "▲" } else { "▼" }}</span>
                        </button>

                        {move || show_details.get().then(|| {
                            // Pre-extract all owned values from `r` so the view! macro
                            // closures each capture independent Strings (no partial moves).
                            let out_sym      = token_out.get_untracked().symbol;
                            let out_fmt      = format!("{} {}", r.amount_out_fmt, out_sym);
                            let min_fmt      = format!("{} {}", r.min_received_fmt, out_sym);
                            let impact_class = r.impact_color();
                            let impact_fmt   = format!("{:.2}%", r.price_impact_pct());
                            let slip_fmt     = format!("{slip}%");
                            let gas_fmt      = (r.gas_estimate > 0)
                                .then(|| format!("{} units", r.gas_estimate));
                            let kind_label   = r.kind.label();

                            view! {
                                <div class="mt-2 bg-gray-800/40 rounded-xl p-4 space-y-2.5
                                            text-sm border border-gray-700/30 animate-fade-in">
                                    <DetailRow label="Expected output">
                                        <span class="font-semibold text-white">{out_fmt}</span>
                                    </DetailRow>
                                    <DetailRow label="Minimum received">
                                        <span class="text-gray-300">{min_fmt}</span>
                                    </DetailRow>
                                    <DetailRow label="Price impact">
                                        <span class=impact_class>{impact_fmt}</span>
                                    </DetailRow>
                                    <DetailRow label="Slippage tolerance">
                                        <span class="text-gray-300">{slip_fmt}</span>
                                    </DetailRow>
                                    {gas_fmt.map(|g| view! {
                                        <DetailRow label="Est. gas">
                                            <span class="text-gray-300">{g}</span>
                                        </DetailRow>
                                    })}
                                    <DetailRow label="Route">
                                        <span class="text-wind-300 font-medium">{kind_label}</span>
                                    </DetailRow>
                                </div>
                            }
                        })}
                    </div>
                }
            })}

            // ── Action button ─────────────────────────────────────────────────
            {move || {
                let connected  = state.is_connected();
                let has_amount = !amount_in_str.get().is_empty();
                let has_route  = route.get().is_some();

                if !connected {
                    view! {
                        <button class="btn-primary w-full py-4 text-base"
                            on:click=move |_| {
                                let s = state;
                                spawn_local(async move {
                                    match crate::wallet::request_accounts().await {
                                        Ok(a)  => { s.wallet_address.set(Some(a)); }
                                        Err(e) => s.toast(ToastKind::Error, e, 5000),
                                    }
                                });
                            }
                        >"Connect Wallet"</button>
                    }.into_view()
                } else if !state.is_correct_network() {
                    view! {
                        <button class="btn-primary w-full py-4 bg-red-500 hover:bg-red-600"
                            on:click=move |_| {
                                spawn_local(async { let _ = crate::wallet::switch_to_base().await; });
                            }
                        >"Switch to Base"</button>
                    }.into_view()
                } else if has_amount && !has_route && !is_quoting.get() {
                    view! {
                        <button class="btn-primary w-full py-4 opacity-50 cursor-not-allowed" disabled=true>
                            "No liquidity found"
                        </button>
                    }.into_view()
                } else if is_quoting.get() {
                    view! {
                        <button class="btn-primary w-full py-4 opacity-70 cursor-wait" disabled=true>
                            <span class="animate-pulse">"Finding best route…"</span>
                        </button>
                    }.into_view()
                } else if has_amount && has_route && needs_approval.get() {
                    view! {
                        <button class="btn-primary w-full py-4"
                            prop:disabled=is_approving
                            on:click=do_approve
                        >
                            {move || if is_approving.get() {
                                "Approving…".to_string()
                            } else {
                                format!("Approve {}", token_in.get().symbol)
                            }}
                        </button>
                    }.into_view()
                } else {
                    view! {
                        <button
                            class=move || format!(
                                "w-full py-4 rounded-xl font-bold text-base transition-all \
                                 disabled:opacity-50 disabled:cursor-not-allowed {}",
                                if has_amount && has_route
                                    { "bg-gradient-to-r from-wind-500 to-wind-600 hover:from-wind-400 hover:to-wind-500 text-white shadow-lg shadow-wind-500/20" }
                                else
                                    { "bg-gray-700 text-gray-500 cursor-not-allowed" }
                            )
                            prop:disabled=move || is_swapping.get() || !has_amount || !has_route
                            on:click=do_swap
                        >
                            {move || {
                                if is_swapping.get() {
                                    "Confirming in wallet…".to_string()
                                } else if !has_amount {
                                    "Enter an amount".to_string()
                                } else {
                                    format!("Swap {} → {}",
                                        token_in.get().symbol, token_out.get().symbol)
                                }
                            }}
                        </button>
                    }.into_view()
                }
            }}

        </div>
        // ── End card ──────────────────────────────────────────────────────────

        // Token selector modals
        {move || modal_for_in.get().then(|| view! {
            <TokenSelectorModal
                selected=Signal::from(token_in)
                on_select=move |t| set_token_in.set(t)
                on_close=move || set_modal_for_in.set(false)
            />
        })}
        {move || modal_for_out.get().then(|| view! {
            <TokenSelectorModal
                selected=Signal::from(token_out)
                on_select=move |t| set_token_out.set(t)
                on_close=move || set_modal_for_out.set(false)
            />
        })}

        </div> // end space-y-3
    }
}

// ─── DetailRow helper ─────────────────────────────────────────────────────────

#[component]
fn DetailRow(label: &'static str, children: Children) -> impl IntoView {
    view! {
        <div class="flex items-center justify-between">
            <span class="text-gray-500">{label}</span>
            {children()}
        </div>
    }
}

// ─── V2 swap execution helper ─────────────────────────────────────────────────

async fn exec_v2_swap(
    wallet:    &str,
    is_eth_in: bool,
    in_raw:    alloy_primitives::U256,
    out_min:   alloy_primitives::U256,
    routes:    Vec<Route>,
    recipient: Address,
    deadline:  alloy_primitives::U256,
) -> Result<String, String> {
    let router_hex = format!("{:#x}", V2_ROUTER);
    if is_eth_in {
        let call = IRouter::swapExactETHForTokensCall {
            amountOutMin: out_min, routes, to: recipient, deadline,
        };
        let value = format!("0x{:x}", in_raw);
        send_transaction(wallet, &router_hex, &call.abi_encode(), &value).await
    } else {
        let call = IRouter::swapExactTokensForTokensCall {
            amountIn: in_raw, amountOutMin: out_min, routes, to: recipient, deadline,
        };
        send_transaction(wallet, &router_hex, &call.abi_encode(), "0x0").await
    }
}

// ─── Display helpers ──────────────────────────────────────────────────────────

fn truncate_decimals(s: &str, places: usize) -> String {
    match s.find('.') {
        None    => s.to_string(),
        Some(i) => {
            let end = (i + 1 + places).min(s.len());
            s[..end].trim_end_matches('0').trim_end_matches('.').to_string()
        }
    }
}
