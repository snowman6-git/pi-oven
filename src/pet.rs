//! Turns pi's event stream into the neutral states the desktop pet (oven-pet) understands, and
//! publishes them on `/api/pet/stream`. The pet follows this stream from another computer, so
//! no tunnel is needed: it connects to the oven the same way a browser does.
//!
//! Translation is deliberately dumb and cheap. pi emits thousands of `message_update` lines per
//! answer, so lines are classified by looking for markers at their structural position, and only
//! the rare ones that carry data we need are parsed as JSON. (A marker inside a model's answer is
//! JSON-escaped, so it can never be mistaken for the real thing.)

use std::{collections::HashMap, sync::{Arc, Mutex}, time::Duration};

use tokio::sync::broadcast;

use crate::runtime::Runtime;

/// A sustained state is re-announced this often, and expires this long after the last
/// announcement: if the oven dies the pet stops "working" within two minutes.
const HEARTBEAT: Duration = Duration::from_secs(45);
const SUSTAINED_TTL_MS: u64 = 120_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    /// What the agent is doing.
    Main,
    /// Background tasks, tracked separately so they do not hide (or get hidden by) the agent's state.
    Bg,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Out {
    pub slot: Slot,
    pub state: &'static str,
    pub tool: Option<String>,
    pub ttl_ms: Option<u64>,
}

fn out(slot: Slot, state: &'static str, tool: Option<String>, ttl_ms: Option<u64>) -> Out {
    Out { slot, state, tool, ttl_ms }
}

#[derive(Default)]
pub struct Translator {
    main: Option<(&'static str, Option<String>)>,
    bg_running: bool,
    failed_run: bool,
}

/// The token after `marker`, up to the next quote.
fn after<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    let i = line.find(marker)? + marker.len();
    let rest = &line[i..];
    Some(&rest[..rest.find('"')?])
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for n in chars.by_ref() {
                    if ('@'..='~').contains(&n) {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// "bg 2 running · 1 failed" -> (running, failed, stopped, done)
fn bg_counts(text: &str) -> (u32, u32, u32, u32) {
    let words: Vec<&str> = text.split(|c: char| c.is_whitespace() || c == '·').filter(|w| !w.is_empty()).collect();
    let n = |word: &str| words.windows(2).find(|w| w[1] == word).and_then(|w| w[0].parse::<u32>().ok()).unwrap_or(0);
    (n("running"), n("failed"), n("stopped"), n("done"))
}

impl Translator {
    fn set_main(&mut self, state: &'static str, tool: Option<String>, ttl: Option<u64>) -> Vec<Out> {
        if self.main.as_ref().is_some_and(|(s, t)| *s == state && *t == tool) {
            return vec![];
        }
        self.main = Some((state, tool.clone()));
        vec![out(Slot::Main, state, tool, ttl)]
    }

    pub fn line(&mut self, line: &str) -> Vec<Out> {
        let Some(kind) = after(line, r#"{"type":""#) else { return vec![] };
        match kind {
            "agent_start" => {
                self.failed_run = false;
                self.main = None; // a new run starts from a clean slate, so even a repeated state is announced
                self.set_main("wake", None, None)
            }
            "message_update" => match after(line, r#""assistantMessageEvent":{"type":""#) {
                Some("thinking_start" | "thinking_delta") => self.set_main("thinking", None, Some(SUSTAINED_TTL_MS)),
                Some("text_start" | "text_delta") => self.set_main("working", None, Some(SUSTAINED_TTL_MS)),
                _ => vec![],
            },
            "tool_execution_start" => {
                let tool = serde_json::from_str::<serde_json::Value>(line).ok().and_then(|v| v["toolName"].as_str().map(str::to_owned));
                self.set_main("working", tool, Some(SUSTAINED_TTL_MS))
            }
            "extension_ui_request" => match after(line, r#""method":""#) {
                Some("select" | "confirm" | "input" | "editor") => self.set_main("ask", None, None),
                Some("setStatus") if after(line, r#""statusKey":""#) == Some("background-tasks") => {
                    let text = serde_json::from_str::<serde_json::Value>(line)
                        .ok()
                        .and_then(|v| v["statusText"].as_str().map(strip_ansi))
                        .unwrap_or_default();
                    self.background(bg_counts(&text))
                }
                _ => vec![],
            },
            "message_end" => {
                if line.contains(r#""stopReason":"error""#) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                        if v["message"]["role"] == "assistant" && v["message"]["stopReason"] == "error" {
                            self.failed_run = true;
                        }
                    }
                }
                vec![]
            }
            "agent_settled" => {
                let state = if line.contains(r#""aborted":true"#) {
                    "aborted"
                } else if std::mem::take(&mut self.failed_run) {
                    "error"
                } else {
                    "done"
                };
                self.main = None;
                self.set_main(state, None, None)
            }
            "oven_exit" => {
                self.main = None;
                self.bg_running = false;
                vec![out(Slot::Main, "idle", None, None), out(Slot::Bg, "idle", None, None)]
            }
            _ => vec![],
        }
    }

    /// The extension reported background tasks: running now, or just ended.
    fn background(&mut self, (running, failed, stopped, _done): (u32, u32, u32, u32)) -> Vec<Out> {
        if running > 0 {
            if self.bg_running {
                return vec![];
            }
            self.bg_running = true;
            return vec![out(Slot::Bg, "bg_running", None, Some(SUSTAINED_TTL_MS))];
        }
        if !self.bg_running {
            return vec![];
        }
        self.bg_running = false;
        let state = if failed > 0 || stopped > 0 { "bg_failed" } else { "bg_done" };
        vec![out(Slot::Bg, state, None, None)]
    }

    /// The person answered a question: the run carries on.
    pub fn answered(&mut self) -> Vec<Out> {
        if matches!(self.main, Some(("ask", _))) {
            self.main = None;
            return self.set_main("working", None, Some(SUSTAINED_TTL_MS));
        }
        vec![]
    }

    /// What is still true, for new subscribers and for the heartbeat.
    pub fn current(&self) -> Vec<Out> {
        let mut v = Vec::new();
        if let Some((state, tool)) = &self.main {
            if matches!(*state, "thinking" | "working") {
                v.push(out(Slot::Main, state, tool.clone(), Some(SUSTAINED_TTL_MS)));
            } else if *state == "ask" {
                v.push(out(Slot::Main, "ask", None, None));
            }
        }
        if self.bg_running {
            v.push(out(Slot::Bg, "bg_running", None, Some(SUSTAINED_TTL_MS)));
        }
        v
    }
}

fn json(runtime: &str, o: &Out) -> String {
    let session = match o.slot {
        Slot::Main => runtime.to_string(),
        Slot::Bg => format!("{runtime}#bg"),
    };
    serde_json::json!({ "source": "pi-oven", "session": session, "group": runtime, "state": o.state, "tool": o.tool, "ttl_ms": o.ttl_ms }).to_string()
}

pub struct PetHub {
    tx: broadcast::Sender<String>,
    translators: Mutex<HashMap<String, Translator>>,
}

impl PetHub {
    pub fn new() -> Arc<PetHub> {
        let (tx, _) = broadcast::channel(256);
        let hub = Arc::new(PetHub { tx, translators: Mutex::new(HashMap::new()) });
        let beat = hub.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(HEARTBEAT);
            tick.tick().await;
            loop {
                tick.tick().await;
                for line in beat.snapshot() {
                    let _ = beat.tx.send(line);
                }
            }
        });
        hub
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }

    /// Everything currently true, so a pet that just connected shows the right thing at once.
    pub fn snapshot(&self) -> Vec<String> {
        let map = self.translators.lock().unwrap();
        map.iter().flat_map(|(id, t)| t.current().into_iter().map(move |o| json(id, &o))).collect()
    }

    fn publish(&self, runtime: &str, events: Vec<Out>) {
        for o in events {
            let _ = self.tx.send(json(runtime, &o));
        }
    }

    pub fn answered(&self, runtime: &str) {
        let events = self.translators.lock().unwrap().get_mut(runtime).map(Translator::answered).unwrap_or_default();
        self.publish(runtime, events);
    }

    /// Follow one runtime's events for as long as it lives.
    pub fn attach(self: &Arc<Self>, rt: Arc<Runtime>) {
        let hub = self.clone();
        let id = rt.info.id.clone();
        let mut rx = rt.events.subscribe();
        hub.translators.lock().unwrap().insert(id.clone(), Translator::default());
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(line) => {
                        let events = hub.translators.lock().unwrap().get_mut(&id).map(|t| t.line(&line)).unwrap_or_default();
                        hub.publish(&id, events);
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            hub.translators.lock().unwrap().remove(&id);
            hub.publish(&id, vec![out(Slot::Main, "idle", None, None), out(Slot::Bg, "idle", None, None)]);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn states(t: &mut Translator, lines: &[&str]) -> Vec<(Slot, &'static str, Option<String>)> {
        lines.iter().flat_map(|l| t.line(l)).map(|o| (o.slot, o.state, o.tool)).collect()
    }

    const THINK: &str = r#"{"type":"message_update","usage":{"input":1},"assistantMessageEvent":{"type":"thinking_delta","contentIndex":0,"delta":"hm"}}"#;
    const TEXT: &str = r#"{"type":"message_update","usage":{"input":1},"assistantMessageEvent":{"type":"text_delta","contentIndex":1,"delta":"hi"}}"#;
    const BASH: &str = r#"{"type":"tool_execution_start","toolCallId":"c1","toolName":"bash","args":{"command":"ls"}}"#;
    const SETTLED: &str = r#"{"type":"agent_settled","aborted":false}"#;

    #[test]
    fn a_normal_run_reads_wake_think_work_answer_done() {
        let mut t = Translator::default();
        let got = states(&mut t, &[r#"{"type":"agent_start"}"#, THINK, THINK, THINK, BASH, BASH, TEXT, TEXT, SETTLED]);
        let want: Vec<(Slot, &str, Option<String>)> = vec![
            (Slot::Main, "wake", None),
            (Slot::Main, "thinking", None),
            (Slot::Main, "working", Some("bash".into())),
            (Slot::Main, "working", None),
            (Slot::Main, "done", None),
        ];
        assert_eq!(got, want, "repeats must not be re-announced");
    }

    #[test]
    fn a_marker_inside_a_model_answer_is_not_a_marker() {
        let mut t = Translator::default();
        // The answer text mentions the event names; inside JSON they are escaped.
        let line = r#"{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"the event \"thinking_delta\" and {\"type\":\"agent_settled\"}"}}"#;
        assert_eq!(states(&mut t, &[line]), vec![(Slot::Main, "working", None)]);
    }

    #[test]
    fn asking_and_being_answered() {
        let mut t = Translator::default();
        let ask = r#"{"type":"extension_ui_request","id":"u1","method":"confirm","title":"bash 실행을 허용할까요?","message":"ls"}"#;
        assert_eq!(states(&mut t, &[BASH, ask]), vec![(Slot::Main, "working", Some("bash".into())), (Slot::Main, "ask", None)]);
        assert_eq!(t.current().len(), 1);
        let after = t.answered();
        assert_eq!((after[0].state, after[0].tool.clone()), ("working", None));
        assert!(t.answered().is_empty(), "answering twice changes nothing");
    }

    #[test]
    fn a_failed_or_stopped_run_is_not_reported_as_done() {
        let mut t = Translator::default();
        let err = r#"{"type":"message_end","message":{"role":"assistant","stopReason":"error","content":[]}}"#;
        assert_eq!(states(&mut t, &[r#"{"type":"agent_start"}"#, err, SETTLED]).last().unwrap().1, "error");
        assert_eq!(states(&mut t, &[r#"{"type":"agent_start"}"#, SETTLED]).last().unwrap().1, "done", "the failure does not carry over");
        assert_eq!(states(&mut t, &[r#"{"type":"agent_settled","aborted":true}"#]).last().unwrap().1, "aborted");
    }

    #[test]
    fn a_status_line_about_other_things_is_ignored() {
        let mut t = Translator::default();
        let other = r#"{"type":"extension_ui_request","id":"u","method":"setStatus","statusKey":"goal","statusText":"x"}"#;
        assert!(states(&mut t, &[other, r#"{"type":"extension_ui_request","id":"u","method":"notify","message":"hi"}"#]).is_empty());
    }

    #[test]
    fn background_tasks_start_end_and_fail_on_their_own_slot() {
        let status = |text: &str| format!(r#"{{"type":"extension_ui_request","id":"u","method":"setStatus","statusKey":"background-tasks","statusText":"\u001b[48;2;1;2;3m {text} \u001b[0m"}}"#);
        let mut t = Translator::default();
        let got = states(&mut t, &[&status("bg 1 running · Shift↓"), &status("bg 1 running · Shift↓"), &status("bg 1 done · /bg-clear")]);
        assert_eq!(got, vec![(Slot::Bg, "bg_running", None), (Slot::Bg, "bg_done", None)]);
        let got = states(&mut t, &[&status("bg 2 running"), &status("bg 1 failed · 1 done")]);
        assert_eq!(got, vec![(Slot::Bg, "bg_running", None), (Slot::Bg, "bg_failed", None)]);
        assert!(states(&mut t, &[&status("bg 1 done")]).is_empty(), "a leftover count is not a new event");
    }

    #[test]
    fn current_state_is_what_a_new_pet_is_told() {
        let mut t = Translator::default();
        let _ = states(&mut t, &[r#"{"type":"agent_start"}"#, THINK]);
        let now = t.current();
        assert_eq!((now[0].state, now[0].ttl_ms), ("thinking", Some(SUSTAINED_TTL_MS)));
        let _ = states(&mut t, &[SETTLED]);
        assert!(t.current().is_empty(), "a finished moment is not replayed");
    }

    #[test]
    fn a_dead_runtime_clears_both_slots() {
        let mut t = Translator::default();
        let _ = states(&mut t, &[BASH]);
        let got = states(&mut t, &[r#"{"type":"oven_exit","code":0}"#]);
        assert_eq!(got.iter().map(|g| (g.0, g.1)).collect::<Vec<_>>(), vec![(Slot::Main, "idle"), (Slot::Bg, "idle")]);
    }

    #[test]
    fn json_shape_matches_what_the_pet_reads() {
        let o = out(Slot::Bg, "bg_running", None, Some(1000));
        let v: serde_json::Value = serde_json::from_str(&json("abc123", &o)).unwrap();
        assert_eq!((v["source"].as_str(), v["session"].as_str(), v["state"].as_str(), v["ttl_ms"].as_u64()), (Some("pi-oven"), Some("abc123#bg"), Some("bg_running"), Some(1000)));
        assert_eq!(v["group"].as_str(), Some("abc123"), "the background slot belongs to its conversation");
    }
}
