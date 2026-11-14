//! # squad-holo
//!
//! Squad Code's crew, drawn as glass cubes floating over projectors.
//!
//! A bot is a [`Look`] (its cube, colours, face and props). Its pose is a pure
//! function of time: [`render`] draws a look at time `t` with Vello's CPU
//! renderer, so the same rig plays identically in the app, in tests and in
//! exported previews. The motion is ported one to one from the Holo v2
//! design (`runo-figma/holo-2d-v2.html`), keyframes, easing and all.
//!
//! The GPU path from the tech plan (one Vello scene for the whole crew shown
//! as a GPUI surface) will slot in behind the same [`draw::bot`] call; this
//! CPU path is the fallback the plan keeps for every platform.

pub mod anim;
pub mod bubble;
pub mod draw;
pub mod fin;
pub mod look;
pub mod viewfinder;

pub use bubble::{Bubble, BubbleRenderer, Eyes, Layout, Pose, Sway, Tint};
pub use draw::Detail;
pub use fin::{Fin, FinPose, FinRenderer};
pub use look::{custom, Bot, Eye, Look, Mood, Motion, Mouth, Palette, Props, Rgb, Shape};
pub use viewfinder::ViewfinderRenderer;

use vello_cpu::kurbo::Affine;
use vello_cpu::{Pixmap, RenderContext, Resources};

/// A crew member ready to draw: one of the bubble crew, Fin, or the viewfinder crew.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Figure {
    Bubble(Pose),
    Fin(FinPose),
    /// The same pose as Fin, drawn as Peek the viewfinder: four corner marks
    /// cut in its own lens style round two eyes in its own colour.
    Viewfinder(FinPose),
}

impl Figure {
    /// Busy enough to need a quick frame rate.
    pub fn lively(&self) -> bool {
        match self {
            Figure::Bubble(p) => {
                p.sparks
                    || p.ask
                    || p.shine
                    || matches!(p.layout, Layout::Think | Layout::Busy | Layout::Splash)
            }
            Figure::Fin(p) => p.lively(),
            Figure::Viewfinder(p) => viewfinder::lively(p),
        }
    }

    /// Marks it as hovered or worth a moment of attention.
    pub fn shine(mut self, on: bool) -> Self {
        match &mut self {
            Figure::Bubble(p) => p.shine |= on,
            Figure::Fin(p) | Figure::Viewfinder(p) => p.shine |= on,
        }
        self
    }

    /// Easing in from the face of `state`, which it left at `since` seconds
    /// on the drawing clock. Only Peek eases; bubbles snap.
    pub fn easing_from(self, state: fin::State, since: f64) -> Self {
        match self {
            Figure::Fin(p) => Figure::Fin(p.easing_from(state, since)),
            Figure::Viewfinder(p) => Figure::Viewfinder(p.easing_from(state, since)),
            other => other,
        }
    }

    /// The state the figure shows, for Peek.
    pub fn fin_state(&self) -> Option<fin::State> {
        match self {
            Figure::Fin(p) | Figure::Viewfinder(p) => Some(p.state),
            Figure::Bubble(_) => None,
        }
    }
}

/// Draws either kind of figure at one size, keeping each renderer's buffers.
pub struct CrewRenderer {
    px: u16,
    bubble: Option<BubbleRenderer>,
    fin: Option<FinRenderer>,
    viewfinder: Option<ViewfinderRenderer>,
}

impl CrewRenderer {
    pub fn new(px: u16) -> Self {
        CrewRenderer {
            px,
            bubble: None,
            fin: None,
            viewfinder: None,
        }
    }

    /// Premultiplied RGBA8 pixels of `figure` at time `t`.
    pub fn draw(&mut self, figure: &Figure, t: f64, framing: Framing) -> &[u8] {
        let px = self.px;
        match figure {
            Figure::Bubble(p) => self
                .bubble
                .get_or_insert_with(|| BubbleRenderer::new(px))
                .draw(p, t, framing),
            Figure::Fin(p) => self
                .fin
                .get_or_insert_with(|| FinRenderer::new(px))
                .draw(p, t, framing),
            Figure::Viewfinder(p) => self
                .viewfinder
                .get_or_insert_with(|| ViewfinderRenderer::new(px))
                .draw(p, t, framing),
        }
    }
}

/// How much of the 200 × 200 view box to show.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Framing {
    /// The whole scene: projector, beam, glow and props.
    Full,
    /// A tight square around the cube, for avatars.
    Bust,
}

impl Framing {
    fn view(self, l: &Look) -> (f64, f64, f64) {
        match self {
            Framing::Full => (0.0, 0.0, 200.0),
            Framing::Bust => {
                let h = l.total_height();
                let side = (2.0 * l.w).max(h) + 14.0;
                let cy = l.top() + h / 2.0 - 2.0;
                (100.0 - side / 2.0, cy - side / 2.0, side)
            }
        }
    }
}

/// The detail that suits a bot drawn `px` pixels wide.
pub fn detail_for(px: u32, framing: Framing) -> Detail {
    match framing {
        Framing::Full if px >= 96 => Detail::FULL,
        Framing::Full => Detail {
            projector: true,
            glow: true,
            effects: false,
            props: false,
        },
        Framing::Bust if px >= 40 => Detail::MEDIUM,
        Framing::Bust => Detail::TINY,
    }
}

/// A reusable renderer: keeps Vello's buffers between frames.
pub struct Renderer {
    ctx: RenderContext,
    resources: Resources,
    pixmap: Pixmap,
}

impl Renderer {
    pub fn new(px: u16) -> Self {
        Renderer {
            ctx: RenderContext::new(px, px),
            resources: Resources::new(),
            pixmap: Pixmap::new(px, px),
        }
    }

    pub fn size(&self) -> u16 {
        self.ctx.width()
    }

    /// Draws `look` at time `t` and returns premultiplied RGBA8 pixels.
    pub fn draw(&mut self, look: &Look, t: f64, framing: Framing, detail: Detail) -> &[u8] {
        let px = self.ctx.width();
        self.ctx.reset();
        let (x0, y0, side) = framing.view(look);
        let k = px as f64 / side;
        let xf = Affine::scale(k) * Affine::translate((-x0, -y0));
        {
            let mut pen = draw::Pen::new(&mut self.ctx, xf);
            draw::bot(&mut pen, look, t, detail);
        }
        self.ctx.flush();
        self.pixmap = Pixmap::new(px, px);
        self.ctx.render(&mut self.pixmap, &mut self.resources);
        self.pixmap.data_as_u8_slice()
    }

    /// The last frame drawn.
    pub fn pixmap(&self) -> &Pixmap {
        &self.pixmap
    }
}

/// One-shot convenience: premultiplied RGBA8 pixels of `look` at time `t`.
pub fn render(look: &Look, t: f64, px: u16, framing: Framing) -> Vec<u8> {
    let mut r = Renderer::new(px);
    let detail = detail_for(px as u32, framing);
    r.draw(look, t, framing, detail).to_vec()
}

/// One-shot PNG of `look` at time `t`.
pub fn render_png(look: &Look, t: f64, px: u16, framing: Framing) -> Vec<u8> {
    let mut r = Renderer::new(px);
    let detail = detail_for(px as u32, framing);
    r.draw(look, t, framing, detail);
    r.pixmap.clone().into_png().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coverage(px: &[u8]) -> usize {
        px.chunks(4).filter(|p| p[3] > 128).count()
    }

    #[test]
    fn every_bot_and_mood_draws_something() {
        for bot in Bot::ALL {
            for mood in Mood::ALL {
                let px = render(&bot.look(mood), 1.3, 96, Framing::Full);
                assert!(coverage(&px) > 500, "{bot:?} {mood:?} drew almost nothing");
            }
        }
    }

    #[test]
    fn motion_changes_the_frame() {
        let l = Bot::Builder.look(Mood::Working);
        assert_ne!(
            render(&l, 0.0, 64, Framing::Full),
            render(&l, 0.4, 64, Framing::Full)
        );
    }

    #[test]
    fn same_time_same_frame() {
        let l = Bot::Checker.signature();
        assert_eq!(
            render(&l, 2.5, 64, Framing::Full),
            render(&l, 2.5, 64, Framing::Full)
        );
    }

    #[test]
    fn bust_fills_more_of_the_frame() {
        let l = Bot::Guard.signature();
        let full = coverage(&render(&l, 0.0, 48, Framing::Full));
        let bust = coverage(&render(&l, 0.0, 48, Framing::Bust));
        assert!(bust > full);
    }
}
