//! Vote & Lock page — lock WIND, vote on gauges, claim rewards.

use leptos::*;
use alloy_primitives::U256;
use crate::state::{use_app_state, ToastKind};
use crate::config::{WIND_TOKEN, VOTING_ESCROW};
use crate::subgraph;

const DURATIONS: &[(&str, u64)] = &[
    ("1M", 30 * 86400),
    ("6M", 180 * 86400),
    ("1Y", 365 * 86400),
    ("4Y", 1460 * 86400),
];

#[component]
pub fn VotePage() -> impl IntoView {
    let state = use_app_state();
    let (tab, set_tab) = create_signal(0u8);
    let (lock_amount, set_lock_amount) = create_signal(String::new());
    let (lock_dur_idx, set_lock_dur_idx) = create_signal(3usize);
    let (selected_nft, set_selected_nft) = create_signal(None::<String>);
    let (gauges, set_gauges) = create_signal(Vec::<subgraph::GaugeInfo>::new());
    let (venfts, set_venfts) = create_signal(Vec::<subgraph::VeNFTInfo>::new());
    let (protocol, set_protocol) = create_signal(subgraph::ProtocolInfo::default());
    let (bribes, set_bribes) = create_signal(Vec::<subgraph::BribeInfo>::new());

    // Fetch gauges on mount
    spawn_local(async move {
        if let Ok(g) = subgraph::fetch_gauges().await {
            set_gauges.set(g);
        }
        if let Ok(p) = subgraph::fetch_protocol().await {
            if let Some(epoch) = p.active_period {
                if let Ok(b) = subgraph::fetch_epoch_bribes(&epoch.to_string()).await {
                    set_bribes.set(b);
                }
            }
            set_protocol.set(p);
        }
    });

    // Fetch veNFTs when wallet connects
    let addr_signal = state.wallet_address;
    spawn_local(async move {
        if let Some(a) = addr_signal.get() {
            if let Ok(nfts) = subgraph::fetch_venfts(&a).await {
                if let Some(first) = nfts.first() {
                    set_selected_nft.set(Some(first.id.clone()));
                }
                set_venfts.set(nfts);
            }
        }
    });

    // Lock WIND handler
    let state_clone = state;
    let handle_lock = move |_| {
        let amount = lock_amount.get();
        let st = state_clone;
        let addr = st.wallet_address.get();
        if amount.is_empty() || addr.is_none() {
            st.toast(ToastKind::Error, "Enter amount and connect wallet", 3000);
            return;
        }
        let dur = DURATIONS[lock_dur_idx.get_untracked()].1;
        let addr_str = addr.unwrap();
        let amount_wei: U256 = alloy_primitives::utils::parse_units(&amount, 18u8)
            .map(|v| v.into())
            .unwrap_or(U256::ZERO);
        if amount_wei.is_zero() {
            st.toast(ToastKind::Error, "Invalid amount", 3000);
            return;
        }

        spawn_local(async move {
            st.toast(ToastKind::Loading, "Approving WIND...", 0);
            match crate::wallet::approve_max(
                &addr_str,
                &format!("{WIND_TOKEN:#x}"),
                &format!("{VOTING_ESCROW:#x}"),
            )
            .await
            {
                Ok(_) => {}
                Err(e) => {
                    st.toast(ToastKind::Error, format!("Approval failed: {e}"), 5000);
                    return;
                }
            }

            st.toast(ToastKind::Loading, "Creating lock...", 0);
            // Build VotingEscrow.createLock(amount, lock_duration) calldata
            // selector = keccak256("createLock(uint256,uint256)") first 4 bytes
            let selector = hex::decode("72d3775a").unwrap_or_default();
            let mut calldata = selector;
            let amount_bytes = amount_wei.to_be_bytes::<32>();
            calldata.extend_from_slice(&amount_bytes);
            let dur_bytes = U256::from(dur).to_be_bytes::<32>();
            calldata.extend_from_slice(&dur_bytes);

            match crate::wallet::send_transaction(
                &addr_str,
                &format!("{VOTING_ESCROW:#x}"),
                &calldata,
                "0x0",
            )
            .await
            {
                Ok(hash) => {
                    st.toast(ToastKind::Success, "Lock created!", 5000);
                    log::info!("Lock tx: {hash}");
                    set_lock_amount.set(String::new());
                    if let Ok(nfts) = subgraph::fetch_venfts(&addr_str).await {
                        set_venfts.set(nfts);
                    }
                }
                Err(e) => st.toast(ToastKind::Error, format!("Lock failed: {e}"), 5000),
            }
        });
    };

    // Epoch time helpers
    let days_left = move || {
        let p = protocol.get();
        if let Some(period) = p.active_period {
            let now = js_sys::Date::now() as u64 / 1000;
            (period + 604800).saturating_sub(now) / 86400
        } else {
            0
        }
    };
    let hours_left = move || {
        let p = protocol.get();
        if let Some(period) = p.active_period {
            let now = js_sys::Date::now() as u64 / 1000;
            ((period + 604800).saturating_sub(now) % 86400) / 3600
        } else {
            0
        }
    };

    let bribes_signal = bribes;

    let tab_class = move |t: u8| -> String {
        if tab.get() == t {
            "flex-1 py-2.5 rounded-lg text-sm font-bold transition border bg-gradient-to-r from-cyan-500 to-blue-500 text-white border-cyan-500/50".into()
        } else {
            "flex-1 py-2.5 rounded-lg text-sm font-bold transition border bg-white/5 text-gray-400 border-white/10".into()
        }
    };

    view! {
        <main class="max-w-2xl mx-auto px-4 py-8">
            <h1 class="text-2xl font-bold mb-1">
                <span class="text-transparent bg-clip-text bg-gradient-to-r from-cyan-400 to-blue-500">"Vote"</span>
                " & Earn"
            </h1>
            <p class="text-sm text-gray-400 mb-6">"Lock WIND -> Vote -> Earn rewards"</p>

            // Epoch banner
            <div class="mb-4 p-3 rounded-xl border border-blue-500/20 bg-blue-500/5 flex items-center justify-between">
                <div class="text-xs">
                    <span class="font-bold text-blue-400">
                        "Epoch "
                        {move || protocol.get().epoch_count.map(|e| e.to_string()).unwrap_or_default()}
                    </span>
                    <span class="text-gray-400 ml-2">
                        {days_left()}"d "{hours_left()}"h left"
                    </span>
                </div>
            </div>

            // Tabs
            <div class="flex gap-1.5 mb-4">
                <button class=move || tab_class(0) on:click=move |_| set_tab.set(0)>"Lock"</button>
                <button class=move || tab_class(1) on:click=move |_| set_tab.set(1)>"Vote"</button>
                <button class=move || tab_class(2) on:click=move |_| set_tab.set(2)>"Rewards"</button>
            </div>

            // Lock tab
            {move || if tab.get() == 0 { view! {
                <div class="space-y-3">
                    <div class="card p-4">
                        <div class="flex justify-between text-xs text-gray-400 mb-1">
                            <label>"Amount"</label>
                            <span>"WIND"</span>
                        </div>
                        <div class="p-3 rounded-lg bg-white/5 border border-white/10">
                            <input
                                type="text"
                                placeholder="0.0"
                                class="w-full bg-transparent text-xl font-bold outline-none placeholder-gray-600"
                                prop:value=lock_amount
                                on:input=move |e| set_lock_amount.set(event_target_value(&e))
                            />
                        </div>

                        <label class="text-xs text-gray-400 block mt-3 mb-2">"Lock Duration"</label>
                        <div class="grid grid-cols-4 gap-2">
                            {DURATIONS.iter().enumerate().map(|(i, (label, _))| {
                                let idx = i;
                                view! {
                                    <button
                                        class=move || format!(
                                            "py-3 rounded-xl text-sm font-bold transition-all border-2 {}",
                                            if lock_dur_idx.get() == idx {
                                                "bg-gradient-to-r from-cyan-500 to-blue-500 text-white border-cyan-500"
                                            } else {
                                                "bg-white/5 text-gray-300 border-white/10"
                                            }
                                        )
                                        on:click=move |_| set_lock_dur_idx.set(idx)
                                    >{*label}</button>
                                }
                            }).collect::<Vec<_>>()}
                        </div>

                        <button
                            class="w-full py-3 rounded-xl font-bold text-sm bg-gradient-to-r from-cyan-500 to-blue-500 text-white mt-3 disabled:opacity-50"
                            disabled=move || lock_amount.get().is_empty() || state.wallet_address.get().is_none()
                            on:click=handle_lock
                        >"Lock WIND"</button>
                    </div>

                    // veNFTs list
                    {move || {
                        let nfts = venfts.get();
                        if nfts.is_empty() {
                            view! { <div class="text-center text-sm text-gray-500 mt-3">"No veNFTs found"</div> }.into_view()
                        } else {
                            nfts.into_iter().map(|nft| {
                                let amount_f = alloy_primitives::utils::format_units(nft.locked_amount, 18u8)
                                    .unwrap_or_else(|_| "0".into());
                                let vp_f = alloy_primitives::utils::format_units(nft.voting_power, 18u8)
                                    .unwrap_or_else(|_| "0".into());
                                let perm = nft.is_permanent;
                                let end_ts = nft.lock_end.saturating_to::<u64>() * 1000;
                                view! {
                                    <div class="card p-4 mt-2 flex items-center justify-between">
                                        <div>
                                            <div class="font-bold">
                                                "#" {nft.id.clone()} " — " {amount_f} " WIND"
                                            </div>
                                            <div class="text-xs text-gray-400">
                                                {if perm {
                                                    "Permanent Lock".to_string()
                                                } else {
                                                    format!("Unlocks {}", js_sys::Date::new(
                                                        &wasm_bindgen::JsValue::from_f64(end_ts as f64)
                                                    ).to_locale_string("en-US", &wasm_bindgen::JsValue::UNDEFINED)
                                                        .as_string()
                                                        .unwrap_or_default())
                                                }}
                                            </div>
                                        </div>
                                        <div class="text-right">
                                            <div class="font-bold text-cyan-400">{vp_f}</div>
                                            <div class="text-[10px] text-gray-400">"veWIND"</div>
                                        </div>
                                    </div>
                                }
                            }).collect::<Vec<_>>().into_view()
                        }
                    }}
                </div>
            }.into_view() } else { ().into_view() }}

            // Vote tab
            {move || if tab.get() == 1 { view! {
                <div>
                    // veNFT selector
                    {move || {
                        let nfts = venfts.get();
                        if nfts.is_empty() {
                            view! { <div class="card p-4 text-center text-sm text-gray-400">"Lock WIND first to vote"</div> }.into_view()
                        } else {
                            let current = selected_nft.get().unwrap_or_default();
                            view! {
                                <div class="card p-3 mb-3">
                                    <label class="text-xs text-gray-400 mb-1 block">"Select veNFT"</label>
                                    <div class="flex gap-2">
                                        {nfts.into_iter().map(|nft| {
                                            let id = nft.id.clone();
                                            let id_display = id.clone();
                                            let is_sel = id == current;
                                            view! {
                                                <button
                                                    class=move || format!(
                                                        "px-3 py-2 rounded-lg text-xs font-medium {}",
                                                        if is_sel { "bg-cyan-500 text-white" } else { "bg-white/5 text-gray-300" }
                                                    )
                                                    on:click=move |_| set_selected_nft.set(Some(id.clone()))
                                                >"#" {id_display}</button>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>
                                </div>
                            }.into_view()
                        }
                    }}

                    // Gauges
                    <div class="space-y-2">
                        {move || {
                            let gs = gauges.get();
                            let br = bribes_signal.get();
                            gs.into_iter().filter(|g| g.alive).map(|gauge| {
                                let gid = gauge.id.clone();
                                let bribes_total: f64 = br.iter().filter(|b| b.gauge_id == gid).map(|b| b.amount_usd).sum();
                                let pool_label = format!("{}/{}", gauge.token0_symbol, gauge.token1_symbol);
                                let type_label = if gauge.is_stable { "Stable" } else if gauge.pool_type == "CL" { "V3" } else { "V2" };
                                let has_bribes = bribes_total > 0.0;
                                view! {
                                    <div class="card p-3 flex items-center justify-between hover:bg-white/5 transition-colors">
                                        <div>
                                            <div class="font-semibold text-sm">{pool_label}</div>
                                            <div class="flex gap-1 text-[10px]">
                                                <span class="px-1.5 py-0.5 rounded bg-white/5 text-gray-500">{type_label}</span>
                                                {has_bribes.then(|| view! {
                                                    <span class="px-1.5 py-0.5 rounded bg-amber-500/15 text-amber-400">"Bribe"</span>
                                                })}
                                            </div>
                                        </div>
                                        {has_bribes.then(|| view! {
                                            <div class="text-right text-xs">
                                                <div class="text-amber-400 font-medium">
                                                    {format!("${:.0}", bribes_total)}
                                                </div>
                                            </div>
                                        })}
                                    </div>
                                }
                            }).collect::<Vec<_>>()
                        }}
                    </div>
                </div>
            }.into_view() } else { ().into_view() }}

            // Rewards tab
            {move || if tab.get() == 2 { view! {
                <div class="card p-6 text-center">
                    <div class="text-4xl mb-3">"🎁"</div>
                    <h3 class="text-lg font-bold mb-2">"Voting Rewards"</h3>
                    <p class="text-sm text-gray-400">"Claim earned fees and bribes."</p>
                    <p class="text-xs text-gray-500 mt-2">"Vote on pools to start earning."</p>
                </div>
            }.into_view() } else { ().into_view() }}
        </main>
    }
}
