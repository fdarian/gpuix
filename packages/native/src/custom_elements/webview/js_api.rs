//! The JS side of the `<webview>` methods, shared by the live and the test renderer.
use std::{cell::RefCell, rc::Rc};

use napi::{bindgen_prelude::Object, Env, Error, JsDeferred, Result};
use napi_derive::napi;

use super::backend::{ScriptDone, WebviewState};

pub(crate) const UNSUPPORTED: &str = "webview is not supported on this platform";

/// What `getURL()`, `getTitle()`, `canGoBack()` and `canGoForward()` read.
#[napi(object)]
pub struct WebviewStateReading {
    pub url: String,
    pub title: String,
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

impl From<WebviewState> for WebviewStateReading {
    fn from(state: WebviewState) -> Self {
        Self {
            url: state.url,
            title: state.title,
            can_go_back: state.can_go_back,
            can_go_forward: state.can_go_forward,
        }
    }
}

type ScriptResolver = Box<dyn FnOnce(Env) -> Result<Option<String>> + Send>;
type ScriptDeferred = JsDeferred<Option<String>, ScriptResolver>;

/// The promise `executeJavaScript` returns. It settles with the JSON of the result, `null` for
/// `undefined`, or rejects with the message of the script's error.
pub(crate) struct ScriptPromise(Rc<RefCell<Option<ScriptDeferred>>>);

impl ScriptPromise {
    pub(crate) fn new<'env>(env: &'env Env) -> Result<(Self, Object<'env>)> {
        let (deferred, promise): (ScriptDeferred, Object<'env>) = env.create_deferred()?;
        Ok((Self(Rc::new(RefCell::new(Some(deferred)))), promise))
    }

    /// What the page calls with the outcome. Settles the promise once.
    pub(crate) fn completion(&self) -> ScriptDone {
        let slot = self.0.clone();
        Box::new(move |outcome| {
            let Some(deferred) = slot.borrow_mut().take() else { return };
            match outcome {
                Ok(value) => {
                    let json = value.map(|value| value.to_string());
                    deferred.resolve(Box::new(move |_env| Ok(json)));
                }
                Err(message) => deferred.reject(Error::from_reason(message)),
            }
        })
    }

    /// Rejects a promise the page was never asked to settle.
    pub(crate) fn reject(&self, message: String) {
        (self.completion())(Err(message));
    }
}
