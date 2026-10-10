//! AI assistant chat state — a worker thread owns the blocking
//! `llm::Assistant` so the UI never stalls on network I/O.

use macinformer_core::llm;
use std::sync::mpsc::{channel, Receiver, Sender};

pub enum ChatEvent {
    Reply { text: String, actions: Vec<llm::Action> },
    Error(String),
}

pub struct Chat {
    tx: Sender<(String, String)>,
    rx: Receiver<ChatEvent>,
    pub provider_name: String,
    /// (role, text) — role is "You", the provider name, or "error".
    pub log: Vec<(String, String)>,
    pub input: String,
    /// Lines scrolled up from the bottom (0 = follow latest).
    pub scroll: usize,
    pub busy: bool,
}

impl Chat {
    pub fn new(provider: llm::Provider) -> Chat {
        let (tx, work_rx) = channel::<(String, String)>();
        let (work_tx, rx) = channel::<ChatEvent>();
        let provider_name = provider.describe();
        std::thread::Builder::new()
            .name("macinformer-ai".into())
            .spawn(move || {
                let mut assistant = llm::Assistant::new(provider);
                while let Ok((ctx, q)) = work_rx.recv() {
                    let ev = match assistant.ask(&ctx, &q) {
                        Ok(r) => ChatEvent::Reply { text: r.text, actions: r.actions },
                        Err(e) => ChatEvent::Error(e),
                    };
                    if work_tx.send(ev).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn ai worker thread");
        // thread is detached: dropping `tx` ends its loop once any in-flight
        // request finishes, and dropping `rx` makes its final send fail.
        Chat { tx, rx, provider_name, log: Vec::new(), input: String::new(), scroll: 0, busy: false }
    }

    /// Queue the current input with a fresh live-state context.
    pub fn send(&mut self, context: String) {
        let q = self.input.trim().to_string();
        if q.is_empty() || self.busy {
            return;
        }
        self.input.clear();
        if self.tx.send((context, q.clone())).is_ok() {
            self.log.push(("You".into(), q));
            self.busy = true;
            self.scroll = 0;
        }
    }

    /// Drain finished replies; `Some(actions)` when anything changed.
    pub fn poll(&mut self) -> Option<Vec<llm::Action>> {
        let mut actions = Vec::new();
        let mut changed = false;
        while let Ok(ev) = self.rx.try_recv() {
            self.busy = false;
            changed = true;
            match ev {
                ChatEvent::Reply { text, actions: a } => {
                    self.log.push((self.provider_name.clone(), text));
                    actions.extend(a);
                }
                ChatEvent::Error(e) => self.log.push(("error".into(), e)),
            }
        }
        if changed && self.scroll == 0 {
            // keep following the tail
        }
        changed.then_some(actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_or_busy_send_is_ignored() {
        let p = llm::resolve("gguf", None, None, None).unwrap();
        let mut c = Chat::new(p);
        c.send("ctx".into());
        assert!(c.log.is_empty());
        c.input = "hello".into();
        c.busy = true; // simulate in-flight request
        c.send("ctx".into());
        assert!(c.log.is_empty(), "busy chat must not queue a second question");
    }
}
