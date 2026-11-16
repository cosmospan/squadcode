//! A contact sheet of the viewfinder Peek: `cargo run --release -p squad-holo --example viewfinder_sheet out.png [t]`.
//!
//! The top rows are every state on the big framing; then the crew in their
//! colours; then a row of the person's choices (eyes, lights, corners);
//! then avatar sizes along the bottom.
use squad_holo::bubble::Tint;
use squad_holo::fin::{Fin, FinPose, Line, Outfit, State};
use squad_holo::viewfinder::{Corners, Eyes, Kit, Lens, Light};
use squad_holo::{Framing, ViewfinderRenderer};

const CREW: [(Tint, bool); 8] = [
    (Tint::VIOLET, true),
    (Tint::ORANGE, false),
    (Tint::RED, false),
    (Tint::AMBER, false),
    (Tint::GREEN, false),
    (Tint::VIOLET, false),
    (Tint::BLUE, false),
    (Tint::TEAL, false),
];

fn pose(tint: Tint, lead: bool, state: State) -> FinPose {
    FinPose {
        fin: Fin {
            tint,
            line: Line::default(),
            ears: lead,
            outfit: Outfit::PLAIN,
            kit: Default::default(),
        },
        state,
        shine: false,
        gaze: (0, 0),
        offset: 0,
        from: None,
        ink: None,
    }
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or("viewfinder.png".into());
    let t: f64 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.7);
    let cell = 160usize;
    let cols = 8;
    let mut cells: Vec<FinPose> = State::ALL
        .iter()
        .map(|&s| pose(Tint::VIOLET, true, s))
        .collect();
    while !cells.len().is_multiple_of(cols) {
        cells.push(pose(Tint::VIOLET, true, State::Idle));
    }
    for (tint, lead) in CREW {
        cells.push(pose(tint, lead, State::Done));
    }
    let k = Kit::default();
    let kits = [
        (
            Tint::BLUE,
            Kit {
                eyes: Eyes::Dot,
                ..k
            },
        ),
        (
            Tint::ORANGE,
            Kit {
                eyes: Eyes::Block,
                ..k
            },
        ),
        (
            Tint::TEAL,
            Kit {
                eyes: Eyes::Diamond,
                ..k
            },
        ),
        (
            Tint::VIOLET,
            Kit {
                eyes: Eyes::Visor,
                ..k
            },
        ),
        (
            Tint::AMBER,
            Kit {
                light: Light::Glow,
                ..k
            },
        ),
        (
            Tint::GREEN,
            Kit {
                corners: Corners::Tinted,
                ..k
            },
        ),
        (
            Tint::RED,
            Kit {
                lens: Some(Lens::Round),
                eyes: Eyes::Visor,
                light: Light::Rec,
                ..k
            },
        ),
        (
            Tint::BLUE,
            Kit {
                lens: Some(Lens::Chamfer),
                eyes: Eyes::Diamond,
                light: Light::Glow,
                corners: Corners::Tinted,
            },
        ),
    ];
    for (tint, kit) in kits {
        let mut p = pose(tint, false, State::Idle);
        p.fin.kit = kit;
        cells.push(p);
    }
    let rows = cells.len().div_ceil(cols);
    let (w, h) = (cols * cell, rows * cell + 80);
    let mut img = vec![0u8; w * h * 4];
    for p in img.chunks_mut(4) {
        p.copy_from_slice(&[0x12, 0x12, 0x17, 0xFF]);
    }
    let mut r = ViewfinderRenderer::new(cell as u16);
    for (i, p) in cells.iter().enumerate() {
        let px = r.draw(p, t, Framing::Full).to_vec();
        blit(&mut img, w, &px, cell, (i % cols) * cell, (i / cols) * cell);
    }
    let mut x = 10;
    for size in [22usize, 26, 34, 56] {
        let mut r = ViewfinderRenderer::new(size as u16);
        for (tint, lead) in CREW.iter().take(4) {
            let px = r
                .draw(&pose(*tint, *lead, State::Idle), t, Framing::Bust)
                .to_vec();
            blit(&mut img, w, &px, size, x, h - 70);
            x += size + 4;
        }
        x += 20;
    }
    let f = std::fs::File::create(&out).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&img).unwrap();
}

fn blit(img: &mut [u8], w: usize, px: &[u8], n: usize, ox: usize, oy: usize) {
    for y in 0..n {
        for x in 0..n {
            let s = (y * n + x) * 4;
            let d = ((oy + y) * w + ox + x) * 4;
            let a = px[s + 3] as u32;
            for c in 0..3 {
                img[d + c] =
                    (px[s + c] as u32 + img[d + c] as u32 * (255 - a) / 255).min(255) as u8;
            }
            img[d + 3] = 255;
        }
    }
}
