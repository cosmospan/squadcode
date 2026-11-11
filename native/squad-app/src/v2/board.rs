//! A job's Design tab: the screens its bot designs in `design/`, as a board to look at
//! and comment on. The board also carries out the bot's design actions (see
//! `designdraw::agentops`), so a design job keeps one while it runs, whether or not
//! the tab is open.

use gpui_kit::*;
use std::path::Path;

use super::store::{Job, Status};
use super::Shell;
use crate::designdraw::view::{DesignCanvas, DesignEvent};

/// Whether a job has a Design tab: it designs in `design/`, or it was asked for design
/// work and may start to.
pub(super) fn has_design(job: &Job) -> bool {
    designs_here(job.dir()) || crate::browse::design::is_design_work(&job.title)
}

/// Whether the folder has screens to show.
fn designs_here(dir: &str) -> bool {
    crate::designdraw::model::folder(Path::new(dir))
        .join("screens")
        .is_dir()
}

impl Shell {
    /// The board for job `id`, made the first time it has screens. None before then.
    pub(super) fn board_for(
        &mut self,
        job: &Job,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<DesignCanvas>> {
        if let Some(b) = self.boards.get(&job.id) {
            return Some(b.clone());
        }
        if !designs_here(job.dir()) {
            return None;
        }
        let root = job.dir().into();
        let board = cx.new(|cx| DesignCanvas::board(root, window, cx));
        // Comments sent from the board reach the bot as the person's next words.
        let id = job.id;
        cx.subscribe(&board, move |this, _, ev: &DesignEvent, cx| {
            if let DesignEvent::Comments(text) = ev {
                this.follow_up(id, text.clone(), cx);
            }
        })
        .detach();
        self.boards.insert(job.id, board.clone());
        Some(board)
    }

    /// Keeps a board for every running job that designs, so its bot's design actions
    /// are carried out, and lets go of boards nobody needs: the job stopped and isn't
    /// the one on screen. Only the board on screen watches its folder.
    pub(super) fn keep_boards(
        &mut self,
        shown: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let running: Vec<Job> = self
            .crew
            .store
            .jobs
            .iter()
            .filter(|j| j.status == Status::Running && !self.boards.contains_key(&j.id))
            .filter(|j| has_design(j))
            .cloned()
            .collect();
        for job in running {
            self.board_for(&job, window, cx);
        }
        let store = &self.crew.store;
        self.boards.retain(|id, _| {
            Some(*id) == shown || store.job(*id).is_some_and(|j| j.status == Status::Running)
        });
        for (id, board) in &self.boards {
            let on = Some(*id) == shown;
            if board.read(cx).watching() != on {
                board.update(cx, |b, cx| b.set_watching(on, cx));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    #[test]
    fn a_job_has_a_design_tab_when_it_designs() {
        let dir = std::env::temp_dir().join(format!("squadcode-board-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut job = Job {
            title: "Fix the flaky login test".into(),
            ..Default::default()
        };
        job.folder.path = dir.to_string_lossy().into_owned();
        assert!(!has_design(&job));
        // Asked for design work: the tab is there, waiting for screens.
        job.title = "Design the booking app's home screen".into();
        assert!(has_design(&job));
        // Screens in design/ show it whatever the job was asked.
        job.title = "Fix the flaky login test".into();
        std::fs::create_dir_all(crate::designdraw::model::folder(&dir).join("screens")).unwrap();
        assert!(has_design(&job));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
