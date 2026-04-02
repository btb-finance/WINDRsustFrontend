use leptos::*;
use crate::state::{use_app_state, ToastKind};

#[component]
pub fn ToastContainer() -> impl IntoView {
    let state = use_app_state();

    view! {
        <div class="fixed bottom-6 right-4 z-[100] flex flex-col gap-2 items-end pointer-events-none">
            <For
                each=move || state.toasts.get()
                key=|t| t.id
                children=move |toast| {
                    let (bg, icon) = match toast.kind {
                        ToastKind::Success => ("bg-green-900 border-green-700", "✓"),
                        ToastKind::Error   => ("bg-red-900 border-red-700",   "✗"),
                        ToastKind::Info    => ("bg-gray-800 border-gray-700", "ℹ"),
                        ToastKind::Loading => ("bg-gray-800 border-gray-700", "⟳"),
                    };
                    let id = toast.id;
                    view! {
                        <div
                            class=format!(
                                "flex items-start gap-3 px-4 py-3 rounded-xl border \
                                 shadow-xl text-sm max-w-xs animate-fade-in pointer-events-auto \
                                 {bg}"
                            )
                        >
                            <span class="shrink-0 font-bold">{icon}</span>
                            <span class="text-gray-100">{toast.message.clone()}</span>
                            <button
                                class="ml-auto text-gray-400 hover:text-white shrink-0"
                                on:click=move |_| state.dismiss_toast(id)
                            >"×"</button>
                        </div>
                    }
                }
            />
        </div>
    }
}
