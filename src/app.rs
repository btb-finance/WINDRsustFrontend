use leptos::*;
use leptos_router::*;

use crate::{
    components::{
        header::Header,
        swap::SwapPage,
        pools::PoolsPage,
        toast::ToastContainer,
    },
    state::AppState,
};

/// Root application component.
/// Provides global state via Leptos context, sets up the router, and renders the shell.
#[component]
pub fn App() -> impl IntoView {
    // Create global state once and share via context
    let app_state = AppState::new();
    provide_context(app_state);

    view! {
        <Router>
            // ── App shell ────────────────────────────────────────────────────
            <div class="min-h-screen flex flex-col">
                <Header/>
                <div class="flex-1">
                    <Routes>
                        // Home → redirect to swap
                        <Route path="/"        view=|| view! { <Redirect path="/swap"/> }/>
                        <Route path="/swap"    view=SwapPage/>
                        <Route path="/pools"   view=PoolsPage/>
                        <Route path="/vote"    view=VotePage/>
                        <Route path="/portfolio" view=PortfolioPage/>
                        // 404 fallback
                        <Route path="/*any"    view=NotFound/>
                    </Routes>
                </div>
                <Footer/>
            </div>
            // Toast notifications overlay (position: fixed, lives outside the flow)
            <ToastContainer/>
        </Router>
    }
}

// ─── Placeholder pages (wired up, ready to flesh out) ────────────────────────

#[component]
fn VotePage() -> impl IntoView {
    view! {
        <main class="max-w-2xl mx-auto px-4 py-12">
            <h1 class="text-2xl font-bold mb-4">"Vote"</h1>
            <p class="text-gray-400">"Lock WIND to obtain veWIND and vote on gauge emissions."</p>
        </main>
    }
}

#[component]
fn PortfolioPage() -> impl IntoView {
    view! {
        <main class="max-w-2xl mx-auto px-4 py-12">
            <h1 class="text-2xl font-bold mb-4">"Portfolio"</h1>
            <p class="text-gray-400">"Your positions, balances and claimable rewards."</p>
        </main>
    }
}

#[component]
fn NotFound() -> impl IntoView {
    view! {
        <main class="flex flex-col items-center justify-center py-32 gap-4">
            <span class="text-6xl">"💨"</span>
            <h1 class="text-2xl font-bold">"404 – Page Not Found"</h1>
            <a href="/swap" class="btn-primary">"Go to Swap"</a>
        </main>
    }
}

// ─── Footer ───────────────────────────────────────────────────────────────────

#[component]
fn Footer() -> impl IntoView {
    view! {
        <footer class="border-t border-gray-800 py-6 text-center text-xs text-gray-600">
            "Wind Swap · Built on Base · "
            <a
                href="https://basescan.org"
                target="_blank"
                class="hover:text-gray-400 transition-colors"
            >"BaseScan ↗"</a>
        </footer>
    }
}
