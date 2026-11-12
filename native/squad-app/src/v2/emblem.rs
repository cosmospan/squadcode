//! A project's emblem: a shape and a colour of its own, so two projects that
//! start with the same letters still look nothing alike in the switcher.
//!
//! You can pick one from a project's right-click menu. Until you do, Squad
//! picks one for it from the project's path, skipping shapes another project
//! already has, so no two look the same while there are shapes to go round.

use gpui_kit::assets::IconName;

use super::store::Folder;
use super::ui;

/// The shapes a project can take: the icon's file name, for saving, and the
/// icon itself.
pub(super) const SHAPES: [(&str, IconName); 28] = [
    ("zap", IconName::Zap),
    ("star", IconName::Star),
    ("waves-horizontal", IconName::WavesHorizontal),
    ("globe", IconName::Globe),
    ("mountain", IconName::Mountain),
    ("triangle", IconName::Triangle),
    ("leaf", IconName::Leaf),
    ("hexagon", IconName::Hexagon),
    ("diamond", IconName::Diamond),
    ("moon", IconName::Moon),
    ("heart", IconName::Heart),
    ("flag", IconName::Flag),
    ("rocket", IconName::Rocket),
    ("flame", IconName::Flame),
    ("sparkles", IconName::Sparkles),
    ("anchor", IconName::Anchor),
    ("feather", IconName::Feather),
    ("cpu", IconName::Cpu),
    ("box", IconName::Box),
    ("layers", IconName::Layers),
    ("sun", IconName::Sun),
    ("cloud", IconName::Cloud),
    ("gem", IconName::Gem),
    ("compass", IconName::Compass),
    ("puzzle", IconName::Puzzle),
    ("tree-pine", IconName::TreePine),
    ("atom", IconName::Atom),
    ("code-xml", IconName::CodeXml),
];

/// The colours an emblem can take.
pub(super) const TINTS: [u32; 8] = [
    ui::VIOLET,
    ui::GREEN,
    ui::ORANGE,
    ui::BLUE,
    ui::AMBER,
    ui::RED,
    0x14B8A6,
    0xEC4899,
];

/// One project's emblem, as places in [`SHAPES`] and [`TINTS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Emblem {
    pub shape: usize,
    pub tint: usize,
}

impl Emblem {
    pub fn icon(self) -> IconName {
        SHAPES[self.shape % SHAPES.len()].1
    }

    pub fn color(self) -> u32 {
        TINTS[self.tint % TINTS.len()]
    }
}

/// Where a saved shape name sits in [`SHAPES`].
pub(super) fn shape_of(name: &str) -> Option<usize> {
    SHAPES.iter().position(|(n, _)| *n == name)
}

fn hash(path: &str) -> u64 {
    // FNV-1a: the same answer on every run and every machine, unlike std's
    // hasher, so a project keeps its emblem.
    path.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100_0000_01b3)
    })
}

/// Every project's emblem, in the order of `folders`. The ones you picked
/// stay as they are; the rest get a shape no other project has, while there
/// are shapes to spare, seen in path order so adding a project never changes
/// another one's emblem unless they'd clash.
pub(super) fn all(folders: &[Folder]) -> Vec<Emblem> {
    let mut shape: Vec<Option<usize>> = folders.iter().map(|f| shape_of(&f.emblem)).collect();
    let mut used: Vec<bool> = vec![false; SHAPES.len()];
    for s in shape.iter().flatten() {
        used[*s] = true;
    }
    let mut order: Vec<usize> = (0..folders.len()).filter(|i| shape[*i].is_none()).collect();
    order.sort_by(|a, b| folders[*a].path.cmp(&folders[*b].path));
    for i in order {
        let start = (hash(&folders[i].path) % SHAPES.len() as u64) as usize;
        let pick = (0..SHAPES.len())
            .map(|k| (start + k) % SHAPES.len())
            .find(|s| !used[*s])
            .unwrap_or(start);
        used[pick] = true;
        shape[i] = Some(pick);
    }
    folders
        .iter()
        .zip(shape)
        .map(|(f, s)| {
            let shape = s.unwrap_or(0);
            let tint = f
                .tint
                .map(|t| t as usize % TINTS.len())
                .unwrap_or_else(|| (hash(&f.path) >> 17) as usize % TINTS.len());
            Emblem { shape, tint }
        })
        .collect()
}

/// The emblem for the project at `path`, as [`all`] gives it.
pub(super) fn of(folders: &[Folder], path: &str) -> Emblem {
    folders
        .iter()
        .position(|f| f.path == path)
        .and_then(|i| all(folders).get(i).copied())
        .unwrap_or(Emblem { shape: 0, tint: 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(path: &str) -> Folder {
        Folder {
            name: path.trim_start_matches('/').into(),
            path: path.into(),
            ..Default::default()
        }
    }

    #[test]
    fn projects_get_shapes_no_other_project_has() {
        let folders: Vec<Folder> = (0..SHAPES.len())
            .map(|i| folder(&format!("/p{i}")))
            .collect();
        let mut shapes: Vec<usize> = all(&folders).iter().map(|e| e.shape).collect();
        shapes.sort();
        shapes.dedup();
        assert_eq!(shapes.len(), SHAPES.len());
    }

    #[test]
    fn a_picked_shape_and_colour_stay_and_others_step_round_them() {
        let mut a = folder("/a");
        a.emblem = "rocket".into();
        a.tint = Some(2);
        let folders = vec![a, folder("/b"), folder("/c")];
        let e = all(&folders);
        assert_eq!(
            e[0],
            Emblem {
                shape: shape_of("rocket").unwrap(),
                tint: 2
            }
        );
        assert!(e[1..].iter().all(|x| x.shape != e[0].shape));
    }

    #[test]
    fn adding_a_project_leaves_the_others_alone_when_nothing_clashes() {
        let before = all(&[folder("/a"), folder("/b")]);
        let after = all(&[folder("/new"), folder("/a"), folder("/b")]);
        if after[0].shape != before[0].shape && after[0].shape != before[1].shape {
            assert_eq!(&after[1..], &before[..]);
        }
    }

    #[test]
    fn every_shape_name_is_found_again() {
        for (i, (name, _)) in SHAPES.iter().enumerate() {
            assert_eq!(shape_of(name), Some(i));
        }
        assert_eq!(shape_of(""), None);
    }
}
