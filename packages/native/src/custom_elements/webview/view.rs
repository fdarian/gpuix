//! What JS asks of a `<webview>`, run on the view that owns the element.
use std::{cell::RefCell, rc::Rc};

use super::backend::{ScriptDone, WebviewState};
use super::Shared;
use crate::renderer::GpuixView;

/// A command of Electron's `<webview>` that returns nothing.
#[derive(Debug, PartialEq)]
pub(crate) enum WebviewAction {
    LoadUrl(String),
    Reload,
    Stop,
    GoBack,
    GoForward,
}

impl WebviewAction {
    pub(crate) fn parse(command: &str, argument: Option<String>) -> Result<Self, String> {
        match (command, argument) {
            ("loadURL", Some(url)) => Ok(Self::LoadUrl(url)),
            ("loadURL", None) => Err("loadURL needs a URL".to_string()),
            ("reload", None) => Ok(Self::Reload),
            ("stop", None) => Ok(Self::Stop),
            ("goBack", None) => Ok(Self::GoBack),
            ("goForward", None) => Ok(Self::GoForward),
            (command, _) => Err(format!("Unknown <webview> command {command}")),
        }
    }
}

impl GpuixView {
    pub(crate) fn webview_shared(&mut self, id: u64) -> Result<Rc<RefCell<Shared>>, String> {
        let props = {
            let tree = self.tree.lock().unwrap();
            let element = tree.elements.get(&id).ok_or_else(|| format!("Unknown element id {id}"))?;
            if element.element_type != "webview" {
                return Err(format!(
                    "This method is only valid on <webview>, got <{}>",
                    element.element_type
                ));
            }
            element.custom_props().clone()
        };
        self.custom_registry
            .with_element(id, "webview", &props, |element| element.as_webview().map(|webview| webview.shared()))
            .flatten()
            .ok_or_else(|| format!("Element {id} is not a <webview>"))
    }

    pub(crate) fn webview_do(&mut self, id: u64, action: WebviewAction) -> Result<(), String> {
        let shared = self.webview_shared(id)?;
        let mut shared = shared.borrow_mut();
        if let WebviewAction::LoadUrl(url) = &action {
            // Electron loads a URL given before the element is attached once it is.
            if let Err(error) = crate::platform::webview_url::classify(url) {
                return Err(error);
            }
            shared.load(url);
            return Ok(());
        }
        let page = shared.page()?;
        match action {
            WebviewAction::Reload => page.reload(),
            WebviewAction::Stop => page.stop(),
            WebviewAction::GoBack => page.go_back(),
            WebviewAction::GoForward => page.go_forward(),
            WebviewAction::LoadUrl(_) => unreachable!("handled above"),
        }
        Ok(())
    }

    pub(crate) fn webview_state(&mut self, id: u64) -> Result<WebviewState, String> {
        let shared = self.webview_shared(id)?;
        let mut shared = shared.borrow_mut();
        Ok(shared.page()?.state())
    }

    /// Runs `code` in the page and settles `done` with the outcome. When it cannot be run,
    /// `done` hears why before this returns.
    pub(crate) fn webview_execute_javascript(&mut self, id: u64, code: &str, done: ScriptDone) {
        let shared = match self.webview_shared(id) {
            Ok(shared) => shared,
            Err(message) => return done(Err(message)),
        };
        let mut shared = shared.borrow_mut();
        match shared.page() {
            Ok(page) => page.execute_javascript(code, done),
            Err(message) => done(Err(message)),
        }
    }

    /// Moves keyboard input to the page or back. The focus observer calls it when the element
    /// gains or loses focus.
    #[allow(dead_code)] // The focus handoff is the next slice; the backend side is in place.
    pub(crate) fn webview_set_focused(&mut self, id: u64, focused: bool) -> Result<(), String> {
        let shared = self.webview_shared(id)?;
        let mut shared = shared.borrow_mut();
        shared.page()?.set_first_responder(focused);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_parse_with_the_argument_they_need() {
        assert_eq!(
            WebviewAction::parse("loadURL", Some("https://example.test".into())),
            Ok(WebviewAction::LoadUrl("https://example.test".into()))
        );
        assert_eq!(WebviewAction::parse("reload", None), Ok(WebviewAction::Reload));
        assert_eq!(WebviewAction::parse("stop", None), Ok(WebviewAction::Stop));
        assert_eq!(WebviewAction::parse("goBack", None), Ok(WebviewAction::GoBack));
        assert_eq!(WebviewAction::parse("goForward", None), Ok(WebviewAction::GoForward));
    }

    #[test]
    fn a_command_without_its_argument_or_with_a_stray_one_is_refused() {
        assert!(WebviewAction::parse("loadURL", None).is_err());
        assert!(WebviewAction::parse("reload", Some("x".into())).is_err());
        assert!(WebviewAction::parse("print", None).is_err());
    }
}
