use leptos::*;

// ─── Toast ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum ToastKind { Success, Error, Info, Loading }

#[derive(Clone, Debug)]
pub struct Toast {
    pub id:      u32,
    pub kind:    ToastKind,
    pub message: String,
}

// ─── Global app state (provided at root, consumed anywhere) ──────────────────

#[derive(Copy, Clone)]
pub struct AppState {
    /// Connected wallet address (None if disconnected)
    pub wallet_address: RwSignal<Option<String>>,
    /// Detected chain ID
    pub chain_id: RwSignal<Option<u64>>,
    /// Toast notification queue
    pub toasts: RwSignal<Vec<Toast>>,
    /// Running toast counter for stable IDs
    pub toast_counter: RwSignal<u32>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            wallet_address:  create_rw_signal(None),
            chain_id:        create_rw_signal(None),
            toasts:          create_rw_signal(vec![]),
            toast_counter:   create_rw_signal(0u32),
        }
    }

    /// Push a toast; auto-dismiss after `ms` milliseconds (0 = never).
    pub fn toast(&self, kind: ToastKind, message: impl Into<String>, ms: u32) {
        let id = {
            let mut c = self.toast_counter.get_untracked();
            c += 1;
            self.toast_counter.set(c);
            c
        };
        let entry = Toast { id, kind, message: message.into() };
        self.toasts.update(|v| v.push(entry));

        if ms > 0 {
            let toasts  = self.toasts;
            let dismiss_id = id;
            let timeout = gloo_timers::callback::Timeout::new(ms, move || {
                toasts.update(|v| v.retain(|t| t.id != dismiss_id));
            });
            timeout.forget(); // let it fire without holding a handle
        }
    }

    pub fn dismiss_toast(&self, id: u32) {
        self.toasts.update(|v| v.retain(|t| t.id != id));
    }

    pub fn is_connected(&self) -> bool {
        self.wallet_address.get().is_some()
    }

    /// True when connected to Base (chain 8453).
    pub fn is_correct_network(&self) -> bool {
        self.chain_id.get() == Some(crate::config::CHAIN_ID)
    }
}

/// Helper: fetch the global state from Leptos context.
pub fn use_app_state() -> AppState {
    use_context::<AppState>().expect("AppState not provided")
}
