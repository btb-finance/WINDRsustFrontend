use leptos::*;
use crate::config::{Token, TOKENS};

/// Modal token picker with live search.
#[component]
pub fn TokenSelectorModal(
    #[prop(into)] selected: Signal<Token>,
    on_select: impl Fn(Token) + 'static + Copy,
    on_close:  impl Fn()    + 'static + Copy,
) -> impl IntoView {
    // WriteSignal — on stable Rust must use .set() not set_query(value)
    let (query, set_query) = create_signal(String::new());

    let filtered = move || {
        let q = query.get().to_lowercase();
        if q.is_empty() {
            TOKENS.to_vec()
        } else {
            TOKENS
                .iter()
                .filter(|t| t.symbol.to_lowercase().contains(&q) || t.name.to_lowercase().contains(&q))
                .cloned()
                .collect()
        }
    };

    view! {
        // Backdrop — click outside to close
        <div
            class="fixed inset-0 z-50 flex items-center justify-center bg-black/70"
            on:click=move |_| on_close()
        >
            // Card — stop propagation so interior clicks don't dismiss
            <div
                class="card w-full max-w-sm mx-4 p-5 animate-fade-in"
                on:click=move |e| e.stop_propagation()
            >
                <div class="flex items-center justify-between mb-4">
                    <h3 class="text-lg font-semibold">"Select a token"</h3>
                    <button on:click=move |_| on_close()
                        class="text-gray-400 hover:text-white text-xl leading-none">"×"</button>
                </div>

                // Search
                <input
                    type="text"
                    placeholder="Search by name or symbol…"
                    class="w-full bg-gray-800 rounded-xl px-4 py-3 text-sm outline-none
                           placeholder-gray-500 border border-gray-700 focus:border-wind-500 mb-4"
                    prop:value=query
                    // stable Rust: use .set() on WriteSignal
                    on:input=move |e| set_query.set(event_target_value(&e))
                />

                // Token list
                <div class="space-y-1 max-h-72 overflow-y-auto pr-1">
                    <For
                        each=filtered
                        key=|t| t.symbol     // &'static str is Hash + Eq
                        children=move |token| {
                            let is_selected = selected.get() == token;
                            let tok = token.clone();
                            view! {
                                <button
                                    class=move || format!(
                                        "w-full flex items-center gap-3 px-3 py-3 rounded-xl \
                                         transition-colors text-left hover:bg-gray-800 {}",
                                        if is_selected { "bg-gray-700" } else { "" }
                                    )
                                    on:click=move |_| {
                                        on_select(tok.clone());
                                        on_close();
                                    }
                                >
                                    <span class="text-2xl w-8 text-center shrink-0">{token.logo}</span>
                                    <div class="flex flex-col">
                                        <span class="font-semibold text-sm">{token.symbol}</span>
                                        <span class="text-xs text-gray-400">{token.name}</span>
                                    </div>
                                    {if is_selected {
                                        view! { <span class="ml-auto text-wind-400">"✓"</span> }.into_view()
                                    } else {
                                        ().into_view()
                                    }}
                                </button>
                            }
                        }
                    />
                </div>
            </div>
        </div>
    }
}
