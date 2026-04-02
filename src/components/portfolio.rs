//! Portfolio page — view positions, balances, veNFTs.

use leptos::*;
use alloy_primitives::U256;
use crate::state::use_app_state;
use crate::config::TOKENS;
use crate::blockchain::{get_erc20_balance, fmt_units, get_eth_balance};
use crate::subgraph;

fn tab_btn(active: bool) -> &'static str {
    if active {
        "px-3 py-1.5 rounded-lg text-xs font-medium bg-cyan-500 text-white"
    } else {
        "px-3 py-1.5 rounded-lg text-xs font-medium text-gray-400 bg-white/5"
    }
}

#[component]
pub fn PortfolioPage() -> impl IntoView {
    let state = use_app_state();
    let (tab, set_tab) = create_signal(0u8);
    let (positions, set_positions) = create_signal(Vec::<subgraph::UserPosition>::new());
    let (venfts, set_venfts) = create_signal(Vec::<subgraph::VeNFTInfo>::new());
    let (balances, set_balances) = create_signal(Vec::<(String, String)>::new());

    let addr_signal = state.wallet_address;
    spawn_local(async move {
        if let Some(addr) = addr_signal.get() {
            if let Ok(addr_parsed) = addr.parse::<alloy_primitives::Address>() {
                let _ = subgraph::fetch_user_positions(&addr).await.map(|p| set_positions.set(p));
                let _ = subgraph::fetch_venfts(&addr).await.map(|n| set_venfts.set(n));
                let mut bl = vec![];
                if let Ok(eth_bal) = get_eth_balance(addr_parsed).await {
                    bl.push(("ETH".into(), fmt_units(eth_bal, 18)));
                }
                for token in TOKENS.iter().filter(|t| !t.is_native).take(8) {
                    if let Ok(bal) = get_erc20_balance(token.address, addr_parsed).await {
                        let fmt = fmt_units(bal, token.decimals);
                        if fmt.parse::<f64>().unwrap_or(0.0) > 0.001 {
                            bl.push((token.symbol.into(), fmt));
                        }
                    }
                }
                set_balances.set(bl);
            }
        }
    });

    let total_locked = move || {
        venfts.get().iter().fold(U256::ZERO, |a, n| a + n.locked_amount)
    };
    let total_vp = move || {
        venfts.get().iter().fold(U256::ZERO, |a, n| a + n.voting_power)
    };

    view! {
        <main class="max-w-2xl mx-auto px-4 py-8">
            <h1 class="text-xl font-bold mb-1 gradient-text">"Portfolio"</h1>
            <p class="text-xs text-gray-400 mb-6">
                {move || format!("{} positions", positions.get().len())}
                " · "
                {move || format!("{} locks", venfts.get().len())}
            </p>

            // Not connected
            {move || if state.wallet_address.get().is_none() {
                view! {
                    <div class="card p-12 text-center">
                        <h2 class="text-lg font-bold mb-2">"Connect Wallet"</h2>
                        <p class="text-sm text-gray-400">"Connect to view portfolio"</p>
                    </div>
                }.into_view()
            } else { ().into_view() }}

            // Stats row
            <div class="grid grid-cols-3 gap-2 mb-4">
                <div class="card p-3 text-center">
                    <div class="text-[10px] text-gray-400">"Positions"</div>
                    <div class="text-lg font-bold">{move || positions.get().len()}</div>
                </div>
                <div class="card p-3 text-center">
                    <div class="text-[10px] text-gray-400">"WIND Locked"</div>
                    <div class="text-sm font-bold text-cyan-400 truncate">{total_locked()}</div>
                </div>
                <div class="card p-3 text-center">
                    <div class="text-[10px] text-gray-400">"veWIND"</div>
                    <div class="text-sm font-bold text-cyan-400 truncate">{total_vp()}</div>
                </div>
            </div>

            // Tabs
            <div class="flex gap-1 mb-4">
                <button class=move || tab_btn(tab.get() == 0)
                    on:click=move |_| set_tab.set(0)
                >"Overview"</button>
                <button class=move || tab_btn(tab.get() == 1)
                    on:click=move |_| set_tab.set(1)
                >"Positions"</button>
                <button class=move || tab_btn(tab.get() == 2)
                    on:click=move |_| set_tab.set(2)
                >"Locks"</button>
            </div>

            // Overview
            {move || if tab.get() == 0 { view! {
                <div class="space-y-3">
                    // Balances
                    <div class="card p-4">
                        <h3 class="text-xs font-medium text-gray-400 uppercase tracking-wider mb-3">"Balances"</h3>
                        <div class="space-y-2">
                            {move || balances.get().into_iter().map(|(sym, fmt)| {
                                view! {
                                    <div class="flex justify-between"> py-1>
                                        <span class="text-sm font-medium">{sym}</span>
                                        <span class="text-sm text-gray-300">{fmt}</span>
                                    </div>
                                }
                            }).collect::<Vec<_>>()}
                        </div>
                    </div>

                    // Positions preview
                    {move || {
                        let p = positions.get();
                        if p.is_empty() {
                            view! {
                                <div class="card p-4 text-center text-sm text-gray-400">"No positions"</div>
                            }.into_view()
                        } else {
                            view! {
                                <div class="card p-4">
                                    <span class="text-xs font-medium text-gray-400 uppercase">"Top Positions"</span>
                                    <div class="space-y-2 mt-2>
                                        {p.into_iter().take(5).map(|pos| {
                                            let label = format!("{}/{}", pos.token0_symbol, pos.token1_symbol);
                                            let val = format!("${:.2}", pos.amount_usd);
                                            view! {
                                                <div class="flex justify-between"> py-1>
                                                    <span class="text-sm font-medium">{label}</span>
                                                    <span class="text-sm text-gray-300">{val}</span>
                                                </div>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>
                                </div>
                            }.into_view()
                        }
                    }}
                </div>
            }.into_view() } else { ().into_view() }}

            // Positions
            {move || if tab.get() == 1 { view! {
                <div class="space-y-2">
                    {move || {
                        let p = positions.get();
                        if p.is_empty() {
                            view! {
                                <div class="card p-8 text-center">
                                    <p class="text-sm text-gray-400">"No positions"</p>
                                    <p class="text-xs text-gray-500 mt-1>"Use Pools page to add liquidity"</p>
                                </div>
                            }.into_view()
                        } else {
                            p.into_iter().map(|pos| {
                                let label = format!("{}/{}", pos.token0_symbol, pos.token1_symbol);
                                let val = format!("${:.2}", pos.amount_usd);
                                let short_id = if pos.pool_id.len() > 10 { format!("{}...", &pos.pool_id[..10]) } else { pos.pool_id.clone() };
                                view! {
                                    <div class="card p-4 flex justify-between">
                                        <div>
                                            <div class="font-semibold text-sm">{label}</div>
                                            <div class="text-xs text-gray-400">{short_id}</div>
                                        </div>
                                        <div class="text-sm font-medium">{val}</div>
                                    </div>
                                }
                            }).collect::<Vec<_>>().into_view()
                        }
                    }}
                </div>
            }.into_view() } else { ().into_view() }}

            // Locks
            {move || if tab.get() == 2 { view! {
                <div class="space-y-2">
                    {move || {
                        let n = venfts.get();
                        if n.is_empty() {
                            view! {
                                <div class="card p-8 text-center">
                                    <p class="text-sm text-gray-400">"No veNFTs"</p>
                                    <p class="text-xs text-gray-500 mt-1>"Use Vote page to lock WIND"</p>
                                </div>
                            }.into_view()
                        } else {
                            n.into_iter().map(|nft| {
                                let amt = alloy_primitives::utils::format_units(nft.locked_amount, 18u8).unwrap_or_else(|_| "0".into());
                                let vp = alloy_primitives::utils::format_units(nft.voting_power, 18u8).unwrap_or_else(|_| "0".into());
                                let perm = nft.is_permanent;
                                let end_ts = nft.lock_end.saturating_to::<u64>() * 1000;
                                let end_str = if perm {
                                    "Permanent".to_string()
                                } else {
                                    js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(end_ts as f64))
                                        .to_locale_string("en-US", &wasm_bindgen::JsValue::UNDEFINED)
                                        .as_string()
                                        .unwrap_or_default()
                                };
                                let nft_id = nft.id.clone();
                                let status = if perm { "Permanent Lock".to_string() } else { format!("Unlocks {}", end_str) };
                                view! {
                                    <div class="card p-4">
                                        <div class="flex justify-between">
                                            <div class="flex gap-3>
                                                <div class="w-10 h-10 rounded-full bg-gradient-to-br from-cyan-500 to-blue-500 flex items-center justify-center text-white font-bold text-sm">
                                                    "#" {nft_id}
                                                </div>
                                                <div>
                                                    <div class="font-bold">{amt} " WIND"</div>
                                                    <div class="text-xs text-gray-400">{status}</div>
                                                </div>
                                            </div>
                                            <div>
                                                <div class="font-bold text-cyan-400">{vp}</div>
                                                <div class="text-[10px] text-gray-400">"veWIND"</div>
                                            </div>
                                        </div>
                                    </div>
                                }
                            }).collect::<Vec<_>>().into_view()
                        }
                    }}
                </div>
            }.into_view() } else { ().into_view() }}
        </main>
    }
}
