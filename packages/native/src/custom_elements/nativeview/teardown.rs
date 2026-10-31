//! Ending the `<nativeview>` elements a batch destroys, in the batch.
//!
//! The registry destroys an element's state when a frame is built and finds the element gone,
//! which is after JS has dropped its handlers (`onDestroyed` runs when `applyBatch` returns). JS
//! has to hear `detach` while it still can, so the elements a batch destroys are ended by the
//! batch, by the id they have in their tree. The registry's own destruction is the backstop:
//! ending an element twice is the same as once.
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::{Rc, Weak},
};

use super::Shared;

type Key = (u64, u64);

thread_local! {
    static LIVE: RefCell<HashMap<Key, Weak<RefCell<Shared>>>> = RefCell::new(HashMap::new());
}

/// Makes `shared` findable as element `key.1` of tree `key.0`.
pub(super) fn track(key: Key, shared: &Rc<RefCell<Shared>>) {
    shared.borrow_mut().tracked = Some(key);
    LIVE.with(|live| live.borrow_mut().insert(key, Rc::downgrade(shared)));
}

pub(super) fn forget(key: Key) {
    // The thread's locals may be gone when the state of a window is dropped at exit.
    let _ = LIVE.try_with(|live| live.borrow_mut().remove(&key));
}

/// Ends the nativeviews among `ids`, the elements a batch destroyed in tree `tree`: `detach` is
/// told to JS and then the surfaces go. Call it with the tree unlocked: the listener runs JS.
pub(crate) fn destroyed(tree: u64, ids: &[f64]) {
    let ended: Vec<Rc<RefCell<Shared>>> = LIVE.with(|live| {
        let mut live = live.borrow_mut();
        ids.iter()
            .filter_map(|id| live.remove(&(tree, *id as u64)))
            .filter_map(|shared| shared.upgrade())
            .collect()
    });
    for shared in ended {
        shared.borrow_mut().take_down();
    }
}

/// [`destroyed`] as a call from JS, which is what lets the listener of `detach` run
/// synchronously inside `applyBatch`.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn tell_destroyed(tree: u64, ids: &[f64]) -> napi::Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    crate::events::sync_dispatch::native_call(|| {
        destroyed(tree, ids);
        Ok(())
    })
}
