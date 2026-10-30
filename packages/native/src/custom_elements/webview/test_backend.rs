//! The engine of the test renderer: it records what the element asks of it, answers from a state
//! a test sets, and reports only the events a test makes.
use std::{cell::RefCell, collections::HashMap, collections::VecDeque, rc::Rc};

use super::backend::{EventEmitter, ScriptDone, ScriptOutcome, WebviewBackend, WebviewEvent, WebviewState};

/// What the element asked of the engine, in order.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum WebviewCommand {
    Load(String),
    Reload,
    Stop,
    GoBack,
    GoForward,
    ExecuteJavaScript(String),
    SetFirstResponder(bool),
}

#[derive(Default)]
pub(crate) struct TestPage {
    pub(crate) commands: Vec<WebviewCommand>,
    pub(crate) state: WebviewState,
    /// The scripts that wait for a test to settle them, oldest first.
    scripts: VecDeque<ScriptDone>,
    emitter: Option<Rc<EventEmitter>>,
}

/// The pages of the webviews that exist, by element id. A global of the app, so the engine of a
/// webview finds it, and so does a test.
#[derive(Clone, Default)]
pub(crate) struct TestWebviews(Rc<RefCell<HashMap<u64, TestPage>>>);

impl gpui::Global for TestWebviews {}

impl TestWebviews {
    pub(crate) fn with_page<R>(&self, id: u64, f: impl FnOnce(&mut TestPage) -> R) -> Result<R, String> {
        let mut pages = self.0.borrow_mut();
        let page = pages.get_mut(&id).ok_or_else(|| {
            format!("Element {id} has no page: a <webview> gets one when it is first painted")
        })?;
        Ok(f(page))
    }

    /// Makes the page report `event`, after `change` has moved the state it reports.
    pub(crate) fn report(
        &self,
        id: u64,
        change: impl FnOnce(&mut WebviewState),
        event: WebviewEvent,
    ) -> Result<(), String> {
        let emitter = self.with_page(id, |page| {
            change(&mut page.state);
            page.emitter.clone()
        })?;
        if let Some(emitter) = emitter {
            emitter.emit(event);
        }
        Ok(())
    }

    /// Settles the oldest script that waits.
    pub(crate) fn settle_script(&self, id: u64, outcome: ScriptOutcome) -> Result<(), String> {
        let done = self
            .with_page(id, |page| page.scripts.pop_front())?
            .ok_or_else(|| format!("Element {id} has no script waiting"))?;
        done(outcome);
        Ok(())
    }
}

pub(crate) struct TestBackend {
    id: u64,
    pages: TestWebviews,
}

impl TestBackend {
    pub(crate) fn new(id: u64, pages: &TestWebviews, emitter: Rc<EventEmitter>) -> Self {
        pages.0.borrow_mut().insert(id, TestPage { emitter: Some(emitter), ..TestPage::default() });
        Self { id, pages: pages.clone() }
    }

    fn command(&self, command: WebviewCommand) {
        self.pages
            .with_page(self.id, |page| page.commands.push(command))
            .expect("the page of a live backend");
    }
}

impl Drop for TestBackend {
    fn drop(&mut self) {
        self.pages.0.borrow_mut().remove(&self.id);
    }
}

impl WebviewBackend for TestBackend {
    fn load(&mut self, url: &str) -> Result<(), String> {
        self.command(WebviewCommand::Load(url.to_string()));
        Ok(())
    }

    fn reload(&mut self) {
        self.command(WebviewCommand::Reload);
    }

    fn stop(&mut self) {
        self.command(WebviewCommand::Stop);
    }

    fn go_back(&mut self) {
        self.command(WebviewCommand::GoBack);
    }

    fn go_forward(&mut self) {
        self.command(WebviewCommand::GoForward);
    }

    fn state(&self) -> WebviewState {
        self.pages.with_page(self.id, |page| page.state.clone()).expect("the page of a live backend")
    }

    fn execute_javascript(&mut self, code: &str, done: ScriptDone) {
        self.pages
            .with_page(self.id, |page| {
                page.commands.push(WebviewCommand::ExecuteJavaScript(code.to_string()));
                page.scripts.push_back(done);
            })
            .expect("the page of a live backend");
    }

    fn set_first_responder(&mut self, focused: bool) {
        self.command(WebviewCommand::SetFirstResponder(focused));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::element_tree::EventPayload;

    fn page() -> (TestWebviews, TestBackend, Arc<Mutex<Vec<EventPayload>>>) {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let sink = heard.clone();
        let emitter = EventEmitter::new(7);
        emitter.set_callback(Some(Arc::new(move |payload| sink.lock().unwrap().push(payload))));
        let pages = TestWebviews::default();
        let backend = TestBackend::new(7, &pages, emitter);
        (pages, backend, heard)
    }

    #[test]
    fn what_the_element_asks_is_recorded_in_order() {
        let (pages, mut backend, _) = page();
        backend.load("https://example.test/").unwrap();
        backend.reload();
        backend.set_first_responder(true);
        assert_eq!(
            pages.with_page(7, |page| page.commands.clone()).unwrap(),
            [
                WebviewCommand::Load("https://example.test/".into()),
                WebviewCommand::Reload,
                WebviewCommand::SetFirstResponder(true),
            ]
        );
    }

    #[test]
    fn a_reported_event_moves_the_state_and_reaches_the_element() {
        let (pages, backend, heard) = page();
        pages
            .report(7, |state| state.url = "https://example.test/a".into(), WebviewEvent::DidNavigate {
                url: "https://example.test/a".into(),
            })
            .unwrap();
        assert_eq!(backend.state().url, "https://example.test/a");
        let heard = heard.lock().unwrap();
        assert_eq!(heard.len(), 1);
        assert_eq!(heard[0].event_type, "did-navigate");
        assert_eq!(heard[0].element_id, 7.);
        assert_eq!(heard[0].url.as_deref(), Some("https://example.test/a"));
    }

    #[test]
    fn scripts_settle_oldest_first_and_only_once() {
        let (pages, mut backend, _) = page();
        let settled = Rc::new(RefCell::new(Vec::new()));
        for name in ["first", "second"] {
            let settled = settled.clone();
            backend.execute_javascript(name, Box::new(move |outcome| settled.borrow_mut().push((name, outcome))));
        }
        pages.settle_script(7, Ok(Some(serde_json::json!(1)))).unwrap();
        pages.settle_script(7, Err("boom".into())).unwrap();
        assert!(pages.settle_script(7, Ok(None)).is_err());
        assert_eq!(
            *settled.borrow(),
            [("first", Ok(Some(serde_json::json!(1)))), ("second", Err("boom".to_string()))]
        );
    }

    #[test]
    fn the_page_goes_with_the_backend() {
        let (pages, backend, _) = page();
        drop(backend);
        assert!(pages.with_page(7, |_| ()).is_err());
    }
}
