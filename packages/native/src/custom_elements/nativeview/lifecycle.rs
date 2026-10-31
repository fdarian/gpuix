//! When JS is told that the native surface of a `<nativeview>` is there and when it goes: the
//! rules, apart from the surface and the emission, so that they can be checked on their own.

/// `attach` once per element when the surface exists and JS listens; `detach` once, iff `attach`
/// was told, when the surface goes. An element never attaches again after its surface went:
/// its id is not reused.
#[derive(Debug, Default)]
pub(super) struct Lifecycle {
    /// The handle `attach` carried, until `detach` carries it.
    attached: Option<usize>,
    closed: bool,
}

impl Lifecycle {
    /// The surface exists, with `handle` for its content view, and `listens` is whether JS
    /// listens for `attach` now. Whether JS is to be told `attach`, with `handle`, now.
    #[must_use]
    pub(super) fn offer(&mut self, handle: usize, listens: bool) -> bool {
        if self.closed || self.attached.is_some() || !listens {
            return false;
        }
        self.attached = Some(handle);
        true
    }

    /// The surface is about to go. The handle to tell JS `detach` with, if `attach` was told, to
    /// be done before the surface is dropped: the listener of `detach` is the last that can
    /// still use the view.
    #[must_use]
    pub(super) fn close(&mut self) -> Option<usize> {
        self.closed = true;
        self.attached.take()
    }

    pub(super) fn is_closed(&self) -> bool {
        self.closed
    }
}

/// Whether the platform can make a surface, learned the first time it is asked for one.
#[derive(Debug, Default)]
pub(super) struct Availability {
    refusal: Option<String>,
    warnings: usize,
}

impl Availability {
    /// Whether to try making a surface: not after the platform refused one.
    pub(super) fn may_try(&self) -> bool {
        self.refusal.is_none()
    }

    /// The platform refused the surface. Returns the reason on the first refusal and nothing
    /// after it, so that the element warns once however many frames it is painted in.
    pub(super) fn refused(&mut self, reason: String) -> Option<&str> {
        if self.refusal.is_some() {
            return None;
        }
        self.warnings += 1;
        self.refusal = Some(reason);
        self.refusal.as_deref()
    }

    pub(super) fn warnings(&self) -> usize {
        self.warnings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What JS was told, as the element tells it.
    #[derive(Default)]
    struct Told {
        lifecycle: Lifecycle,
        told: Vec<String>,
    }

    impl Told {
        fn offer(&mut self, handle: usize, listens: bool) {
            if self.lifecycle.offer(handle, listens) {
                self.told.push(format!("attach {handle}"));
            }
        }

        fn close(&mut self) {
            if let Some(handle) = self.lifecycle.close() {
                self.told.push(format!("detach {handle}"));
            }
        }
    }

    #[test]
    fn attach_is_told_once_when_the_surface_exists_and_js_listens() {
        let mut view = Told::default();
        view.offer(4, true);
        view.offer(4, true);
        view.offer(9, true);
        assert_eq!(view.told, ["attach 4"]);
    }

    #[test]
    fn attach_waits_for_a_listener() {
        let mut view = Told::default();
        view.offer(4, false);
        view.offer(4, false);
        assert!(view.told.is_empty());
        view.offer(4, true);
        assert_eq!(view.told, ["attach 4"]);
    }

    #[test]
    fn a_listener_that_goes_does_not_undo_attach() {
        let mut view = Told::default();
        view.offer(4, true);
        view.offer(4, false);
        view.close();
        assert_eq!(view.told, ["attach 4", "detach 4"]);
    }

    #[test]
    fn detach_is_told_once_with_the_handle_attach_carried() {
        let mut view = Told::default();
        view.offer(4, true);
        view.close();
        view.close();
        assert_eq!(view.told, ["attach 4", "detach 4"]);
    }

    #[test]
    fn detach_is_not_told_for_an_attach_that_was_not() {
        let mut view = Told::default();
        view.offer(4, false);
        view.close();
        assert!(view.told.is_empty());
    }

    #[test]
    fn nothing_attaches_after_the_surface_went() {
        let mut view = Told::default();
        view.offer(4, true);
        view.close();
        view.offer(5, true);
        assert_eq!(view.told, ["attach 4", "detach 4"]);
        assert!(view.lifecycle.is_closed());
    }

    #[test]
    fn an_element_closed_before_its_surface_never_attaches() {
        let mut view = Told::default();
        view.close();
        view.offer(4, true);
        assert!(view.told.is_empty());
    }

    #[test]
    fn a_refusal_warns_once_and_is_never_tried_again() {
        let mut availability = Availability::default();
        assert!(availability.may_try());
        assert_eq!(availability.refused("no compositor".to_string()), Some("no compositor"));
        assert_eq!(availability.refused("no compositor".to_string()), None);
        assert_eq!(availability.refused("other".to_string()), None);
        assert!(!availability.may_try());
        assert_eq!(availability.warnings(), 1);
    }

    #[test]
    fn an_element_that_was_never_refused_has_not_warned() {
        assert_eq!(Availability::default().warnings(), 0);
    }
}
