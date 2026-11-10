//! Themes for the v2 window, picked under Settings > Appearance.
//!
//! A theme is the neutral ground (page, panels, raised controls, hairlines), the
//! text levels, and the status colours as they read on that ground. The status and
//! member colours used as fills (`ui::GREEN`, `ui::AMBER`…) stay the same in every
//! theme, so a bot keeps its colour whatever the look.
//!
//! Frosted themes let the desktop show through the window, blurred. Their surfaces
//! carry an opacity in the top byte of the colour (`0xAARRGGBB`); a top byte of zero
//! means solid, so every plain `0xRRGGBB` colour still reads as it always did.
//! `ui::c` and `ui::ca` honour it.
//!
//! Window transparency, under Settings > Appearance, makes any theme see-through:
//! a switch and a slider thin every surface towards the theme's own floor, and lift
//! the quieter text towards the brightest, so it stays readable over the desktop.
//! With the switch off, every theme looks exactly as it is written below.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

/// Squad Code's own voice: its suggestions and the selected row.
pub const RUNO: u32 = 0x7B4DF5;

pub struct Theme {
    pub id: &'static str,
    /// Its name and line: the picker shows only the three looks now, but
    /// older themes keep theirs for anyone who still has one picked.
    #[allow(dead_code)]
    pub name: &'static str,
    /// One line under the name in the picker.
    #[allow(dead_code)]
    pub line: &'static str,
    /// Light ground: dark text, and the kit's text boxes and menus in light mode.
    pub light: bool,
    /// The window lets the desktop show through, blurred.
    pub frosted: bool,
    /// The page, panels on it, raised controls, and their hover and press fills.
    pub bg: u32,
    pub panel: u32,
    pub raised: u32,
    pub hover: u32,
    pub press: u32,
    /// Text, brightest first.
    pub ink: u32,
    pub body: u32,
    pub muted: u32,
    pub faint: u32,
    /// Hairlines: the usual one, a quieter one inside panels, a stronger one on hover.
    pub line_c: u32,
    pub line2: u32,
    pub line_hi: u32,
    /// The primary button, its hover and press, and the text on it.
    pub fill: u32,
    pub fill_hover: u32,
    pub fill_press: u32,
    pub on_fill: u32,
    /// The terminal and the diff: a sunken well under code.
    pub well: u32,
    /// Status colours as text: blue running, amber waiting on you, green good, red broken.
    pub t_blue: u32,
    pub t_amber: u32,
    pub t_green: u32,
    pub t_red: u32,
    pub t_violet: u32,
    /// Added and removed lines in a diff.
    pub add: u32,
    pub del: u32,
    /// The soft fill and edge behind each status pill: blue, amber, green, red, grey.
    pub tones: [(u32, u32); 5],
}

/// Blends `b` into `a` by `t` (0 is all `a`), on plain `0xRRGGBB` colours.
pub const fn mix(a: u32, b: u32, t: f32) -> u32 {
    let mut out = 0;
    let mut shift = 0;
    while shift <= 16 {
        let x = ((a >> shift) & 0xff) as f32;
        let y = ((b >> shift) & 0xff) as f32;
        let v = x + (y - x) * t;
        out |= ((v + 0.5) as u32 & 0xff) << shift;
        shift += 8;
    }
    out
}

/// A colour at an opacity, as frosted themes store it.
pub const fn see(rgb: u32, alpha: u8) -> u32 {
    (rgb & 0xff_ffff) | ((alpha as u32) << 24)
}

/// Pill fills and edges for a solid ground: each status colour, faintly over the page.
const fn tones(bg: u32, blue: u32, amber: u32, green: u32, red: u32, grey: u32) -> [(u32, u32); 5] {
    [
        (mix(bg, blue, 0.12), mix(bg, blue, 0.30)),
        (mix(bg, amber, 0.12), mix(bg, amber, 0.30)),
        (mix(bg, green, 0.12), mix(bg, green, 0.30)),
        (mix(bg, red, 0.12), mix(bg, red, 0.30)),
        (mix(bg, grey, 0.08), mix(bg, grey, 0.16)),
    ]
}

/// Pill fills and edges for a frosted ground: each status colour, see-through.
const fn glass_tones(blue: u32, amber: u32, green: u32, red: u32, grey: u32) -> [(u32, u32); 5] {
    [
        (see(blue, 0x24), see(blue, 0x55)),
        (see(amber, 0x24), see(amber, 0x55)),
        (see(green, 0x24), see(green, 0x55)),
        (see(red, 0x24), see(red, 0x55)),
        (see(grey, 0x14), see(grey, 0x26)),
    ]
}

pub const THEMES: [Theme; 14] = [
    // Graphite and lime: the default. A warm paper page, hairline edges, and
    // lime for the one thing to press. Lime is too pale to read as text on
    // paper, so text keeps to ink and the status colours.
    Theme {
        id: "graphite-lime",
        name: "Graphite and lime",
        line: "Warm paper and graphite ink, with lime for the one thing to press.",
        light: true,
        frosted: false,
        bg: 0xF6F6F1,
        panel: 0xFBFBF8,
        raised: 0xFFFFFF,
        hover: 0xE9E9E2,
        press: 0xDFDFD6,
        ink: 0x1A1A17,
        body: 0x3B3B36,
        muted: 0x5C5C54,
        faint: 0x76766C,
        line_c: 0xDAD9CF,
        line2: 0xE4E3DA,
        line_hi: 0xCFCEC3,
        fill: 0xC8F04B,
        fill_hover: 0xBCE63A,
        fill_press: 0xAED82E,
        on_fill: 0x151513,
        well: 0xEFEFE9,
        t_blue: 0x1D5AC4,
        t_amber: 0x8F5700,
        t_green: 0x0F7646,
        t_red: 0xB52634,
        t_violet: 0x5B3FC4,
        add: 0x0F7646,
        del: 0xB52634,
        tones: tones(0xF6F6F1, 0x2F7BEB, 0xD08A12, 0x1E9E62, 0xE0414F, 0x3B3B36),
    },
    // Graphite and lime after dark: the same design on warm graphite.
    Theme {
        id: "graphite-lime-dark",
        name: "Graphite and lime, dark",
        line: "Warm graphite and lime",
        light: false,
        frosted: false,
        bg: 0x1C1C1A,
        panel: 0x20201E,
        raised: 0x252523,
        hover: 0x2B2B28,
        press: 0x33332F,
        ink: 0xEDEDE6,
        body: 0xC9C9C0,
        muted: 0xA8A89F,
        faint: 0x85857B,
        line_c: 0x33332F,
        line2: 0x2A2A27,
        line_hi: 0x45453F,
        fill: 0xC8F04B,
        fill_hover: 0xD4F56A,
        fill_press: 0xB5DC3A,
        on_fill: 0x151513,
        well: 0x171715,
        t_blue: 0x86B6FF,
        t_amber: 0xF2B544,
        t_green: 0x7DDB8C,
        t_red: 0xFF7B6F,
        t_violet: 0xB9A8FF,
        add: 0x7DDB8C,
        del: 0xFF8A80,
        tones: tones(0x1C1C1A, 0x86B6FF, 0xF2B544, 0x7DDB8C, 0xFF6B5E, 0xC9C9C0),
    },
    // Harbour: cool slate paper and a periwinkle accent.
    Theme {
        id: "harbour",
        name: "Harbour",
        line: "Cool slate and navy ink, with a soft periwinkle to press.",
        light: true,
        frosted: false,
        bg: 0xF2F4F7,
        panel: 0xF8F9FB,
        raised: 0xFFFFFF,
        hover: 0xE0E5EB,
        press: 0xD4DBE3,
        ink: 0x131D29,
        body: 0x334155,
        muted: 0x475569,
        faint: 0x5F6E83,
        line_c: 0xD2D9E1,
        line2: 0xE1E6EC,
        line_hi: 0xC1CAD4,
        fill: 0xB8B2FF,
        fill_hover: 0xACA5FF,
        fill_press: 0x9F97F7,
        on_fill: 0x14102E,
        well: 0xEBEEF2,
        t_blue: 0x1A50B0,
        t_amber: 0x7F4D00,
        t_green: 0x0D6A3F,
        t_red: 0xA5222F,
        t_violet: 0x4B3DC2,
        add: 0x0D6A3F,
        del: 0xA5222F,
        tones: tones(0xF2F4F7, 0x2F7BEB, 0xD08A12, 0x1E9E62, 0xE0414F, 0x334155),
    },
    // Harbour after dark: deep harbour blue, the same periwinkle.
    Theme {
        id: "harbour-dark",
        name: "Harbour, dark",
        line: "Deep slate and periwinkle",
        light: false,
        frosted: false,
        bg: 0x141A22,
        panel: 0x171E27,
        raised: 0x1B222C,
        hover: 0x232C38,
        press: 0x2A3441,
        ink: 0xE6ECF3,
        body: 0xC6D0DC,
        muted: 0xA9B5C4,
        faint: 0x8592A3,
        line_c: 0x2A3441,
        line2: 0x222A35,
        line_hi: 0x374354,
        fill: 0xB8B2FF,
        fill_hover: 0xC6C1FF,
        fill_press: 0xA69FF5,
        on_fill: 0x14102E,
        well: 0x10151C,
        t_blue: 0x82ADF7,
        t_amber: 0xE6A84C,
        t_green: 0x62D29C,
        t_red: 0xF27D86,
        t_violet: 0xC4BFFF,
        add: 0x62D29C,
        del: 0xF27D86,
        tones: tones(0x141A22, 0x82ADF7, 0xE6A84C, 0x62D29C, 0xF27D86, 0xC6D0DC),
    },
    // Orchid: warm linen and an orchid pink accent.
    Theme {
        id: "orchid",
        name: "Orchid",
        line: "Warm sand and brown ink, with orchid pink to press.",
        light: true,
        frosted: false,
        bg: 0xF7F3EE,
        panel: 0xFBF8F4,
        raised: 0xFFFFFF,
        hover: 0xE8E1D7,
        press: 0xDED5C9,
        ink: 0x231C17,
        body: 0x433932,
        muted: 0x62564C,
        faint: 0x74675C,
        line_c: 0xDDD4C8,
        line2: 0xE6DFD5,
        line_hi: 0xCFC4B6,
        fill: 0xF4B8EC,
        fill_hover: 0xEFA8E5,
        fill_press: 0xE898DD,
        on_fill: 0x2A0D25,
        well: 0xF0EBE4,
        t_blue: 0x1A50B0,
        t_amber: 0x7F4D00,
        t_green: 0x0D6A3F,
        t_red: 0xA5222F,
        t_violet: 0x8E2479,
        add: 0x0D6A3F,
        del: 0xA5222F,
        tones: tones(0xF7F3EE, 0x2F7BEB, 0xD08A12, 0x1E9E62, 0xE0414F, 0x433932),
    },
    // Orchid after dark: warm espresso, the same orchid.
    Theme {
        id: "orchid-dark",
        name: "Orchid, dark",
        line: "Warm espresso and orchid",
        light: false,
        frosted: false,
        bg: 0x1E1A17,
        panel: 0x221E1A,
        raised: 0x28231F,
        hover: 0x312B26,
        press: 0x383029,
        ink: 0xF0E9E1,
        body: 0xD4CABF,
        muted: 0xBCB0A4,
        faint: 0x998D82,
        line_c: 0x383029,
        line2: 0x2E2823,
        line_hi: 0x463D35,
        fill: 0xF4B8EC,
        fill_hover: 0xF8CAF1,
        fill_press: 0xE8A3DE,
        on_fill: 0x2A0D25,
        well: 0x191512,
        t_blue: 0x82ADF7,
        t_amber: 0xE6A84C,
        t_green: 0x62D29C,
        t_red: 0xF27D86,
        t_violet: 0xF4B8EC,
        add: 0x62D29C,
        del: 0xF27D86,
        tones: tones(0x1E1A17, 0x82ADF7, 0xE6A84C, 0x62D29C, 0xF27D86, 0xD4CABF),
    },
    // Crew Chat: the default. A white page, a soft grey sidebar and Jobs panel,
    // and violet for the crew's own voice, so the bots' glowing faces stand out.
    Theme {
        id: "crew-chat",
        name: "Crew Chat",
        line: "Bright and calm, the default",
        light: true,
        frosted: false,
        bg: 0xFFFFFF,
        panel: 0xFFFFFF,
        raised: 0xFFFFFF,
        hover: 0xF1F1F5,
        press: 0xE6E6EC,
        ink: 0x16161D,
        body: 0x3A3A44,
        muted: 0x62626E,
        faint: 0x84848F,
        line_c: 0xE6E6EC,
        line2: 0xEDEDF2,
        line_hi: 0xD4D4DD,
        fill: 0x16161D,
        fill_hover: 0x2C2C35,
        fill_press: 0x42424C,
        on_fill: 0xFFFFFF,
        well: 0xF6F6F9,
        t_blue: 0x1D5AC4,
        t_amber: 0x9A5E00,
        t_green: 0x0F7646,
        t_red: 0xBA2836,
        t_violet: 0x5B47D6,
        add: 0x0F7646,
        del: 0xBA2836,
        tones: tones(0xFFFFFF, 0x2F7BEB, 0xD08A12, 0x1E9E62, 0xE0414F, 0x3A3A44),
    },
    // The original look: a dark, Linear-style ground.
    Theme {
        id: "midnight",
        name: "Midnight",
        line: "The original dark look",
        light: false,
        frosted: false,
        bg: 0x0D0D11,
        panel: 0x121217,
        raised: 0x18181F,
        hover: 0x1E1E26,
        press: 0x26262F,
        ink: 0xE9E9EF,
        body: 0xB9B9C4,
        muted: 0x8B8B97,
        faint: 0x6E6E7A,
        line_c: 0x24242D,
        line2: 0x1A1A21,
        line_hi: 0x34343F,
        fill: 0xFFFFFF,
        fill_hover: 0xDCDCE3,
        fill_press: 0xC8C8D0,
        on_fill: 0x0D0D11,
        well: 0x0A0A0D,
        t_blue: 0x86AEFF,
        t_amber: 0xFBBF4A,
        t_green: 0x5FE0A4,
        t_red: 0xFF8292,
        t_violet: 0xB9A2FF,
        add: 0x7EE2B0,
        del: 0xFF9AA6,
        tones: [
            (0x131C2F, 0x1C325A),
            (0x2B2010, 0x573B0F),
            (0x0F251F, 0x134733),
            (0x2B1219, 0x561A25),
            (0x1E1E26, 0x2A2A33),
        ],
    },
    // A softer, neutral grey for long sessions.
    Theme {
        id: "graphite",
        name: "Graphite",
        line: "Soft neutral grey",
        light: false,
        frosted: false,
        bg: 0x1C1C1F,
        panel: 0x232327,
        raised: 0x2A2A2F,
        hover: 0x313137,
        press: 0x3A3A41,
        ink: 0xEDEDF0,
        body: 0xC4C4CB,
        muted: 0x98989F,
        faint: 0x7C7C85,
        line_c: 0x34343A,
        line2: 0x2A2A2F,
        line_hi: 0x46464E,
        fill: 0xF4F4F6,
        fill_hover: 0xDDDDE2,
        fill_press: 0xC9C9CF,
        on_fill: 0x1C1C1F,
        well: 0x161618,
        t_blue: 0x8DB3FF,
        t_amber: 0xF7C25C,
        t_green: 0x6BDDA8,
        t_red: 0xFF8E9B,
        t_violet: 0xBFA9FF,
        add: 0x86E0B4,
        del: 0xFFA1AC,
        tones: tones(0x1C1C1F, 0x8DB3FF, 0xF7C25C, 0x6BDDA8, 0xFF8E9B, 0xC4C4CB),
    },
    // Deep navy, a cooler dark.
    Theme {
        id: "deep-sea",
        name: "Deep sea",
        line: "Cool navy blues",
        light: false,
        frosted: false,
        bg: 0x0A1020,
        panel: 0x0F1729,
        raised: 0x152034,
        hover: 0x1B2841,
        press: 0x23324F,
        ink: 0xE6ECF7,
        body: 0xB2BDD3,
        muted: 0x8592AC,
        faint: 0x6A7894,
        line_c: 0x1E2A42,
        line2: 0x162036,
        line_hi: 0x2D3C5B,
        fill: 0xF2F6FD,
        fill_hover: 0xD8E0EE,
        fill_press: 0xC4CEE0,
        on_fill: 0x0A1020,
        well: 0x070C18,
        t_blue: 0x7FB0FF,
        t_amber: 0xF8C056,
        t_green: 0x58DDB0,
        t_red: 0xFF8A9A,
        t_violet: 0xB4A6FF,
        add: 0x7BE3BE,
        del: 0xFF9FAD,
        tones: tones(0x0A1020, 0x7FB0FF, 0xF8C056, 0x58DDB0, 0xFF8A9A, 0xB2BDD3),
    },
    // A light ground for bright rooms.
    Theme {
        id: "daylight",
        name: "Daylight",
        line: "Light and crisp",
        light: true,
        frosted: false,
        bg: 0xF5F5F7,
        panel: 0xFFFFFF,
        raised: 0xFFFFFF,
        hover: 0xEDEDF1,
        press: 0xE2E2E8,
        ink: 0x17171D,
        body: 0x3E3E48,
        muted: 0x6A6A76,
        faint: 0x878793,
        line_c: 0xE1E1E7,
        line2: 0xECECF0,
        line_hi: 0xCBCBD4,
        fill: 0x17171D,
        fill_hover: 0x2E2E37,
        fill_press: 0x45454F,
        on_fill: 0xFFFFFF,
        well: 0xFAFAFB,
        t_blue: 0x2563D8,
        t_amber: 0x965700,
        t_green: 0x0F7246,
        t_red: 0xC02540,
        t_violet: 0x6236DB,
        add: 0x0F7246,
        del: 0xC02540,
        tones: tones(0xFFFFFF, 0x2563D8, 0xD48A0A, 0x13804F, 0xCC2B42, 0x3E3E48),
    },
    // Frosted dark glass: the desktop shows through, blurred.
    Theme {
        id: "frost",
        name: "Frost",
        line: "Dark frosted glass",
        light: false,
        frosted: true,
        bg: see(0x0D0D12, 0xD9),
        panel: see(0x17171E, 0xCC),
        raised: see(0x22222B, 0xD8),
        hover: see(0xFFFFFF, 0x14),
        press: see(0xFFFFFF, 0x22),
        ink: 0xF5F5F9,
        body: 0xD0D0D9,
        muted: 0xA2A2AE,
        faint: 0x85858F,
        line_c: see(0xFFFFFF, 0x1C),
        line2: see(0xFFFFFF, 0x10),
        line_hi: see(0xFFFFFF, 0x30),
        fill: 0xFFFFFF,
        fill_hover: 0xE4E4EA,
        fill_press: 0xD0D0D8,
        on_fill: 0x0D0D11,
        well: see(0x050507, 0xC8),
        t_blue: 0x93B8FF,
        t_amber: 0xFCC85E,
        t_green: 0x6EE8AE,
        t_red: 0xFF919F,
        t_violet: 0xC4B1FF,
        add: 0x86EAB8,
        del: 0xFFA4AF,
        tones: glass_tones(0x86AEFF, 0xFBBF4A, 0x5FE0A4, 0xFF8292, 0xFFFFFF),
    },
    // Frosted light glass, like a sheet of milky glass.
    Theme {
        id: "mist",
        name: "Mist",
        line: "Light frosted glass",
        light: true,
        frosted: true,
        bg: see(0xF2F2F6, 0xE4),
        panel: see(0xFFFFFF, 0xCC),
        raised: see(0xFFFFFF, 0xE6),
        hover: see(0x000000, 0x0D),
        press: see(0x000000, 0x18),
        ink: 0x101016,
        body: 0x2F2F39,
        muted: 0x585865,
        faint: 0x74747F,
        line_c: see(0x000000, 0x16),
        line2: see(0x000000, 0x0C),
        line_hi: see(0x000000, 0x28),
        fill: 0x15151B,
        fill_hover: 0x2C2C35,
        fill_press: 0x42424C,
        on_fill: 0xFFFFFF,
        well: see(0xFFFFFF, 0xD0),
        t_blue: 0x1A4BB0,
        t_amber: 0x7E4800,
        t_green: 0x0B6139,
        t_red: 0xA51D31,
        t_violet: 0x5A2DCC,
        add: 0x0B6139,
        del: 0xA51D31,
        tones: glass_tones(0x2563D8, 0xD48A0A, 0x13804F, 0xCC2B42, 0x000000),
    },
    // Frosted glass with a violet cast, in Squad Code's own colour.
    Theme {
        id: "aurora",
        name: "Aurora",
        line: "Violet-tinted glass",
        light: false,
        frosted: true,
        bg: see(0x120C24, 0xD9),
        panel: see(0x1B1433, 0xCC),
        raised: see(0x261D44, 0xD8),
        hover: see(0xC9B8FF, 0x16),
        press: see(0xC9B8FF, 0x24),
        ink: 0xF4F1FF,
        body: 0xD3CBEE,
        muted: 0xA69CC8,
        faint: 0x8A80AC,
        line_c: see(0xC9B8FF, 0x22),
        line2: see(0xC9B8FF, 0x14),
        line_hi: see(0xC9B8FF, 0x3A),
        fill: 0xFFFFFF,
        fill_hover: 0xE6E0FA,
        fill_press: 0xD3CAF2,
        on_fill: 0x120C24,
        well: see(0x08051A, 0xC8),
        t_blue: 0x97BAFF,
        t_amber: 0xFCC85E,
        t_green: 0x6EE8AE,
        t_red: 0xFF93A8,
        t_violet: 0xCDBBFF,
        add: 0x86EAB8,
        del: 0xFFA4B5,
        tones: glass_tones(0x86AEFF, 0xFBBF4A, 0x5FE0A4, 0xFF8292, 0xC9B8FF),
    },
];

static CURRENT: AtomicUsize = AtomicUsize::new(0);
/// Window transparency is on.
static SEE_THROUGH: AtomicBool = AtomicBool::new(false);
/// How see-through, 0 to 100, while it's on.
static CLEAR: AtomicU8 = AtomicU8::new(0);

/// How much a quieter line of text moves towards the brightest one at full
/// transparency, so it still reads over a busy desktop.
const LIFT: f32 = 0.85;

impl Theme {
    /// The opacity the transparency slider thins this theme's surfaces to at its
    /// far end: as far as its text stays readable over a busy desktop (checked in
    /// the tests below). A frosted theme's own opacity is thinned on top of this.
    pub fn floor(&self) -> f32 {
        match self.id {
            "graphite-lime" => 0.36,
            "graphite-lime-dark" => 0.60,
            "harbour" => 0.36,
            "harbour-dark" => 0.60,
            "orchid" => 0.36,
            "orchid-dark" => 0.60,
            "crew-chat" => 0.34,
            "midnight" => 0.58,
            "graphite" => 0.60,
            "deep-sea" => 0.58,
            "daylight" => 0.36,
            "frost" => 0.62,
            "mist" => 0.36,
            "aurora" => 0.64,
            _ => 0.8,
        }
    }
}

/// The theme in use.
pub fn theme() -> &'static Theme {
    &THEMES[CURRENT.load(Ordering::Relaxed).min(THEMES.len() - 1)]
}

/// Puts a theme in use by id; an unknown id means the default, Graphite and lime.
pub fn set_theme(id: &str) {
    let i = THEMES.iter().position(|t| t.id == id).unwrap_or(0);
    CURRENT.store(i, Ordering::Relaxed);
}

/// A look that comes light and dark: picked once, then shown in either, or in
/// whichever the system is using.
pub struct Family {
    pub id: &'static str,
    pub name: &'static str,
    pub line: &'static str,
    /// The theme ids for its light and dark sides.
    pub light: &'static str,
    pub dark: &'static str,
}

/// The three looks of the redesign, in the order Settings shows them.
pub const FAMILIES: [Family; 3] = [
    Family {
        id: "graphite-lime",
        name: "Graphite and lime",
        line: "Warm paper and graphite ink, with lime for the one thing to press.",
        light: "graphite-lime",
        dark: "graphite-lime-dark",
    },
    Family {
        id: "harbour",
        name: "Harbour",
        line: "Cool slate and navy ink, with a soft periwinkle to press.",
        light: "harbour",
        dark: "harbour-dark",
    },
    Family {
        id: "orchid",
        name: "Orchid",
        line: "Warm sand and brown ink, with orchid pink to press.",
        light: "orchid",
        dark: "orchid-dark",
    },
];

/// How a saved theme asks to follow the system: `<family>:system`.
pub const SYSTEM: &str = ":system";

/// 0 when the theme is fixed; otherwise one more than the family it follows.
static FOLLOW: AtomicUsize = AtomicUsize::new(0);

/// The family a theme id belongs to, if it's one of the redesign's looks.
pub fn family_of(id: &str) -> Option<&'static Family> {
    let fam = id.strip_suffix(SYSTEM).unwrap_or(id);
    FAMILIES
        .iter()
        .find(|f| f.id == fam || f.light == fam || f.dark == fam)
}

/// Puts the saved theme in use. A theme id is used as it is; `<family>:system`
/// picks the family's light or dark side to match the system (`dark`), and keeps
/// matching it through [`follow_system`].
pub fn set_pref(pref: &str, dark: bool) {
    match follows(pref) {
        Some(i) => {
            FOLLOW.store(i + 1, Ordering::Relaxed);
            follow_system(dark);
        }
        None => {
            FOLLOW.store(0, Ordering::Relaxed);
            set_theme(pref);
        }
    }
}

/// Which family `pref` follows the system with, if it does.
fn follows(pref: &str) -> Option<usize> {
    pref.strip_suffix(SYSTEM)
        .and_then(|f| FAMILIES.iter().position(|x| x.id == f))
}

/// The theme `pref` shows when the system is light or `dark`, if it follows it.
#[cfg(test)]
pub fn system_side(pref: &str, dark: bool) -> Option<&'static str> {
    follows(pref).map(|i| {
        if dark {
            FAMILIES[i].dark
        } else {
            FAMILIES[i].light
        }
    })
}

/// The theme follows the system's light or dark.
pub fn following() -> bool {
    FOLLOW.load(Ordering::Relaxed) > 0
}

/// When the theme follows the system, switches to the side that matches `dark`.
pub fn follow_system(dark: bool) {
    let i = FOLLOW.load(Ordering::Relaxed);
    if i == 0 {
        return;
    }
    let f = &FAMILIES[(i - 1).min(FAMILIES.len() - 1)];
    let id = if dark { f.dark } else { f.light };
    if theme().id != id {
        set_theme(id);
    }
}

/// Turns window transparency on or off, and sets how see-through it is (0 to 100).
pub fn set_see_through(on: bool, clear: u8) {
    SEE_THROUGH.store(on, Ordering::Relaxed);
    CLEAR.store(clear.min(100), Ordering::Relaxed);
}

/// Window transparency is switched on.
pub fn see_through() -> bool {
    SEE_THROUGH.load(Ordering::Relaxed)
}

/// The desktop shows through the window: a frosted theme, or transparency on.
pub fn window_clear() -> bool {
    theme().frosted || see_through()
}

/// A background image is set under Settings > Appearance.
static IMAGE: AtomicBool = AtomicBool::new(false);
/// How far the image shows through the window's surfaces, 0 to 100.
static IMAGE_SHOW: AtomicU8 = AtomicU8::new(0);

/// Puts a background image under the window, or takes it away, and sets how far it
/// shows through the surfaces over it (0 to 100). The image covers the desktop, so
/// while it's set it takes the place of window transparency.
pub fn set_image(on: bool, show: u8) {
    IMAGE.store(on, Ordering::Relaxed);
    IMAGE_SHOW.store(show.min(100), Ordering::Relaxed);
}

/// A background image lies under the window.
pub fn image_on() -> bool {
    IMAGE.load(Ordering::Relaxed)
}

/// Something shows through the window's surfaces: the desktop, or a background
/// image. Each surface then paints its own ground, since stacked see-through
/// layers would add up to nearly solid.
pub fn layered() -> bool {
    window_clear() || image_on()
}

/// How far transparency has gone, 0 (off, or the slider at its start) to 1. With
/// a background image it's the image's own slider; the same floor keeps text
/// readable over a busy picture as over a busy desktop.
pub fn amount() -> f32 {
    if image_on() {
        IMAGE_SHOW.load(Ordering::Relaxed).min(100) as f32 / 100.
    } else if see_through() {
        clear_amount()
    } else {
        0.
    }
}

/// Where the window transparency slider sits, 0 to 1, whether or not it's in use.
pub fn clear_amount() -> f32 {
    CLEAR.load(Ordering::Relaxed).min(100) as f32 / 100.
}

/// A surface thinned to `k` of its opacity. Faint overlays (a hover tint in a
/// frosted theme, under half opaque) are left as they are: they already show
/// what's under them.
pub const fn thin(color: u32, k: f32) -> u32 {
    let a = match color >> 24 {
        0 => 255,
        a => a,
    };
    if a < 0x80 || k >= 1. {
        return color;
    }
    let mut out = (a as f32 * k + 0.5) as u32;
    if out < 1 {
        out = 1;
    }
    see(color, out as u8)
}

/// A surface as the transparency setting leaves it.
pub fn glass(color: u32) -> u32 {
    glass_at(color, amount(), theme().floor())
}

/// A surface at transparency `t` (0 to 1) for a theme whose floor is `floor`.
const fn glass_at(color: u32, t: f32, floor: f32) -> u32 {
    if t <= 0. {
        return color;
    }
    thin(color, 1. - t * (1. - floor))
}

/// A line of text lifted towards the brightest, as the transparency setting needs.
fn lift(color: u32) -> u32 {
    lift_at(color, theme().ink, amount())
}

/// A line of text at transparency `t`, lifted towards `ink`.
const fn lift_at(color: u32, ink: u32, t: f32) -> u32 {
    if t <= 0. {
        return color;
    }
    mix(color, ink, LIFT * t)
}

/// The status pill colours: text, fill and edge.
pub type Tone = (u32, u32, u32);

pub fn bg() -> u32 {
    glass(theme().bg)
}
pub fn panel() -> u32 {
    glass(theme().panel)
}
pub fn raised() -> u32 {
    glass(theme().raised)
}
pub fn hover() -> u32 {
    glass(theme().hover)
}
/// A surface that floats over the page: the palette, menus, popovers and dialogs.
/// While the desktop shows through the window they can be see-through too, as far
/// as their own slider under Settings > Appearance says. The window only blurs the
/// desktop, not the page under a popover, so even at its clearest a popover keeps
/// `POP_FLOOR` of its opacity, enough that its text reads over the page.
pub fn float() -> u32 {
    float_at(theme(), pop_amount(), layered() && glass_popovers())
}

/// The least opacity a see-through popover goes down to, its slider all the way up.
const POP_FLOOR: f32 = 0.6;

/// A floating surface for theme `t`, at popover transparency `amount` (0 to 1),
/// see-through or not.
const fn float_at(t: &Theme, amount: f32, clear: bool) -> u32 {
    let rgb = t.raised & 0xff_ffff;
    if !clear {
        return rgb;
    }
    // A frosted theme starts at its own raised opacity; a solid one, nearly opaque.
    let base = match t.raised >> 24 {
        0 => 0.94,
        a => a as f32 / 255.,
    };
    let amount = if amount < 0. {
        0.
    } else if amount > 1. {
        1.
    } else {
        amount
    };
    let k = base - amount * (base - POP_FLOOR);
    see(rgb, (k * 255. + 0.5) as u8)
}

/// How strong a floating surface's drop shadow is, as a share of its solid
/// strength. The shadow is painted under the whole surface, not just around it,
/// so behind a see-through popover it would darken the very page that should show
/// through: it fades as the popover clears, to a third at its clearest.
pub fn float_shadow() -> f32 {
    shadow_for(float())
}

/// The shadow share for a floating surface of colour `color`.
const fn shadow_for(color: u32) -> f32 {
    let a = match color >> 24 {
        0 => return 1.,
        a => a as f32 / 255.,
    };
    let mut k = (a - POP_FLOOR) / (1. - POP_FLOOR);
    if k < 0. {
        k = 0.;
    }
    if k > 1. {
        k = 1.;
    }
    // Squared, so it lifts quickly off the page as the slider moves.
    0.3 + 0.7 * k * k
}

/// Popovers, menus and dialogs are see-through while the window is. On by default.
static GLASS_POPOVERS: AtomicBool = AtomicBool::new(true);
/// How see-through popovers are, 0 to 100, while they follow the window.
static POP_CLEAR: AtomicU8 = AtomicU8::new(40);

/// Lets popovers, menus and dialogs go see-through with the window, or keeps them
/// solid, and sets how see-through they go (0 to 100).
pub fn set_glass_popovers(on: bool, clear: u8) {
    GLASS_POPOVERS.store(on, Ordering::Relaxed);
    POP_CLEAR.store(clear.min(100), Ordering::Relaxed);
}

/// Popovers, menus and dialogs follow the window's transparency.
pub fn glass_popovers() -> bool {
    GLASS_POPOVERS.load(Ordering::Relaxed)
}

/// How far popovers have cleared, 0 to 1, by their own slider.
pub fn pop_amount() -> f32 {
    POP_CLEAR.load(Ordering::Relaxed).min(100) as f32 / 100.
}

pub fn press() -> u32 {
    glass(theme().press)
}
pub fn ink() -> u32 {
    theme().ink
}
pub fn body() -> u32 {
    lift(theme().body)
}
pub fn muted() -> u32 {
    lift(theme().muted)
}
pub fn faint() -> u32 {
    lift(theme().faint)
}
pub fn line() -> u32 {
    glass(theme().line_c)
}
pub fn line2() -> u32 {
    glass(theme().line2)
}
pub fn line_hi() -> u32 {
    glass(theme().line_hi)
}
pub fn fill() -> u32 {
    theme().fill
}
pub fn fill_hover() -> u32 {
    theme().fill_hover
}
pub fn fill_press() -> u32 {
    theme().fill_press
}
pub fn on_fill() -> u32 {
    theme().on_fill
}
pub fn well() -> u32 {
    glass(theme().well)
}
pub fn t_blue() -> u32 {
    lift(theme().t_blue)
}
pub fn t_amber() -> u32 {
    lift(theme().t_amber)
}
pub fn t_green() -> u32 {
    lift(theme().t_green)
}
pub fn t_red() -> u32 {
    lift(theme().t_red)
}
pub fn t_violet() -> u32 {
    lift(theme().t_violet)
}
pub fn add() -> u32 {
    lift(theme().add)
}
pub fn del() -> u32 {
    lift(theme().del)
}
/// The track of a switch that's off.
pub fn off() -> u32 {
    glass(theme().tones[4].1)
}

fn tone(i: usize, text: u32) -> Tone {
    let (fill, edge) = theme().tones[i];
    (text, glass(fill), glass(edge))
}
/// Running: blue.
pub fn run_tone() -> Tone {
    tone(0, t_blue())
}
/// Waiting on you: amber.
pub fn ask_tone() -> Tone {
    tone(1, t_amber())
}
/// Good: green.
pub fn ok_tone() -> Tone {
    tone(2, t_green())
}
/// Broken: red.
pub fn bad_tone() -> Tone {
    tone(3, t_red())
}
/// Neither: grey.
pub fn grey_tone() -> Tone {
    tone(4, body())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_graphite_lime_is_first() {
        assert_eq!(THEMES[0].id, "graphite-lime");
        for (i, a) in THEMES.iter().enumerate() {
            for b in &THEMES[i + 1..] {
                assert_ne!(a.id, b.id);
            }
        }
    }

    #[test]
    fn solid_themes_have_solid_surfaces() {
        for t in THEMES.iter().filter(|t| !t.frosted) {
            for s in [t.bg, t.panel, t.raised, t.hover, t.press, t.line_c, t.well] {
                assert_eq!(s >> 24, 0, "{} has a see-through surface", t.id);
            }
        }
    }

    #[test]
    fn text_is_always_solid() {
        for t in &THEMES {
            for s in [
                t.ink, t.body, t.muted, t.faint, t.t_blue, t.t_red, t.fill, t.on_fill,
            ] {
                assert_eq!(s >> 24, 0, "{} has see-through text", t.id);
            }
        }
    }

    /// Relative luminance, as WCAG defines it.
    fn lum(rgb: u32) -> f64 {
        let ch = |shift: u32| {
            let c = ((rgb >> shift) & 0xff) as f64 / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(16) + 0.7152 * ch(8) + 0.0722 * ch(0)
    }

    fn contrast(a: u32, b: u32) -> f64 {
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    /// `top` painted over the solid `under`, honouring its opacity byte.
    fn over(top: u32, under: u32) -> u32 {
        let a = match top >> 24 {
            0 => 1.0,
            a => a as f64 / 255.0,
        };
        let mut out = 0;
        for shift in [0, 8, 16] {
            let (t, u) = (
                ((top >> shift) & 0xff) as f64,
                ((under >> shift) & 0xff) as f64,
            );
            out |= ((t * a + u * (1.0 - a)).round() as u32) << shift;
        }
        out
    }

    /// Every level of text stays readable on every surface. A frosted theme is
    /// checked over the worst desktops behind it: white, black, grey, a saturated
    /// blue and a warm yellow.
    #[test]
    fn text_reads_on_every_surface() {
        let need = |t: &Theme| {
            [
                ("ink", t.ink, 7.0),
                ("body", t.body, 4.5),
                ("muted", t.muted, 4.5),
                ("faint", t.faint, 3.0),
                ("blue", t.t_blue, 4.5),
                ("amber", t.t_amber, 4.5),
                ("green", t.t_green, 4.5),
                ("red", t.t_red, 4.5),
                ("violet", t.t_violet, 4.5),
                ("add", t.add, 4.5),
                ("del", t.del, 4.5),
            ]
        };
        for t in &THEMES {
            let desks: &[u32] = if t.frosted {
                &[0xFFFFFF, 0x000000, 0x808080, 0x2050D0, 0xF0C040]
            } else {
                &[0x000000]
            };
            for &desk in desks {
                let page = over(t.bg, desk);
                let panel = over(t.panel, page);
                let surfaces = [
                    ("page", page),
                    ("panel", panel),
                    ("raised", over(t.raised, panel)),
                    ("hover", over(t.hover, panel)),
                ];
                for (name, color, min) in need(t) {
                    for (s, back) in surfaces {
                        let got = contrast(color, back);
                        assert!(
                            got >= min,
                            "{}: {name} on {s} over {desk:06x} is {got:.2}, needs {min}",
                            t.id
                        );
                    }
                }
                let pills = [t.t_blue, t.t_amber, t.t_green, t.t_red, t.body];
                for (i, text) in pills.into_iter().enumerate() {
                    let got = contrast(text, over(t.tones[i].0, panel));
                    assert!(got >= 4.5, "{}: pill {i} is {got:.2}", t.id);
                }
            }
            assert!(contrast(t.on_fill, t.fill) >= 7.0, "{}: button text", t.id);
        }
    }

    /// With window transparency all the way up, every theme's text still reads.
    /// The desktop behind is blurred, and checked at its worst for the theme's own
    /// ground: a dark theme over black, grey, a saturated blue and a warm yellow;
    /// a light one over white and the same colours. At the far end every level
    /// of text meets 4.5:1 (WCAG AA): the brightest gives up its usual 7:1 so the
    /// window can clear much further.
    #[test]
    fn text_reads_when_see_through() {
        for t in &THEMES {
            let (k, f) = (1.0, t.floor());
            let g = |c: u32| glass_at(c, k, f);
            let l = |c: u32| lift_at(c, t.ink, k);
            let desks: &[u32] = if t.light {
                &[0xFFFFFF, 0x808080, 0x2050D0, 0xF0C040]
            } else {
                &[0x000000, 0x808080, 0x2050D0, 0xF0C040]
            };
            let text = [
                ("ink", t.ink, 4.5),
                ("body", l(t.body), 4.5),
                ("muted", l(t.muted), 4.5),
                ("faint", l(t.faint), 3.0),
                ("blue", l(t.t_blue), 4.5),
                ("amber", l(t.t_amber), 4.5),
                ("green", l(t.t_green), 4.5),
                ("red", l(t.t_red), 4.5),
                ("violet", l(t.t_violet), 4.5),
                ("add", l(t.add), 4.5),
                ("del", l(t.del), 4.5),
            ];
            for &desk in desks {
                let page = over(g(t.bg), desk);
                let panel = over(g(t.panel), page);
                let surfaces = [
                    ("page", page),
                    ("panel", panel),
                    ("raised", over(g(t.raised), panel)),
                    ("hover", over(g(t.hover), panel)),
                ];
                for (name, color, min) in text {
                    for (s, back) in surfaces {
                        let got = contrast(color, back);
                        assert!(
                            got >= min,
                            "{} see-through: {name} on {s} over {desk:06x} is {got:.2}, needs {min}",
                            t.id
                        );
                    }
                }
            }
        }
    }

    /// A see-through popover keeps its text readable at any setting of its slider
    /// and the window's: over the page (on the theme's worst desktops), every level
    /// of text reads as it does elsewhere; over a line of the page's own text
    /// running right under a word on top, the popover's text still stands out.
    #[test]
    fn text_reads_on_see_through_popovers() {
        for t in &THEMES {
            for w in [0.0, 0.5, 1.0] {
                let l = |c: u32| lift_at(c, t.ink, w);
                let g = |c: u32| glass_at(c, w, t.floor());
                let desks: &[u32] = if t.light {
                    &[0xFFFFFF, 0x808080, 0x2050D0, 0xF0C040]
                } else {
                    &[0x000000, 0x808080, 0x2050D0, 0xF0C040]
                };
                for k in [0.0, 0.5, 1.0] {
                    let pop = float_at(t, k, true);
                    for &desk in desks {
                        let page = over(g(t.bg), desk);
                        let checks = [
                            (page, "page", [7.0, 4.5, 3.0]),
                            (over(t.ink, page), "page text", [3.0, 2.0, 1.5]),
                            (over(l(t.body), page), "page text", [3.0, 2.0, 1.5]),
                        ];
                        for (under, what, mins) in checks {
                            let back = over(pop, under);
                            let text = [("ink", t.ink), ("body", l(t.body)), ("muted", l(t.muted))];
                            for ((name, color), min) in text.into_iter().zip(mins) {
                                let got = contrast(color, back);
                                assert!(
                                    got >= min,
                                    "{} popover {k} window {w}: {name} over {what} {under:06x} on {desk:06x} is {got:.2}, needs {min}",
                                    t.id
                                );
                            }
                        }
                    }
                }
            }
            // Switched off, or with the window solid, a popover is opaque; the
            // slider at its start leaves it nearly so.
            assert_eq!(float_at(t, 1.0, false) >> 24, 0);
            assert!(float_at(t, 0.0, true) >> 24 >= 0xD8);
        }
    }

    #[test]
    fn popover_shadows_fade_as_they_clear() {
        for t in &THEMES {
            assert_eq!(shadow_for(float_at(t, 0.0, false)), 1.0);
            let near = shadow_for(float_at(t, 0.0, true));
            let clear = shadow_for(float_at(t, 1.0, true));
            assert!(clear < near, "{}", t.id);
            assert!((clear - 0.3).abs() < 0.01, "{}: {clear}", t.id);
        }
    }

    #[test]
    fn transparency_off_changes_nothing() {
        for t in &THEMES {
            for s in [t.bg, t.panel, t.hover, t.well, t.line_c] {
                assert_eq!(glass_at(s, 0., t.floor()), s);
            }
            assert_eq!(lift_at(t.muted, t.ink, 0.), t.muted);
        }
        // Faint overlays stay as they are; solid surfaces gain an opacity.
        assert_eq!(thin(see(0xFFFFFF, 0x14), 0.5), see(0xFFFFFF, 0x14));
        assert_eq!(thin(0x101010, 0.5) >> 24, 128);
    }

    #[test]
    fn families_point_at_real_themes() {
        for f in &FAMILIES {
            let light = THEMES.iter().find(|t| t.id == f.light).unwrap();
            let dark = THEMES.iter().find(|t| t.id == f.dark).unwrap();
            assert!(light.light && !dark.light, "{}", f.id);
            assert_eq!(family_of(f.dark).unwrap().id, f.id);
        }
    }

    #[test]
    fn following_the_system_picks_the_matching_side() {
        assert_eq!(system_side("harbour:system", true), Some("harbour-dark"));
        assert_eq!(system_side("harbour:system", false), Some("harbour"));
        assert_eq!(system_side("orchid-dark", false), None);
        assert_eq!(system_side("nothing:system", true), None);
    }

    #[test]
    fn mix_blends_each_channel() {
        assert_eq!(mix(0x000000, 0xFFFFFF, 0.0), 0x000000);
        assert_eq!(mix(0x000000, 0xFFFFFF, 1.0), 0xFFFFFF);
        assert_eq!(mix(0x000000, 0x204060, 0.5), 0x102030);
    }

    #[test]
    fn unknown_id_falls_back_to_graphite_lime() {
        set_theme("no-such-theme");
        assert_eq!(theme().id, "graphite-lime");
        set_theme("frost");
        assert_eq!(theme().id, "frost");
        set_theme("graphite-lime");
    }
}
