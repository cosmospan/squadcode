//! The small menus that float over a page: the Run it later times by the
//! send button, the project picker on Today, a Settings drop-down and the
//! engines popover over the status bar.
//!
//! They all behave the same way: one is open at a time, and a click anywhere
//! outside it, or Esc, puts it away. The click still lands where it was
//! aimed, so nothing takes two clicks.

use gpui_kit::*;

use super::Shell;

/// Which floating menu is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pop {
    /// Run it later, by the thread's send button.
    Later,
    /// The project picker on Today.
    Folder,
    /// A Settings drop-down, by its id.
    Select(&'static str),
    /// The engines popover over the status bar.
    Usage,
}

/// Where a click went down, for a mouse or a tap; None for the keyboard.
fn down_at(e: &ClickEvent) -> Option<Point<Pixels>> {
    match e {
        ClickEvent::Mouse(m) => Some(m.down.position),
        ClickEvent::Touch(t) => Some(t.position),
        ClickEvent::Keyboard(_) => None,
    }
}

/// Whether the click that opens a menu is the same one that just put it
/// away: its button sits outside the menu, so pressing it closes the menu
/// first, and the click that follows mustn't open it straight back up.
fn same_click(shut: Option<(Pop, Point<Pixels>)>, pop: Pop, at: Option<Point<Pixels>>) -> bool {
    matches!((shut, at), (Some((p, was)), Some(now)) if p == pop && was == now)
}

impl Shell {
    /// Whether this menu is the one that's open.
    pub(super) fn pop_open(&self, pop: Pop) -> bool {
        self.pop == Some(pop)
    }

    /// Opens a menu, closing any other.
    pub(super) fn open_pop(&mut self, pop: Pop) {
        self.pop = Some(pop);
        self.pop_shut = None;
    }

    /// Closes whichever menu is open. Whether one was.
    pub(super) fn close_pop(&mut self) -> bool {
        self.pop_shut = None;
        self.pop.take().is_some()
    }

    /// Closes this menu if it's the one open.
    pub(super) fn close_this_pop(&mut self, pop: Pop) {
        if self.pop == Some(pop) {
            self.pop = None;
        }
    }

    /// A menu's button was clicked: open it, or close it when it's open.
    /// Whether it's open now.
    pub(super) fn toggle_pop(&mut self, pop: Pop, e: &ClickEvent) -> bool {
        if same_click(self.pop_shut.take(), pop, down_at(e)) {
            return false;
        }
        if self.pop == Some(pop) {
            self.pop = None;
        } else {
            self.open_pop(pop);
        }
        self.pop == Some(pop)
    }

    /// Makes `el` the open menu's panel: clicks inside it stay inside, and a
    /// mouse-down anywhere outside it closes it.
    pub(super) fn pop_panel<E>(&self, pop: Pop, el: E, cx: &mut Context<Self>) -> E
    where
        E: InteractiveElement,
    {
        el.occlude()
            .on_mouse_down_out(cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                if this.pop == Some(pop) {
                    this.pop = None;
                    this.pop_shut = Some((pop, e.position));
                    cx.notify();
                }
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::{same_click, Pop};
    use gpui_kit::{point, px};

    #[test]
    fn the_click_that_closed_it_doesnt_open_it_again() {
        let at = point(px(10.), px(20.));
        let elsewhere = point(px(11.), px(20.));
        assert!(same_click(Some((Pop::Later, at)), Pop::Later, Some(at)));
        // A later click on the button opens it as usual.
        assert!(!same_click(
            Some((Pop::Later, at)),
            Pop::Later,
            Some(elsewhere)
        ));
        // Closing one menu by clicking another's button opens that one.
        assert!(!same_click(Some((Pop::Later, at)), Pop::Folder, Some(at)));
        assert!(!same_click(None, Pop::Later, Some(at)));
        assert!(!same_click(Some((Pop::Later, at)), Pop::Later, None));
    }
}
