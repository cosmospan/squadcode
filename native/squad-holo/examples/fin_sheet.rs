//! A contact sheet of Fin: `cargo run --release -p squad-holo --example fin_sheet out.png [t]`.
use squad_holo::bubble::Tint;
use squad_holo::fin::{Eyes, Fin, FinPose, FinRenderer, Ink, Line, Outfit, State, Weight, ROLES};
use squad_holo::Framing;

fn main() {
    let out = std::env::args().nth(1).unwrap_or("fin.png".into());
    let t: f64 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.3);
    let cell = 180usize;
    let cols = 8;
    let mut cells: Vec<(FinPose, usize)> = Vec::new();
    // Every state in the default line, then a row for each other look.
    let mut lines = vec![Line::default()];
    lines.extend(Weight::ALL.iter().map(|&weight| Line {
        weight,
        ..Line::default()
    }));
    lines.extend(Ink::ALL.iter().map(|&ink| Line {
        ink,
        ..Line::default()
    }));
    lines.push(Line {
        eyes: Eyes::Dots,
        ..Line::default()
    });
    let mut seen = Vec::new();
    lines.retain(|l| {
        let new = !seen.contains(l);
        seen.push(*l);
        new
    });
    for (i, &line) in lines.iter().enumerate() {
        let states: &[State] = if i == 0 {
            &State::ALL
        } else {
            &State::ALL[..8]
        };
        for &s in states {
            let fin = Fin {
                tint: Tint::RED,
                line,
                // Peek, with its ears, on the first row.
                ears: i == 0,
                outfit: Outfit::PLAIN,
                kit: Default::default(),
            };
            cells.push((
                FinPose {
                    fin,
                    state: s,
                    shine: false,
                    gaze: (0, 0),
                    offset: 0,
                    from: None,
                },
                cell,
            ));
        }
    }
    for r in ROLES {
        let fin = Fin {
            tint: r.tint,
            line: Line::default(),
            ears: false,
            outfit: r.outfit,
            kit: Default::default(),
        };
        cells.push((
            FinPose {
                fin,
                state: State::Idle,
                shine: false,
                gaze: (0, 0),
                offset: 0,
                from: None,
            },
            cell,
        ));
    }
    let rows = cells.len().div_ceil(cols);
    let (w, h) = (cols * cell, rows * cell + 80);
    let mut img = vec![0u8; w * h * 4];
    for p in img.chunks_mut(4) {
        p.copy_from_slice(&[0x12, 0x12, 0x17, 0xFF]);
    }
    let mut r = FinRenderer::new(cell as u16);
    for (i, (pose, _)) in cells.iter().enumerate() {
        let px = r.draw(pose, t, Framing::Full).to_vec();
        blit(&mut img, w, &px, cell, (i % cols) * cell, (i / cols) * cell);
    }
    let mut x = 10;
    for size in [22usize, 26, 34, 56] {
        let mut r = FinRenderer::new(size as u16);
        for tint in Tint::ALL {
            let fin = Fin {
                tint,
                line: Line::default(),
                ears: false,
                outfit: Outfit::PLAIN,
                kit: Default::default(),
            };
            let px = r
                .draw(
                    &FinPose {
                        fin,
                        state: State::Idle,
                        shine: false,
                        gaze: (0, 0),
                        offset: 0,
                        from: None,
                    },
                    t,
                    Framing::Bust,
                )
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
