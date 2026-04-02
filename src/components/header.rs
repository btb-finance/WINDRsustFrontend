use leptos::*;
use leptos_router::A;
use crate::state::{use_app_state, ToastKind};
use crate::wallet;

#[component]
pub fn Header() -> impl IntoView {
    let state = use_app_state();

    let connect_wallet = move |_| {
        let state = state;
        spawn_local(async move {
            match wallet::request_accounts().await {
                Ok(addr) => {
                    state.wallet_address.set(Some(addr.clone()));
                    if let Ok(cid) = wallet::get_chain_id().await {
                        state.chain_id.set(Some(cid));
                        if cid != crate::config::CHAIN_ID {
                            state.toast(ToastKind::Error, "Please switch to Base network", 5000);
                        } else {
                            state.toast(
                                ToastKind::Success,
                                format!("Connected: {}…{}", &addr[..6], &addr[addr.len()-4..]),
                                3000,
                            );
                        }
                    }
                }
                Err(e) => state.toast(ToastKind::Error, e, 5000),
            }
        });
    };

    let switch_network = move |_| {
        let state = state;
        spawn_local(async move {
            match wallet::switch_to_base().await {
                Ok(_)  => state.toast(ToastKind::Success, "Switched to Base", 2000),
                Err(e) => state.toast(ToastKind::Error, e, 5000),
            }
        });
    };

    view! {
        <header class="sticky top-0 z-50 bg-gray-950/90 backdrop-blur border-b border-gray-800">
            <div class="max-w-6xl mx-auto px-4 h-16 flex items-center justify-between">

                // ── Logo ──────────────────────────────────────────────────────
                <A href="/" class="flex items-center gap-2 font-bold text-xl text-white">
                    <span class="text-2xl">"💨"</span>
                    <span>"Wind Swap"</span>
                </A>

                // ── Nav links ─────────────────────────────────────────────────
                <nav class="hidden md:flex items-center gap-6 text-sm text-gray-400">
                    <A href="/swap"       class="hover:text-white transition-colors">"Swap"</A>
                    <A href="/pools"      class="hover:text-white transition-colors">"Pools"</A>
                    <A href="/vote"       class="hover:text-white transition-colors">"Vote"</A>
                    <A href="/portfolio"  class="hover:text-white transition-colors">"Portfolio"</A>
                </nav>

                // ── Wallet button ─────────────────────────────────────────────
                {move || {
                    let addr = state.wallet_address.get();
                    let wrong_net = addr.is_some() && !state.is_correct_network();

                    if wrong_net {
                        view! {
                            <button on:click=switch_network
                                class="btn-primary bg-red-500 hover:bg-red-600 text-sm px-4 py-2">
                                "⚠ Wrong Network"
                            </button>
                        }.into_view()
                    } else if let Some(a) = addr {
                        view! {
                            <div class="flex items-center gap-2 bg-gray-800 rounded-xl px-4 py-2 text-sm">
                                <span class="w-2 h-2 rounded-full bg-green-400 inline-block"></span>
                                <span class="text-gray-200 font-mono">
                                    {format!("{}…{}", &a[..6], &a[a.len()-4..])}
                                </span>
                            </div>
                        }.into_view()
                    } else {
                        view! {
                            <button on:click=connect_wallet class="btn-primary text-sm px-5 py-2">
                                "Connect Wallet"
                            </button>
                        }.into_view()
                    }
                }}

            </div>
        </header>
    }
}
