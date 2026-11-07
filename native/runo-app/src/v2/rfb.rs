//! A small VNC (RFB 3.8) client, so the server's screen shows inside Squad.
//!
//! It speaks just what x11vnc needs: VNC password auth, a 32-bit true-colour
//! frame, and the Raw, CopyRect, ZRLE and desktop-size encodings. ZRLE keeps a
//! busy screen light over an SSH tunnel; Raw is the fallback every server has.
//! A thread reads the server and keeps [`Frame`] current; the window takes a
//! copy when it changed and sends the mouse and keys back with [`Viewer`].

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The screen as last drawn, as BGRA with alpha 255 (what GPUI paints).
#[derive(Default)]
pub struct Frame {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

/// How the connection is.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum State {
    #[default]
    Connecting,
    Live,
    /// It ended, and why.
    Closed(String),
}

/// A live connection to a VNC server.
pub struct Viewer {
    pub frame: Arc<Mutex<Frame>>,
    state: Arc<Mutex<State>>,
    /// Bumped each time the frame changes, so the window redraws only then.
    pub version: Arc<AtomicU64>,
    out: Arc<Mutex<Option<TcpStream>>>,
    stop: Arc<AtomicBool>,
    buttons: u8,
}

impl Viewer {
    /// Connects to `127.0.0.1:port` with `password`, on a thread of its own.
    pub fn connect(port: u16, password: String) -> Viewer {
        let v = Viewer {
            frame: Arc::default(),
            state: Arc::default(),
            version: Arc::default(),
            out: Arc::default(),
            stop: Arc::default(),
            buttons: 0,
        };
        let (frame, state, version, out, stop) = (
            v.frame.clone(),
            v.state.clone(),
            v.version.clone(),
            v.out.clone(),
            v.stop.clone(),
        );
        std::thread::spawn(move || {
            let why = match run(port, &password, &frame, &version, &out, &stop) {
                Ok(()) => "The screen closed.".to_string(),
                Err(e) => e,
            };
            *state.lock().unwrap_or_else(|e| e.into_inner()) = State::Closed(why);
            version.fetch_add(1, Ordering::Relaxed);
        });
        // Live once the first frame is in; `run` can't reach `state` sooner
        // without another lock in its loop, so the window asks `state()`.
        v
    }

    pub fn state(&self) -> State {
        let s = self.state.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if s == State::Connecting && self.frame.lock().map(|f| f.w > 0).unwrap_or(false) {
            return State::Live;
        }
        s
    }

    fn send(&self, bytes: &[u8]) {
        if let Some(s) = self.out.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            let _ = s.write_all(bytes);
        }
    }

    /// The pointer at (`x`, `y`) on the screen, with `button` (1 left, 2
    /// middle, 3 right) pressed or released when given.
    pub fn pointer(&mut self, x: u16, y: u16, button: Option<(u8, bool)>) {
        if let Some((b, down)) = button {
            let bit = 1u8 << (b.clamp(1, 8) - 1);
            if down {
                self.buttons |= bit;
            } else {
                self.buttons &= !bit;
            }
        }
        self.send(&pointer_event(self.buttons, x, y));
    }

    /// A wheel turn at (`x`, `y`): up when `up`, as a press and release of
    /// button 4 or 5.
    pub fn wheel(&mut self, x: u16, y: u16, up: bool) {
        let bit = if up { 1u8 << 3 } else { 1u8 << 4 };
        self.send(&pointer_event(self.buttons | bit, x, y));
        self.send(&pointer_event(self.buttons, x, y));
    }

    /// A key, by its X keysym, pressed or released.
    pub fn key(&self, keysym: u32, down: bool) {
        let mut m = vec![4u8, down as u8, 0, 0];
        m.extend_from_slice(&keysym.to_be_bytes());
        self.send(&m);
    }

    /// Text for the server's clipboard (what a paste there pastes).
    pub fn clipboard(&self, text: &str) {
        let latin: Vec<u8> = text
            .chars()
            .map(|c| if (c as u32) < 256 { c as u8 } else { b'?' })
            .collect();
        let mut m = vec![6u8, 0, 0, 0];
        m.extend_from_slice(&(latin.len() as u32).to_be_bytes());
        m.extend_from_slice(&latin);
        self.send(&m);
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(s) = self.out.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }
}

fn pointer_event(buttons: u8, x: u16, y: u16) -> [u8; 6] {
    let (x, y) = (x.to_be_bytes(), y.to_be_bytes());
    [5, buttons, x[0], x[1], y[0], y[1]]
}

/// The X keysym for a key as GPUI names it, and the character it typed.
pub fn keysym(key: &str, typed: Option<&str>) -> Option<u32> {
    let named = match key {
        "enter" => 0xff0d,
        "escape" => 0xff1b,
        "backspace" => 0xff08,
        "tab" => 0xff09,
        "delete" => 0xffff,
        "insert" => 0xff63,
        "home" => 0xff50,
        "end" => 0xff57,
        "pageup" => 0xff55,
        "pagedown" => 0xff56,
        "left" => 0xff51,
        "up" => 0xff52,
        "right" => 0xff53,
        "down" => 0xff54,
        "space" => 0x20,
        _ => 0,
    };
    if named != 0 {
        return Some(named);
    }
    if let Some(n) = key.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=12).contains(&n) {
            return Some(0xffbe + n - 1);
        }
    }
    // What it typed, else the key itself (with Ctrl held nothing is typed).
    let text = typed.filter(|t| !t.is_empty()).unwrap_or(key);
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let cp = ch as u32;
    Some(
        if (0x20..0x7f).contains(&cp) || (0xa0..0x100).contains(&cp) {
            cp
        } else {
            0x0100_0000 + cp
        },
    )
}

/// X keysyms of the modifier keys: Shift, Control, Alt.
pub const SHIFT: u32 = 0xffe1;
pub const CONTROL: u32 = 0xffe3;
pub const ALT: u32 = 0xffe9;

// ---------- the protocol ----------

struct Conn {
    s: TcpStream,
}

impl Conn {
    fn exact(&mut self, n: usize) -> Result<Vec<u8>, String> {
        let mut b = vec![0; n];
        self.s.read_exact(&mut b).map_err(|e| match e.kind() {
            std::io::ErrorKind::UnexpectedEof => "the server closed the screen".to_string(),
            _ => e.to_string(),
        })?;
        Ok(b)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.exact(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        let b = self.exact(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, String> {
        let b = self.exact(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn put(&mut self, b: &[u8]) -> Result<(), String> {
        self.s.write_all(b).map_err(|e| e.to_string())
    }
}

fn run(
    port: u16,
    password: &str,
    frame: &Mutex<Frame>,
    version: &AtomicU64,
    out: &Mutex<Option<TcpStream>>,
    stop: &AtomicBool,
) -> Result<(), String> {
    let s = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    let _ = s.set_nodelay(true);
    let mut c = Conn { s };
    handshake(&mut c, password)?;

    // ClientInit (shared, so Screen Sharing and Squad can both watch), ServerInit.
    c.put(&[1])?;
    let w = c.u16()? as u32;
    let h = c.u16()? as u32;
    let _server_format = c.exact(16)?;
    let n = c.u32()? as usize;
    c.exact(n.min(1 << 16))?;
    {
        let mut f = frame.lock().unwrap_or_else(|e| e.into_inner());
        *f = Frame {
            w,
            h,
            px: opaque(w as usize * h as usize),
        };
    }

    // 32 bits a pixel, little-endian, red at bit 16: B, G, R, X in memory.
    let fmt = vec![
        0u8, 0, 0, 0, 32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 16, 8, 0, 0, 0, 0,
    ];
    c.put(&fmt)?;
    // ZRLE, CopyRect, Raw, and the desktop changing size.
    let encs: [i32; 4] = [16, 1, 0, -223];
    let mut m = vec![2u8, 0];
    m.extend_from_slice(&(encs.len() as u16).to_be_bytes());
    for e in encs {
        m.extend_from_slice(&e.to_be_bytes());
    }
    c.put(&m)?;
    *out.lock().unwrap_or_else(|e| e.into_inner()) =
        Some(c.s.try_clone().map_err(|e| e.to_string())?);
    request(&mut c, w, h, false)?;
    version.fetch_add(1, Ordering::Relaxed);

    let mut zrle = Zrle::default();
    while !stop.load(Ordering::Relaxed) {
        match c.u8()? {
            0 => {
                c.u8()?;
                let rects = c.u16()?;
                for _ in 0..rects {
                    let (x, y, rw, rh) = (c.u16()?, c.u16()?, c.u16()?, c.u16()?);
                    let enc = c.u32()? as i32;
                    rect(
                        &mut c,
                        frame,
                        &mut zrle,
                        enc,
                        x as usize,
                        y as usize,
                        rw as usize,
                        rh as usize,
                    )?;
                }
                version.fetch_add(1, Ordering::Relaxed);
                let (w, h) = {
                    let f = frame.lock().unwrap_or_else(|e| e.into_inner());
                    (f.w, f.h)
                };
                // Ask for the next change; a short pause keeps a busy screen
                // from flooding the tunnel.
                std::thread::sleep(Duration::from_millis(15));
                request(&mut c, w, h, true)?;
            }
            1 => {
                c.exact(3)?;
                let n = c.u16()? as usize;
                c.exact(n * 6)?;
            }
            2 => {}
            3 => {
                c.exact(3)?;
                let n = c.u32()? as usize;
                c.exact(n.min(1 << 24))?;
            }
            t => {
                return Err(format!(
                    "the server sent a message Squad doesn't know ({t})"
                ))
            }
        }
    }
    Ok(())
}

fn opaque(pixels: usize) -> Vec<u8> {
    let mut v = vec![0u8; pixels * 4];
    for p in v.as_chunks_mut::<4>().0 {
        p[3] = 255;
    }
    v
}

fn request(c: &mut Conn, w: u32, h: u32, incremental: bool) -> Result<(), String> {
    let (w, h) = ((w as u16).to_be_bytes(), (h as u16).to_be_bytes());
    c.put(&[3, incremental as u8, 0, 0, 0, 0, w[0], w[1], h[0], h[1]])
}

fn handshake(c: &mut Conn, password: &str) -> Result<(), String> {
    let hello = c.exact(12)?;
    if !hello.starts_with(b"RFB ") {
        return Err("that isn't a screen-sharing server".into());
    }
    c.put(b"RFB 003.008\n")?;
    let n = c.u8()? as usize;
    if n == 0 {
        let len = c.u32()? as usize;
        let why = String::from_utf8_lossy(&c.exact(len.min(4096))?).to_string();
        return Err(format!("the server refused: {why}"));
    }
    let types = c.exact(n)?;
    if types.contains(&2) {
        c.put(&[2])?;
        let challenge = c.exact(16)?;
        c.put(&vnc_auth(password, &challenge))?;
    } else if types.contains(&1) {
        c.put(&[1])?;
    } else {
        return Err("the server wants a kind of sign-in Squad doesn't speak".into());
    }
    if c.u32()? != 0 {
        let why = c
            .u32()
            .ok()
            .and_then(|len| c.exact((len as usize).min(4096)).ok())
            .map(|b| String::from_utf8_lossy(&b).to_string())
            .unwrap_or_default();
        return Err(if why.is_empty() {
            "the screen's password was refused".into()
        } else {
            format!("the screen's password was refused: {why}")
        });
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn rect(
    c: &mut Conn,
    frame: &Mutex<Frame>,
    zrle: &mut Zrle,
    enc: i32,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
) -> Result<(), String> {
    match enc {
        0 => {
            let data = c.exact(w * h * 4)?;
            let mut f = frame.lock().unwrap_or_else(|e| e.into_inner());
            blit(&mut f, x, y, w, h, &data);
        }
        1 => {
            let (sx, sy) = (c.u16()? as usize, c.u16()? as usize);
            let mut f = frame.lock().unwrap_or_else(|e| e.into_inner());
            copy_rect(&mut f, sx, sy, x, y, w, h);
        }
        16 => {
            let len = c.u32()? as usize;
            let data = c.exact(len)?;
            let pixels = zrle.decode(&data, w, h)?;
            let mut f = frame.lock().unwrap_or_else(|e| e.into_inner());
            blit(&mut f, x, y, w, h, &pixels);
        }
        -223 => {
            let mut f = frame.lock().unwrap_or_else(|e| e.into_inner());
            f.w = w as u32;
            f.h = h as u32;
            f.px = opaque(w * h);
        }
        e => {
            return Err(format!(
                "the server used an encoding Squad didn't ask for ({e})"
            ))
        }
    }
    Ok(())
}

/// Puts `w`×`h` 4-byte pixels at (`x`, `y`), keeping alpha opaque.
fn blit(f: &mut Frame, x: usize, y: usize, w: usize, h: usize, data: &[u8]) {
    let fw = f.w as usize;
    let fh = f.h as usize;
    for row in 0..h {
        let ty = y + row;
        if ty >= fh {
            break;
        }
        let cols = w.min(fw.saturating_sub(x));
        let src = &data[row * w * 4..row * w * 4 + cols * 4];
        let dst = &mut f.px[(ty * fw + x) * 4..(ty * fw + x + cols) * 4];
        dst.copy_from_slice(src);
        for p in dst.as_chunks_mut::<4>().0 {
            p[3] = 255;
        }
    }
}

fn copy_rect(f: &mut Frame, sx: usize, sy: usize, x: usize, y: usize, w: usize, h: usize) {
    let fw = f.w as usize;
    let fh = f.h as usize;
    if sx + w > fw || x + w > fw || sy + h > fh || y + h > fh {
        return;
    }
    let mut tmp = vec![0u8; w * h * 4];
    for row in 0..h {
        let s = ((sy + row) * fw + sx) * 4;
        tmp[row * w * 4..(row + 1) * w * 4].copy_from_slice(&f.px[s..s + w * 4]);
    }
    blit(f, x, y, w, h, &tmp);
}

// ---------- ZRLE ----------

/// ZRLE keeps one zlib stream for the whole connection.
#[derive(Default)]
struct Zrle {
    z: Option<flate2::Decompress>,
}

impl Zrle {
    fn inflate(&mut self, data: &[u8]) -> Result<Vec<u8>, String> {
        let z = self.z.get_or_insert_with(|| flate2::Decompress::new(true));
        let mut out = Vec::with_capacity(data.len() * 4);
        let mut input = data;
        loop {
            if out.capacity() - out.len() < 64 * 1024 {
                out.reserve(256 * 1024);
            }
            let before_in = z.total_in();
            z.decompress_vec(input, &mut out, flate2::FlushDecompress::Sync)
                .map_err(|e| format!("the screen's data was garbled: {e}"))?;
            let used = (z.total_in() - before_in) as usize;
            input = &input[used..];
            if input.is_empty() && out.capacity() > out.len() {
                break;
            }
        }
        Ok(out)
    }

    /// A ZRLE rectangle as 4-byte BGRX pixels.
    fn decode(&mut self, data: &[u8], w: usize, h: usize) -> Result<Vec<u8>, String> {
        let raw = self.inflate(data)?;
        let mut r = Bytes { b: &raw, i: 0 };
        let mut out = vec![0u8; w * h * 4];
        let mut ty = 0;
        while ty < h {
            let th = 64.min(h - ty);
            let mut tx = 0;
            while tx < w {
                let tw = 64.min(w - tx);
                tile(&mut r, &mut out, w, tx, ty, tw, th)?;
                tx += 64;
            }
            ty += 64;
        }
        Ok(out)
    }
}

struct Bytes<'a> {
    b: &'a [u8],
    i: usize,
}

impl Bytes<'_> {
    fn u8(&mut self) -> Result<u8, String> {
        let v = *self.b.get(self.i).ok_or("the screen's data ran short")?;
        self.i += 1;
        Ok(v)
    }
    /// A compressed pixel: 3 bytes (B, G, R) for a 24-bit-deep 32-bit format.
    fn cpixel(&mut self) -> Result<[u8; 4], String> {
        Ok([self.u8()?, self.u8()?, self.u8()?, 255])
    }
    fn run(&mut self) -> Result<usize, String> {
        let mut n = 1;
        loop {
            let b = self.u8()? as usize;
            n += b;
            if b != 255 {
                return Ok(n);
            }
        }
    }
}

fn tile(
    r: &mut Bytes,
    out: &mut [u8],
    stride: usize,
    tx: usize,
    ty: usize,
    tw: usize,
    th: usize,
) -> Result<(), String> {
    let mut put = |i: usize, p: [u8; 4]| {
        let (x, y) = (tx + i % tw, ty + i / tw);
        let o = (y * stride + x) * 4;
        out[o..o + 4].copy_from_slice(&p);
    };
    let n = tw * th;
    let kind = r.u8()?;
    match kind {
        0 => {
            for i in 0..n {
                put(i, r.cpixel()?);
            }
        }
        1 => {
            let p = r.cpixel()?;
            for i in 0..n {
                put(i, p);
            }
        }
        2..=16 => {
            let pal: Vec<[u8; 4]> = (0..kind).map(|_| r.cpixel()).collect::<Result<_, _>>()?;
            let bits = match kind {
                2 => 1,
                3..=4 => 2,
                _ => 4,
            };
            for row in 0..th {
                let mut byte = 0u8;
                let mut left = 0;
                for col in 0..tw {
                    if left == 0 {
                        byte = r.u8()?;
                        left = 8;
                    }
                    left -= bits;
                    let idx = ((byte >> left) & ((1 << bits) - 1)) as usize;
                    put(row * tw + col, *pal.get(idx).ok_or("bad palette index")?);
                }
            }
        }
        128 => {
            let mut i = 0;
            while i < n {
                let p = r.cpixel()?;
                let len = r.run()?;
                for _ in 0..len.min(n - i) {
                    put(i, p);
                    i += 1;
                }
            }
        }
        130..=255 => {
            let size = (kind - 128) as usize;
            let pal: Vec<[u8; 4]> = (0..size).map(|_| r.cpixel()).collect::<Result<_, _>>()?;
            let mut i = 0;
            while i < n {
                let b = r.u8()?;
                let p = *pal.get((b & 0x7f) as usize).ok_or("bad palette index")?;
                let len = if b & 0x80 != 0 { r.run()? } else { 1 };
                for _ in 0..len.min(n - i) {
                    put(i, p);
                    i += 1;
                }
            }
        }
        k => return Err(format!("the screen's data was garbled (tile {k})")),
    }
    Ok(())
}

// ---------- VNC password auth: DES of the challenge ----------

/// The answer to a VNC challenge: the challenge DES-encrypted with the
/// password (8 bytes, each bit-reversed, as VNC does it).
pub fn vnc_auth(password: &str, challenge: &[u8]) -> Vec<u8> {
    let mut key = [0u8; 8];
    for (k, b) in key.iter_mut().zip(password.bytes()) {
        *k = b.reverse_bits();
    }
    let ks = des_keys(u64::from_be_bytes(key));
    challenge
        .chunks(8)
        .flat_map(|blk| {
            let mut b = [0u8; 8];
            b[..blk.len()].copy_from_slice(blk);
            des_block(u64::from_be_bytes(b), &ks).to_be_bytes()
        })
        .collect()
}

const IP: [u8; 64] = [
    58, 50, 42, 34, 26, 18, 10, 2, 60, 52, 44, 36, 28, 20, 12, 4, 62, 54, 46, 38, 30, 22, 14, 6,
    64, 56, 48, 40, 32, 24, 16, 8, 57, 49, 41, 33, 25, 17, 9, 1, 59, 51, 43, 35, 27, 19, 11, 3, 61,
    53, 45, 37, 29, 21, 13, 5, 63, 55, 47, 39, 31, 23, 15, 7,
];
const FP: [u8; 64] = [
    40, 8, 48, 16, 56, 24, 64, 32, 39, 7, 47, 15, 55, 23, 63, 31, 38, 6, 46, 14, 54, 22, 62, 30,
    37, 5, 45, 13, 53, 21, 61, 29, 36, 4, 44, 12, 52, 20, 60, 28, 35, 3, 43, 11, 51, 19, 59, 27,
    34, 2, 42, 10, 50, 18, 58, 26, 33, 1, 41, 9, 49, 17, 57, 25,
];
const E: [u8; 48] = [
    32, 1, 2, 3, 4, 5, 4, 5, 6, 7, 8, 9, 8, 9, 10, 11, 12, 13, 12, 13, 14, 15, 16, 17, 16, 17, 18,
    19, 20, 21, 20, 21, 22, 23, 24, 25, 24, 25, 26, 27, 28, 29, 28, 29, 30, 31, 32, 1,
];
const P: [u8; 32] = [
    16, 7, 20, 21, 29, 12, 28, 17, 1, 15, 23, 26, 5, 18, 31, 10, 2, 8, 24, 14, 32, 27, 3, 9, 19,
    13, 30, 6, 22, 11, 4, 25,
];
const PC1: [u8; 56] = [
    57, 49, 41, 33, 25, 17, 9, 1, 58, 50, 42, 34, 26, 18, 10, 2, 59, 51, 43, 35, 27, 19, 11, 3, 60,
    52, 44, 36, 63, 55, 47, 39, 31, 23, 15, 7, 62, 54, 46, 38, 30, 22, 14, 6, 61, 53, 45, 37, 29,
    21, 13, 5, 28, 20, 12, 4,
];
const PC2: [u8; 48] = [
    14, 17, 11, 24, 1, 5, 3, 28, 15, 6, 21, 10, 23, 19, 12, 4, 26, 8, 16, 7, 27, 20, 13, 2, 41, 52,
    31, 37, 47, 55, 30, 40, 51, 45, 33, 48, 44, 49, 39, 56, 34, 53, 46, 42, 50, 36, 29, 32,
];
const SHIFTS: [u8; 16] = [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1];
const S: [[u8; 64]; 8] = [
    [
        14, 4, 13, 1, 2, 15, 11, 8, 3, 10, 6, 12, 5, 9, 0, 7, 0, 15, 7, 4, 14, 2, 13, 1, 10, 6, 12,
        11, 9, 5, 3, 8, 4, 1, 14, 8, 13, 6, 2, 11, 15, 12, 9, 7, 3, 10, 5, 0, 15, 12, 8, 2, 4, 9,
        1, 7, 5, 11, 3, 14, 10, 0, 6, 13,
    ],
    [
        15, 1, 8, 14, 6, 11, 3, 4, 9, 7, 2, 13, 12, 0, 5, 10, 3, 13, 4, 7, 15, 2, 8, 14, 12, 0, 1,
        10, 6, 9, 11, 5, 0, 14, 7, 11, 10, 4, 13, 1, 5, 8, 12, 6, 9, 3, 2, 15, 13, 8, 10, 1, 3, 15,
        4, 2, 11, 6, 7, 12, 0, 5, 14, 9,
    ],
    [
        10, 0, 9, 14, 6, 3, 15, 5, 1, 13, 12, 7, 11, 4, 2, 8, 13, 7, 0, 9, 3, 4, 6, 10, 2, 8, 5,
        14, 12, 11, 15, 1, 13, 6, 4, 9, 8, 15, 3, 0, 11, 1, 2, 12, 5, 10, 14, 7, 1, 10, 13, 0, 6,
        9, 8, 7, 4, 15, 14, 3, 11, 5, 2, 12,
    ],
    [
        7, 13, 14, 3, 0, 6, 9, 10, 1, 2, 8, 5, 11, 12, 4, 15, 13, 8, 11, 5, 6, 15, 0, 3, 4, 7, 2,
        12, 1, 10, 14, 9, 10, 6, 9, 0, 12, 11, 7, 13, 15, 1, 3, 14, 5, 2, 8, 4, 3, 15, 0, 6, 10, 1,
        13, 8, 9, 4, 5, 11, 12, 7, 2, 14,
    ],
    [
        2, 12, 4, 1, 7, 10, 11, 6, 8, 5, 3, 15, 13, 0, 14, 9, 14, 11, 2, 12, 4, 7, 13, 1, 5, 0, 15,
        10, 3, 9, 8, 6, 4, 2, 1, 11, 10, 13, 7, 8, 15, 9, 12, 5, 6, 3, 0, 14, 11, 8, 12, 7, 1, 14,
        2, 13, 6, 15, 0, 9, 10, 4, 5, 3,
    ],
    [
        12, 1, 10, 15, 9, 2, 6, 8, 0, 13, 3, 4, 14, 7, 5, 11, 10, 15, 4, 2, 7, 12, 9, 5, 6, 1, 13,
        14, 0, 11, 3, 8, 9, 14, 15, 5, 2, 8, 12, 3, 7, 0, 4, 10, 1, 13, 11, 6, 4, 3, 2, 12, 9, 5,
        15, 10, 11, 14, 1, 7, 6, 0, 8, 13,
    ],
    [
        4, 11, 2, 14, 15, 0, 8, 13, 3, 12, 9, 7, 5, 10, 6, 1, 13, 0, 11, 7, 4, 9, 1, 10, 14, 3, 5,
        12, 2, 15, 8, 6, 1, 4, 11, 13, 12, 3, 7, 14, 10, 15, 6, 8, 0, 5, 9, 2, 6, 11, 13, 8, 1, 4,
        10, 7, 9, 5, 0, 15, 14, 2, 3, 12,
    ],
    [
        13, 2, 8, 4, 6, 15, 11, 1, 10, 9, 3, 14, 5, 0, 12, 7, 1, 15, 13, 8, 10, 3, 7, 4, 12, 5, 6,
        11, 0, 14, 9, 2, 7, 11, 4, 1, 9, 12, 14, 2, 0, 6, 10, 13, 15, 3, 5, 8, 2, 1, 14, 7, 4, 10,
        8, 13, 15, 12, 9, 0, 3, 5, 6, 11,
    ],
];

/// Picks bits of `v` (`width` bits wide, numbered from 1 at the top) in `table`'s order.
fn permute(v: u64, width: u32, table: &[u8]) -> u64 {
    table
        .iter()
        .fold(0, |acc, &b| (acc << 1) | ((v >> (width - b as u32)) & 1))
}

fn des_keys(key: u64) -> [u64; 16] {
    let k = permute(key, 64, &PC1);
    let (mut c, mut d) = ((k >> 28) & 0xfff_ffff, k & 0xfff_ffff);
    let mut out = [0u64; 16];
    for (i, s) in SHIFTS.iter().enumerate() {
        let s = *s as u32;
        c = ((c << s) | (c >> (28 - s))) & 0xfff_ffff;
        d = ((d << s) | (d >> (28 - s))) & 0xfff_ffff;
        out[i] = permute((c << 28) | d, 56, &PC2);
    }
    out
}

fn feistel(r: u32, k: u64) -> u32 {
    let x = permute(r as u64, 32, &E) ^ k;
    let mut o = 0u64;
    for (i, sbox) in S.iter().enumerate() {
        let six = ((x >> (42 - 6 * i)) & 0x3f) as usize;
        let row = ((six & 0x20) >> 4) | (six & 1);
        let col = (six >> 1) & 0xf;
        o = (o << 4) | sbox[row * 16 + col] as u64;
    }
    permute(o, 32, &P) as u32
}

fn des_block(block: u64, keys: &[u64; 16]) -> u64 {
    let b = permute(block, 64, &IP);
    let (mut l, mut r) = ((b >> 32) as u32, b as u32);
    for k in keys {
        let next = l ^ feistel(r, *k);
        l = r;
        r = next;
    }
    permute(((r as u64) << 32) | l as u64, 64, &FP)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn des_matches_the_textbook_example() {
        let ks = des_keys(0x133457799BBCDFF1);
        assert_eq!(des_block(0x0123456789ABCDEF, &ks), 0x85E813540F0AB405);
    }

    #[test]
    fn vnc_auth_is_des_with_bit_reversed_password() {
        let challenge: Vec<u8> = (0u8..16).collect();
        let mut key = [0u8; 8];
        for (k, b) in key.iter_mut().zip(b"j7sbya8S") {
            *k = b.reverse_bits();
        }
        let ks = des_keys(u64::from_be_bytes(key));
        let mut want = Vec::new();
        for blk in challenge.chunks(8) {
            let b: [u8; 8] = blk.try_into().unwrap();
            want.extend(des_block(u64::from_be_bytes(b), &ks).to_be_bytes());
        }
        assert_eq!(vnc_auth("j7sbya8S", &challenge), want);
        // Longer passwords are cut to 8 characters, as VNC does.
        assert_eq!(vnc_auth("j7sbya8Sxyz", &challenge), want);
    }

    #[test]
    fn keys_map_to_keysyms() {
        assert_eq!(keysym("enter", None), Some(0xff0d));
        assert_eq!(keysym("a", Some("a")), Some(0x61));
        assert_eq!(keysym("a", Some("A")), Some(0x41));
        assert_eq!(keysym("c", None), Some(0x63));
        assert_eq!(keysym("f5", None), Some(0xffc2));
        assert_eq!(keysym("e", Some("é")), Some(0xe9));
        assert_eq!(keysym("x", Some("€")), Some(0x0100_20ac));
        assert_eq!(keysym("shift", None), None);
    }

    #[test]
    fn zrle_tiles_decode() {
        let mut out = vec![0u8; 2 * 2 * 4];
        // A solid tile.
        let data = [1u8, 10, 20, 30];
        tile(&mut Bytes { b: &data, i: 0 }, &mut out, 2, 0, 0, 2, 2).unwrap();
        assert_eq!(&out[..4], &[10, 20, 30, 255]);
        assert_eq!(&out[12..], &[10, 20, 30, 255]);
        // A two-colour palette: 1 bit a pixel, rows padded to a byte.
        let data = [2u8, 1, 1, 1, 9, 9, 9, 0b0100_0000, 0b1000_0000];
        tile(&mut Bytes { b: &data, i: 0 }, &mut out, 2, 0, 0, 2, 2).unwrap();
        assert_eq!(out[0], 1);
        assert_eq!(out[4], 9);
        assert_eq!(out[8], 9);
        assert_eq!(out[12], 1);
        // Plain run-length: one colour for 4 pixels.
        let data = [128u8, 5, 6, 7, 3];
        tile(&mut Bytes { b: &data, i: 0 }, &mut out, 2, 0, 0, 2, 2).unwrap();
        assert!(out.chunks(4).all(|p| p == [5, 6, 7, 255]));
    }
}
